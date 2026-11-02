//! Serial ports and the ESP ROM bootloader, through espflash.

use std::time::Duration;

use espflash::connection::{Connection, ResetAfterOperation, ResetBeforeOperation};
use espflash::flasher::Flasher;
use serialport::{FlowControl, SerialPortType, UsbPortInfo};

use crate::failure::{Failure, Result, flash_error, port_error};
use crate::image::{
    PARTITION_TABLE_LEN, PARTITION_TABLE_OFFSET, format_flash_size, is_blank, sha256_hex,
};
use crate::release::{BootIndex, Product, chip_label};

/// Faster than the ROM's 115200 once the flasher stub runs, and still reliable on CH340 bridges.
const FLASH_BAUD: u32 = 460_800;

/// USB-serial bridges found on ESP32 boards, by vendor id.
const KNOWN_VENDORS: &[(u16, &str)] = &[
    (0x10c4, "Silicon Labs CP210x"),
    (0x1a86, "WCH CH340/CH9102"),
    (0x0403, "FTDI"),
    (0x303a, "Espressif USB"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortEntry {
    pub name: String,
    pub description: String,
    /// The USB device looks like an ESP32 board.
    pub recognised: bool,
}

/// USB serial ports, the ones that look like ESP32 boards first.
pub fn list_ports() -> Vec<PortEntry> {
    let mut ports: Vec<PortEntry> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| !(cfg!(target_os = "macos") && p.port_name.starts_with("/dev/tty.")))
        .filter_map(|p| match p.port_type {
            SerialPortType::UsbPort(usb) => {
                let vendor = KNOWN_VENDORS
                    .iter()
                    .find(|(vid, _)| *vid == usb.vid)
                    .map(|(_, name)| *name);
                let product = usb.product.clone().filter(|s| !s.trim().is_empty());
                let description = match (product, vendor) {
                    (Some(product), _) => product,
                    (None, Some(vendor)) => vendor.to_owned(),
                    (None, None) => format!("USB device {:04x}:{:04x}", usb.vid, usb.pid),
                };
                Some(PortEntry {
                    name: p.port_name,
                    description,
                    recognised: vendor.is_some(),
                })
            }
            _ => None,
        })
        .collect();
    ports.sort_by(|a, b| {
        b.recognised
            .cmp(&a.recognised)
            .then_with(|| a.name.cmp(&b.name))
    });
    ports
}

/// What a board currently has installed, judged by its partition table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Installed {
    Product(Product),
    Blank,
    /// Some other firmware, an older layout, or unreadable.
    Unknown,
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub port: String,
    pub chip: String,
    pub mac: Option<String>,
    pub flash_size: String,
    pub revision: Option<String>,
    pub installed: Installed,
    /// SHA-256 of the board's current partition table, if it could be read.
    pub table_sha256: Option<String>,
}

impl DeviceInfo {
    pub fn chip_label(&self) -> String {
        chip_label(&self.chip)
    }
}

/// Connect to the ROM bootloader. `no_reset`: the board is already in download mode (BOOT held while
/// it powered up), so don't toggle DTR/RTS; for boards without the auto-reset circuit.
pub fn connect(port: &str, no_reset: bool) -> Result<Flasher> {
    let info = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .find(|p| p.port_name == port)
        .and_then(|p| match p.port_type {
            SerialPortType::UsbPort(usb) => Some(usb),
            _ => None,
        })
        .unwrap_or(UsbPortInfo {
            vid: 0,
            pid: 0,
            serial_number: None,
            manufacturer: None,
            product: None,
        });
    let serial = serialport::new(port, 115_200)
        .flow_control(FlowControl::None)
        .timeout(Duration::from_secs(3))
        .open_native()
        .map_err(|e| port_error(port, &e))?;
    let before = if no_reset {
        ResetBeforeOperation::NoReset
    } else {
        ResetBeforeOperation::DefaultReset
    };
    let connection = Connection::new(
        serial,
        info,
        ResetAfterOperation::HardReset,
        before,
        115_200,
    );
    // stub on, verify after writing on, skip-identical-regions off
    Flasher::connect(connection, true, true, false, None, Some(FLASH_BAUD))
        .map_err(|e| flash_error(port, e))
}

