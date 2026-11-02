//! Checks on the bytes we are about to write: ESP image headers and partition tables.

use sha2::{Digest, Sha256};

pub const PARTITION_TABLE_OFFSET: u32 = 0x8000;
pub const PARTITION_TABLE_LEN: usize = 0xC00;

/// The chip an ESP application or bootloader image was built for (its extended header).
pub fn image_chip(data: &[u8]) -> Option<&'static str> {
    if data.len() < 24 || data[0] != 0xE9 {
        return None;
    }
    match u16::from_le_bytes([data[12], data[13]]) {
        0 => Some("esp32"),
        2 => Some("esp32s2"),
        5 => Some("esp32c3"),
        9 => Some("esp32s3"),
        12 => Some("esp32c2"),
        13 => Some("esp32c6"),
        16 => Some("esp32h2"),
        _ => None,
    }
}

/// The version stamped into IR Blaster images ("IRB-FW-VERSION=26.11.01.2146").
pub fn firmware_version(data: &[u8]) -> Option<String> {
    const MARKER: &[u8] = b"IRB-FW-VERSION=";
    let start = data.windows(MARKER.len()).position(|w| w == MARKER)? + MARKER.len();
    let version: String = data[start..]
        .iter()
        .take_while(|b| b.is_ascii_digit() || **b == b'.')
        .map(|b| *b as char)
        .collect();
    let version = version.trim_end_matches('.').to_owned();
    (!version.is_empty()).then_some(version)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Partition {
    pub kind: u8,
    pub subtype: u8,
    pub offset: u32,
    pub size: u32,
    pub name: String,
}

/// The entries of a binary partition table (ESP-IDF format).
pub fn parse_partitions(table: &[u8]) -> Vec<Partition> {
    table
        .as_chunks::<32>()
        .0
        .iter()
        .take(PARTITION_TABLE_LEN / 32)
        .take_while(|entry| entry[..2] == [0xAA, 0x50])
        .map(|e| Partition {
            kind: e[2],
            subtype: e[3],
            offset: u32::from_le_bytes([e[4], e[5], e[6], e[7]]),
            size: u32::from_le_bytes([e[8], e[9], e[10], e[11]]),
            name: String::from_utf8_lossy(&e[12..28])
                .trim_end_matches('\0')
                .to_owned(),
        })
        .collect()
}

/// Size of the app partition that starts at `offset`.
pub fn app_slot_size(table: &[u8], offset: u32) -> Option<u32> {
    parse_partitions(table)
        .into_iter()
        .find(|p| p.kind == 0 && p.offset == offset)
        .map(|p| p.size)
}

pub fn is_blank(data: &[u8]) -> bool {
    data.iter().all(|b| *b == 0xFF)
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

#[cfg(test)]
pub fn md5(data: &[u8]) -> [u8; 16] {
    use md5::Md5;
    let digest = Md5::digest(data);
    let mut out = [0u8; 16];
    out.copy_from_slice(&digest);
    out
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// "4MB" -> bytes, as in firmware/boot/index.json.
pub fn parse_flash_size(text: &str) -> Option<u32> {
    let upper = text.trim().to_ascii_uppercase();
    let (number, scale) = match upper.strip_suffix("MB") {
        Some(n) => (n, 1024 * 1024),
        None => (upper.strip_suffix("KB")?, 1024),
    };
    number.trim().parse::<u32>().ok()?.checked_mul(scale)
}

pub fn format_flash_size(bytes: u32) -> String {
    if bytes >= 1024 * 1024 {
        format!("{} MB", bytes / (1024 * 1024))
    } else {
        format!("{} KB", bytes / 1024)
    }
}

pub fn format_size(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", bytes.div_ceil(1024))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NODE_TABLE: &[u8] = include_bytes!("../../firmware/boot/node-esp32/partitions.bin");
    const HUB_TABLE: &[u8] = include_bytes!("../../firmware/boot/hub-esp32/partitions.bin");

    #[test]
    fn reads_the_node_partition_table() {
        let parts = parse_partitions(NODE_TABLE);
        let names: Vec<_> = parts.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            ["nvs", "otadata", "app0", "app1", "spiffs", "coredump"]
        );
        assert_eq!(app_slot_size(NODE_TABLE, 0x10000), Some(0x1A0000));
        assert_eq!(app_slot_size(HUB_TABLE, 0x10000), Some(0x170000));
        assert_eq!(app_slot_size(NODE_TABLE, 0x20000), None);
    }

    #[test]
    fn partition_tables_carry_their_md5() {
        // the ESP-IDF format ends the entries with 0xEBEB, 14 x 0xFF and the MD5 of the entries
        let entries = parse_partitions(NODE_TABLE).len() * 32;
        assert_eq!(&NODE_TABLE[entries..entries + 2], &[0xEB, 0xEB]);
        assert_eq!(
            &NODE_TABLE[entries + 16..entries + 32],
            &md5(&NODE_TABLE[..entries])
        );
        assert_eq!(NODE_TABLE.len(), PARTITION_TABLE_LEN);
    }

    #[test]
    fn identifies_image_chips() {
        let esp32 = include_bytes!("../../firmware/boot/node-esp32/bootloader.bin");
        let c3 = include_bytes!("../../firmware/boot/node-esp32c3/bootloader.bin");
        assert_eq!(image_chip(esp32), Some("esp32"));
        assert_eq!(image_chip(c3), Some("esp32c3"));
        assert_eq!(image_chip(NODE_TABLE), None);
        assert_eq!(image_chip(&[0xE9]), None);
    }

    #[test]
    fn finds_the_version_marker() {
        assert_eq!(
            firmware_version(b"xxIRB-FW-VERSION=26.11.01.2146.\0yy").as_deref(),
            Some("26.11.01.2146")
        );
        assert_eq!(firmware_version(b"IRB-FW-VERSION=\0"), None);
        assert_eq!(firmware_version(b"nothing here"), None);
    }

    #[test]
    fn blank_and_sizes() {
        assert!(is_blank(&[0xFF; 32]));
        assert!(!is_blank(NODE_TABLE));
        assert_eq!(format_size(1_450_984), "1.38 MB");
        assert_eq!(format_size(17_536), "18 KB");
        assert_eq!(parse_flash_size("4MB"), Some(4 * 1024 * 1024));
        assert_eq!(parse_flash_size("512kb"), Some(512 * 1024));
        assert_eq!(parse_flash_size("big"), None);
        assert_eq!(format_flash_size(16 * 1024 * 1024), "16 MB");
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
