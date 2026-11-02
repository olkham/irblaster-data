//! A recovery: get and check every file first, then erase if needed, write the boot set and the
//! firmware, verify, and restart the board. Shared by the window and the command line.

use std::borrow::Cow;
use std::path::PathBuf;

use espflash::flasher::Flasher;
use espflash::image_format::Segment;
use espflash::target::ProgressCallbacks;

use crate::device;
use crate::failure::{Failure, Result, flash_error};
use crate::image::{
    app_slot_size, firmware_version, format_flash_size, format_size, image_chip, is_blank,
    parse_flash_size,
};
use crate::release::{
    BootIndex, BootTarget, Build, Product, Release, Source, chip_label, load_boot_images,
};
use crate::signature::{self, Verdict};

#[derive(Clone, Debug)]
pub enum Firmware {
    /// The current published image for the product and chip.
    Published(Build),
    /// A file from this computer.
    File { path: PathBuf, data: Vec<u8> },
}

#[derive(Clone, Debug)]
pub struct Job {
    pub port: String,
    /// The chip found when the board was connected; the job refuses a different one.
    pub chip: String,
    pub product: Product,
    pub firmware: Firmware,
    /// Erase the whole flash first (settings, WiFi, learned codes).
    pub erase: bool,
    /// Flash a local file even though it has no valid signature.
    pub allow_unsigned: bool,
    /// The board is already in download mode: don't reset it with DTR/RTS.
    pub no_reset: bool,
    /// The published release, when online. Without it the built-in boot set is used.
    pub release: Option<Release>,
}

#[derive(Clone, Debug)]
pub enum Event {
    /// A new step, shown as the current activity.
    Step(String),
    /// Overall progress of the writing, 0.0 to 1.0.
    Progress(f32),
    /// A line for the details log.
    Log(String),
}

#[derive(Clone, Debug)]
pub struct Outcome {
    pub product: Product,
    pub version: Option<String>,
    pub erased: bool,
}

/// Does recovering `product` onto this board need a full erase? Yes unless the board already has
/// the same partition layout, or is blank.
pub fn erase_required(table_sha256: Option<&str>, target: &BootTarget, blank: bool) -> bool {
    if blank {
        return false;
    }
    let wanted = target.partitions().map(|p| p.sha256.as_str());
    !matches!((table_sha256, wanted), (Some(have), Some(want)) if have.eq_ignore_ascii_case(want))
}

struct Prepared {
    segments: Vec<(String, u32, Vec<u8>)>,
    target: BootTarget,
    version: Option<String>,
}

fn prepare(job: &Job, emit: &mut dyn FnMut(Event)) -> Result<Prepared> {
    let product = job.product;
    let chip = job.chip.as_str();
    let (boot_index, source) = match &job.release {
        Some(release) => (
            release.boot.clone(),
            Source::Online(release.base_url.clone()),
        ),
        None => (BootIndex::embedded(), Source::BuiltIn),
    };
    let target = boot_index.target(product, chip).cloned().ok_or_else(|| {
        Failure::new(format!(
            "There are no recovery files for an {} on {}.",
            product.label(),
            chip_label(chip)
        ))
    })?;

    emit(Event::Step("Getting the firmware…".into()));
    let (app, name) = match &job.firmware {
        Firmware::Published(build) => {
            let release = job.release.as_ref().ok_or_else(|| {
                Failure::new("The published firmware needs an internet connection.")
            })?;
            emit(Event::Log(format!(
                "Downloading {} ({})",
                build.file, build.version
            )));
            (release.download_build(build)?, build.file.clone())
        }
        Firmware::File { path, data } => {
            emit(Event::Log(format!("Using {}", path.display())));
            (
                data.clone(),
                path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            )
        }
    };

    match image_chip(&app) {
        None => return Err(Failure::new(format!("{name} isn't ESP32 firmware."))),
        Some(found) if found != chip => {
            return Err(Failure::new(format!(
                "{name} is built for {}, but this board is an {}.",
                chip_label(found),
                chip_label(chip)
            ))
            .hint("Choose the file for this board's chip."));
        }
        Some(_) => {}
    }

    match signature::check(product.id(), chip, &app) {
        Verdict::Valid => emit(Event::Log("Signature: valid IR Blaster signature".into())),
        Verdict::NotChecked => emit(Event::Log(
            "Signature: not checked (no trusted keys in this build)".into(),
        )),
        Verdict::Invalid => {
            return Err(Failure::new(format!(
                "{name} isn't signed as {} firmware for {}.",
                product.label(),
                chip_label(chip)
            ))
            .hint("Check you chose the right product, or use the published firmware."));
        }
        Verdict::Unsigned => {
            let local = matches!(job.firmware, Firmware::File { .. });
            if !(local && job.allow_unsigned) {
                return Err(Failure::new(format!("{name} isn't signed by IR Blaster."))
                    .hint("Only flash unsigned firmware you built yourself."));
            }
            emit(Event::Log(
                "Signature: none (allowed for this local file)".into(),
            ));
        }
    }

    let boot = load_boot_images(&target, &source)?;
    emit(Event::Log(match source {
        Source::Online(_) => "Boot files: published set".to_owned(),
        Source::BuiltIn => "Boot files: built into this app".to_owned(),
    }));
    let table = &boot
        .iter()
        .find(|(image, _)| image.name == "partitions")
        .ok_or_else(|| Failure::new("The boot set has no partition table."))?
        .1;
    let slot = app_slot_size(table, target.app_offset).ok_or_else(|| {
        Failure::new("The partition table has no app partition at the firmware's offset.")
    })?;
    if app.len() as u64 > slot as u64 {
        return Err(Failure::new(format!(
            "{name} ({}) is bigger than the {} firmware slot ({}).",
            format_size(app.len()),
            product.label(),
            format_size(slot as usize)
        ))
        .hint("Check you chose the right product."));
    }

    let version = firmware_version(&app);
    let mut segments: Vec<(String, u32, Vec<u8>)> = boot
        .into_iter()
        .map(|(image, data)| (image.name, image.offset, data))
        .collect();
    segments.push(("firmware".into(), target.app_offset, app));
    Ok(Prepared {
        segments,
        target,
        version,
    })
}