pub fn chip_id(flasher: &Flasher) -> String {
    flasher.chip().to_string()
}

/// Read the partition table region (0xC00 bytes at 0x8000).
pub fn read_partition_table(flasher: &mut Flasher) -> Option<Vec<u8>> {
    if flasher.secure_download_mode() {
        return None;
    }
    let path = std::env::temp_dir().join(format!("irblaster-recovery-{}.bin", std::process::id()));
    let result = flasher.read_flash(
        PARTITION_TABLE_OFFSET,
        PARTITION_TABLE_LEN as u32,
        0x1000,
        64,
        path.clone(),
    );
    let data = result.ok().and_then(|_| std::fs::read(&path).ok());
    let _ = std::fs::remove_file(&path);
    data.filter(|d| d.len() == PARTITION_TABLE_LEN)
}

pub fn identify(table: Option<&[u8]>, chip: &str, boot: &BootIndex) -> Installed {
    let Some(table) = table else {
        return Installed::Unknown;
    };
    if is_blank(table) {
        return Installed::Blank;
    }
    let digest = sha256_hex(table);
    Product::ALL
        .into_iter()
        .find(|p| {
            boot.target(*p, chip)
                .and_then(|t| t.partitions())
                .is_some_and(|image| image.sha256.eq_ignore_ascii_case(&digest))
        })
        .map(Installed::Product)
        .unwrap_or(Installed::Unknown)
}

/// Restart the board into its firmware.
pub fn restart(flasher: &mut Flasher) {
    let chip = flasher.chip();
    let _ = flasher.connection().reset_after(true, chip);
}

/// Connect, describe the board and what it runs, and restart it.
pub fn probe(port: &str, no_reset: bool, boot: &BootIndex) -> Result<DeviceInfo> {
    let mut flasher = connect(port, no_reset)?;
    let info = describe(&mut flasher, port, boot);
    restart(&mut flasher);
    info
}

/// Describe a connected board and what it runs.
pub fn describe(flasher: &mut Flasher, port: &str, boot: &BootIndex) -> Result<DeviceInfo> {
    let info = flasher.device_info().map_err(|e| flash_error(port, e))?;
    let chip = chip_id(flasher);
    let table = read_partition_table(flasher);
    let installed = identify(table.as_deref(), &chip, boot);
    Ok(DeviceInfo {
        port: port.to_owned(),
        mac: info.mac_address.map(|m| m.to_string().to_uppercase()),
        flash_size: format_flash_size(info.flash_size.size()),
        revision: info
            .revision
            .map(|(major, minor)| format!("v{major}.{minor}")),
        installed,
        table_sha256: table.as_deref().map(sha256_hex),
        chip,
    })
}

pub fn describe_failure_to_find(ports: &[PortEntry]) -> Failure {
    if ports.is_empty() {
        Failure::new("No USB serial device found.").hint(
            "Plug the board in with a USB data cable. On Windows, boards with a CP210x or CH340 chip may need its driver \
             (Windows Update usually installs it within a minute).",
        )
    } else {
        Failure::new("More than one serial port: choose the board's port.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_installed_firmware_from_the_partition_table() {
        let boot = BootIndex::embedded();
        let node = include_bytes!("../../firmware/boot/node-esp32/partitions.bin");
        let hub = include_bytes!("../../firmware/boot/hub-esp32/partitions.bin");
        assert_eq!(
            identify(Some(node), "esp32", &boot),
            Installed::Product(Product::Node)
        );
        assert_eq!(
            identify(Some(hub), "esp32", &boot),
            Installed::Product(Product::Hub)
        );
        assert_eq!(
            identify(Some(node), "esp32c3", &boot),
            Installed::Product(Product::Node)
        );
        assert_eq!(
            identify(Some(&[0xFF; PARTITION_TABLE_LEN]), "esp32", &boot),
            Installed::Blank
        );
        assert_eq!(
            identify(Some(&[0u8; PARTITION_TABLE_LEN]), "esp32", &boot),
            Installed::Unknown
        );
        assert_eq!(identify(None, "esp32", &boot), Installed::Unknown);
    }
}