/// Progress across all segments, weighted by their size.
struct Progress<'a> {
    emit: &'a mut dyn FnMut(Event),
    spans: Vec<(u32, String, usize)>,
    total: usize,
    done_before: usize,
    current_size: usize,
    chunks: usize,
}

impl ProgressCallbacks for Progress<'_> {
    fn init(&mut self, addr: u32, total: usize) {
        let mut before = 0;
        for (offset, name, size) in &self.spans {
            if *offset == addr {
                self.current_size = *size;
                (self.emit)(Event::Step(format!("Writing the {name}…")));
                (self.emit)(Event::Log(format!(
                    "Writing {name} at 0x{addr:x} ({})",
                    format_size(*size)
                )));
                break;
            }
            before += size;
        }
        self.done_before = before;
        self.chunks = total.max(1);
    }

    fn update(&mut self, current: usize) {
        let part = self.current_size as f32 * (current as f32 / self.chunks as f32);
        (self.emit)(Event::Progress(
            ((self.done_before as f32 + part) / self.total.max(1) as f32).min(1.0),
        ));
    }

    fn verifying(&mut self) {
        (self.emit)(Event::Log("  verifying…".into()));
    }

    fn finish(&mut self, _skipped: bool) {
        let done = self.done_before + self.current_size;
        (self.emit)(Event::Progress(
            (done as f32 / self.total.max(1) as f32).min(1.0),
        ));
    }
}

/// Get everything, then connect and recover.
pub fn run(job: &Job, emit: &mut dyn FnMut(Event)) -> Result<Outcome> {
    let prepared = prepare(job, emit)?;
    emit(Event::Step("Connecting to the board…".into()));
    let mut flasher = device::connect(&job.port, job.no_reset)?;
    flash(&mut flasher, &prepared, job, emit)
}

/// Recover a board that is already connected.
pub fn run_connected(
    flasher: &mut Flasher,
    job: &Job,
    emit: &mut dyn FnMut(Event),
) -> Result<Outcome> {
    let prepared = prepare(job, emit)?;
    flash(flasher, &prepared, job, emit)
}

fn flash(
    flasher: &mut Flasher,
    prepared: &Prepared,
    job: &Job,
    emit: &mut dyn FnMut(Event),
) -> Result<Outcome> {
    let chip = device::chip_id(flasher);
    if chip != job.chip {
        return Err(Failure::new(format!(
            "The board on {} is now an {}, not the {} you connected.",
            job.port,
            chip_label(&chip),
            chip_label(&job.chip)
        ))
        .hint("Connect the board again before recovering."));
    }

    let flash = flasher
        .device_info()
        .map_err(|e| flash_error(&job.port, e))?
        .flash_size
        .size();
    if let Some(needed) = parse_flash_size(&prepared.target.flash_size)
        && flash < needed
    {
        return Err(Failure::new(format!(
            "This board has {} of flash, but the {} needs {}.",
            format_flash_size(flash),
            job.product.label(),
            format_flash_size(needed)
        )));
    }

    let table = device::read_partition_table(flasher);
    let blank = table.as_deref().is_some_and(is_blank);
    let table_sha = table.as_deref().map(crate::image::sha256_hex);
    let mut erase = job.erase;
    if !erase && erase_required(table_sha.as_deref(), &prepared.target, blank) {
        erase = true;
        emit(Event::Log("The board's storage layout differs from this firmware's, so the whole flash is erased.".into()));
    }
    if erase {
        emit(Event::Step("Erasing the flash (up to a minute)…".into()));
        emit(Event::Log("Erasing the whole flash".into()));
        flasher
            .erase_flash()
            .map_err(|e| flash_error(&job.port, e))?;
    }

    let spans: Vec<(u32, String, usize)> = prepared
        .segments
        .iter()
        .map(|(name, offset, data)| (*offset, segment_label(name), data.len()))
        .collect();
    let total = spans.iter().map(|(_, _, size)| size).sum();
    let segments: Vec<Segment<'_>> = prepared
        .segments
        .iter()
        .map(|(_, offset, data)| Segment {
            addr: *offset,
            data: Cow::Borrowed(data.as_slice()),
        })
        .collect();
    let mut progress = Progress {
        emit,
        spans,
        total,
        done_before: 0,
        current_size: 0,
        chunks: 1,
    };
    flasher
        .write_bins_to_flash(&segments, &mut progress)
        .map_err(|e| flash_error(&job.port, e))?;

    emit(Event::Step("Restarting the board…".into()));
    device::restart(flasher);
    emit(Event::Log("Done".into()));
    Ok(Outcome {
        product: job.product,
        version: prepared.version.clone(),
        erased: erase,
    })
}

fn segment_label(name: &str) -> String {
    match name {
        "bootloader" => "bootloader".into(),
        "partitions" => "partition table".into(),
        "otadata" => "boot selection".into(),
        other => other.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erases_only_when_the_layout_changes() {
        let boot = BootIndex::embedded();
        let node = boot.target(Product::Node, "esp32").unwrap();
        let hub = boot.target(Product::Hub, "esp32").unwrap();
        let node_sha = node.partitions().unwrap().sha256.clone();
        assert!(!erase_required(Some(&node_sha), node, false));
        assert!(erase_required(Some(&node_sha), hub, false));
        assert!(erase_required(None, node, false));
        assert!(!erase_required(None, node, true));
    }

    #[test]
    fn refuses_firmware_for_another_chip() {
        let c3_image = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../firmware/boot/node-esp32c3/bootloader.bin"
        ))
        .unwrap();
        let job = Job {
            port: "none".into(),
            chip: "esp32".into(),
            product: Product::Node,
            firmware: Firmware::File {
                path: "c3.bin".into(),
                data: c3_image,
            },
            erase: false,
            allow_unsigned: true,
            no_reset: false,
            release: None,
        };
        let err = run(&job, &mut |_| {}).unwrap_err();
        assert!(
            err.message.contains("built for ESP32-C3"),
            "{}",
            err.message
        );
    }

    #[test]
    fn refuses_firmware_too_big_for_the_slot() {
        let mut image = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../firmware/boot/hub-esp32/bootloader.bin"
        ))
        .unwrap();
        image.resize(0x170000 + 1, 0xFF);
        let job = Job {
            port: "none".into(),
            chip: "esp32".into(),
            product: Product::Hub,
            firmware: Firmware::File {
                path: "big.bin".into(),
                data: image,
            },
            erase: false,
            allow_unsigned: true,
            no_reset: false,
            release: None,
        };
        let err = run(&job, &mut |_| {}).unwrap_err();
        assert!(err.message.contains("bigger than"), "{}", err.message);
    }

    #[test]
    fn prepares_a_published_image_with_the_built_in_boot_set() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../firmware/");
        let manifest: crate::release::Manifest =
            serde_json::from_str(&std::fs::read_to_string(format!("{dir}manifest.json")).unwrap())
                .unwrap();
        let build = manifest.build(Product::Node, "esp32c3").unwrap().clone();
        let data = std::fs::read(format!("{dir}{}", build.file)).unwrap();
        let job = Job {
            port: "none".into(),
            chip: "esp32c3".into(),
            product: Product::Node,
            firmware: Firmware::File {
                path: build.file.clone().into(),
                data,
            },
            erase: false,
            allow_unsigned: false,
            no_reset: false,
            release: None,
        };
        let mut log = Vec::new();
        let prepared = prepare(&job, &mut |e| log.push(e)).unwrap();
        let layout: Vec<_> = prepared
            .segments
            .iter()
            .map(|(name, offset, _)| (name.as_str(), *offset))
            .collect();
        assert_eq!(
            layout,
            [
                ("bootloader", 0x0),
                ("partitions", 0x8000),
                ("otadata", 0xe000),
                ("firmware", 0x10000)
            ]
        );
        assert_eq!(prepared.version.as_deref(), Some(build.version.as_str()));
    }
}
