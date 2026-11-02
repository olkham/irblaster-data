//! What is published on the IR Blaster site: firmware/manifest.json (the OTA images the hub and
//! nodes use) and firmware/boot/index.json (the bootloader, partition table and OTA data that a USB
//! recovery writes first). A copy of the boot set is built into the app for offline use.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Deserialize;

use crate::failure::{Failure, Result};
use crate::image::sha256_hex;

pub const DEFAULT_BASE_URL: &str = "https://olkham.github.io/irblaster-data/";

/// Largest download we accept: bigger than any 4 MB board's flash.
const MAX_DOWNLOAD: u64 = 8 * 1024 * 1024;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded_boot.rs"));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Product {
    Node,
    Hub,
}

impl Product {
    pub const ALL: [Product; 2] = [Product::Node, Product::Hub];

    /// The name used in signatures and file names.
    pub fn id(self) -> &'static str {
        match self {
            Product::Node => "node",
            Product::Hub => "hub",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Product::Node => "IR Blaster node",
            Product::Hub => "IR Hub",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_lowercase().as_str() {
            "node" | "nodes" => Some(Product::Node),
            "hub" | "hubs" => Some(Product::Hub),
            _ => None,
        }
    }
}

pub fn chip_label(chip: &str) -> String {
    match chip {
        "esp32" => "ESP32".into(),
        "esp32c3" => "ESP32-C3".into(),
        "esp32s3" => "ESP32-S3".into(),
        "esp32s2" => "ESP32-S2".into(),
        "esp32c6" => "ESP32-C6".into(),
        other => other.to_uppercase(),
    }
}

/// An entry of firmware/manifest.json.
#[derive(Clone, Debug, Deserialize)]
pub struct Build {
    pub version: String,
    pub file: String,
    pub size: Option<u64>,
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub nodes: BTreeMap<String, Build>,
    #[serde(default)]
    pub hubs: BTreeMap<String, Build>,
}

impl Manifest {
    pub fn build(&self, product: Product, chip: &str) -> Option<&Build> {
        match product {
            Product::Node => self.nodes.get(chip),
            Product::Hub => self.hubs.get(chip),
        }
    }
}

/// One file of the boot set, written at `offset`.
#[derive(Clone, Debug, Deserialize)]
pub struct BootImage {
    pub name: String,
    pub offset: u32,
    pub file: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BootTarget {
    pub flash_size: String,
    pub app_offset: u32,
    pub images: Vec<BootImage>,
}

impl BootTarget {
    pub fn partitions(&self) -> Option<&BootImage> {
        self.images.iter().find(|image| image.name == "partitions")
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct BootIndex {
    #[serde(default)]
    pub nodes: BTreeMap<String, BootTarget>,
    #[serde(default)]
    pub hubs: BTreeMap<String, BootTarget>,
}

impl BootIndex {
    pub fn embedded() -> BootIndex {
        serde_json::from_str(embedded::INDEX).expect("the built-in boot index is valid")
    }

    pub fn target(&self, product: Product, chip: &str) -> Option<&BootTarget> {
        match product {
            Product::Node => self.nodes.get(chip),
            Product::Hub => self.hubs.get(chip),
        }
    }
}

/// Where the boot files come from.
#[derive(Clone, Debug)]
pub enum Source {
    /// The published site, at this base URL.
    Online(String),
    /// The copy built into this app.
    BuiltIn,
}

/// Everything published, as fetched at start-up.
#[derive(Clone, Debug)]
pub struct Release {
    pub base_url: String,
    pub manifest: Manifest,
    pub boot: BootIndex,
}

impl Release {
    pub fn fetch(base_url: &str) -> Result<Release> {
        let base_url = normalise(base_url);
        let manifest = download(&format!("{base_url}firmware/manifest.json"))?;
        let manifest: Manifest = serde_json::from_slice(&manifest).map_err(|e| {
            Failure::new(format!("The firmware list on the site is unreadable: {e}"))
        })?;
        let boot = download(&format!("{base_url}firmware/boot/index.json"))?;
        let boot: BootIndex = serde_json::from_slice(&boot).map_err(|e| {
            Failure::new(format!(
                "The recovery files list on the site is unreadable: {e}"
            ))
        })?;
        Ok(Release {
            base_url,
            manifest,
            boot,
        })
    }

    /// Products with both an image and a boot set for this chip.
    pub fn products_for(&self, chip: &str) -> Vec<Product> {
        Product::ALL
            .into_iter()
            .filter(|p| {
                self.manifest.build(*p, chip).is_some() && self.boot.target(*p, chip).is_some()
            })
            .collect()
    }

    pub fn download_build(&self, build: &Build) -> Result<Vec<u8>> {
        let data = download(&format!("{}firmware/{}", self.base_url, build.file))?;
        if let Some(size) = build.size
            && size != data.len() as u64
        {
            return Err(
                Failure::new(format!("{} downloaded incompletely.", build.file))
                    .hint("Check your connection and try again."),
            );
        }
        if let Some(expected) = &build.sha256
            && !expected.eq_ignore_ascii_case(&sha256_hex(&data))
        {
            return Err(Failure::new(format!("{} doesn't match the published checksum.", build.file))
                .hint("The download may have been damaged or the site is mid-update. Try again in a few minutes."));
        }
        Ok(data)
    }
}

pub fn normalise(base_url: &str) -> String {
    let trimmed = base_url.trim();
    if trimmed.ends_with('/') {
        trimmed.to_owned()
    } else {
        format!("{trimmed}/")
    }
}

/// The boot images of `target`, read from `source` and checked against their published checksums.
pub fn load_boot_images(target: &BootTarget, source: &Source) -> Result<Vec<(BootImage, Vec<u8>)>> {
    target
        .images
        .iter()
        .map(|image| {
            let data = match source {
                Source::Online(base_url) => {
                    download(&format!("{base_url}firmware/boot/{}", image.file))?
                }
                Source::BuiltIn => embedded::file(&image.file)
                    .ok_or_else(|| {
                        Failure::new(format!("{} is missing from this app.", image.file))
                    })?
                    .to_vec(),
            };
            if data.len() as u64 != image.size
                || !image.sha256.eq_ignore_ascii_case(&sha256_hex(&data))
            {
                return Err(Failure::new(format!(
                    "{} doesn't match its published checksum.",
                    image.file
                ))
                .hint("Try again in a few minutes, or update this app."));
            }
            Ok((image.clone(), data))
        })
        .collect()
}

fn agent() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_global(Some(Duration::from_secs(180)))
        .user_agent(concat!("irblaster-recovery/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

pub fn download(url: &str) -> Result<Vec<u8>> {
    let offline_hint = "Check this computer is online. To recover without internet, choose a firmware file you downloaded earlier.";
    let mut response = agent().get(url).call().map_err(|e| match e {
        ureq::Error::StatusCode(404) => Failure::new(format!("{url} isn't published (404).")),
        ureq::Error::StatusCode(code) => {
            Failure::new(format!("The site answered HTTP {code} for {url}.")).hint(offline_hint)
        }
        other => Failure::new(format!("Couldn't download {url}: {other}")).hint(offline_hint),
    })?;
    response
        .body_mut()
        .with_config()
        .limit(MAX_DOWNLOAD)
        .read_to_vec()
        .map_err(|e| Failure::new(format!("Download of {url} failed: {e}")).hint(offline_hint))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_boot_set_is_complete_and_consistent() {
        let index = BootIndex::embedded();
        let expected = [
            (Product::Node, "esp32"),
            (Product::Node, "esp32c3"),
            (Product::Hub, "esp32"),
        ];
        for (product, chip) in expected {
            let target = index
                .target(product, chip)
                .unwrap_or_else(|| panic!("no {product:?}/{chip}"));
            let images = load_boot_images(target, &Source::BuiltIn).unwrap();
            let names: Vec<_> = images.iter().map(|(i, _)| i.name.as_str()).collect();
            assert_eq!(names, ["bootloader", "partitions", "otadata"]);
            let (_, bootloader) = &images[0];
            assert_eq!(crate::image::image_chip(bootloader), Some(chip));
            let (_, table) = &images[1];
            assert!(crate::image::app_slot_size(table, target.app_offset).is_some());
            let bootloader_offset = if chip == "esp32" { 0x1000 } else { 0 };
            assert_eq!(images[0].0.offset, bootloader_offset);
            assert_eq!(images[1].0.offset, crate::image::PARTITION_TABLE_OFFSET);
        }
    }

    #[test]
    fn manifest_matches_published_files() {
        let manifest: Manifest = serde_json::from_str(include_str!("../../firmware/manifest.json"))
            .expect("manifest.json parses");
        let release = Release {
            base_url: "unused/".into(),
            manifest,
            boot: BootIndex::embedded(),
        };
        assert!(release.products_for("esp32").contains(&Product::Node));
        assert_eq!(release.products_for("esp32s2"), Vec::<Product>::new());
    }

    /// Serves the repository over HTTP on 127.0.0.1, like the published site.
    fn serve_repo() -> String {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/", listener.local_addr().unwrap());
        let root = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                reader.read_line(&mut request).unwrap();
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).unwrap() == 0 || header == "\r\n" {
                        break;
                    }
                }
                let path = request
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .trim_start_matches('/')
                    .to_owned();
                let file = root.join(&path);
                let (status, body) = match (path.contains(".."), std::fs::read(&file)) {
                    (false, Ok(body)) => ("200 OK", body),
                    _ => ("404 Not Found", b"missing".to_vec()),
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream
                    .write_all(head.as_bytes())
                    .and_then(|_| stream.write_all(&body));
            }
        });
        base
    }

    #[test]
    fn downloads_and_checks_the_published_files() {
        let base = serve_repo();
        let release = Release::fetch(&base).expect("fetch from the local site");
        assert!(release.products_for("esp32").contains(&Product::Hub));
        let build = release.manifest.build(Product::Node, "esp32").unwrap();
        let data = release.download_build(build).unwrap();
        assert_eq!(data.len() as u64, build.size.unwrap());
        let target = release.boot.target(Product::Hub, "esp32").unwrap();
        let images = load_boot_images(target, &Source::Online(release.base_url.clone())).unwrap();
        assert_eq!(images.len(), 3);

        // a checksum that doesn't match is refused
        let mut wrong = build.clone();
        wrong.sha256 = Some("00".repeat(32));
        assert!(
            release
                .download_build(&wrong)
                .unwrap_err()
                .message
                .contains("checksum")
        );
        let mut missing = build.clone();
        missing.file = "nope.bin".into();
        assert!(
            release
                .download_build(&missing)
                .unwrap_err()
                .message
                .contains("404")
        );
    }

    #[test]
    fn names() {
        assert_eq!(Product::parse("HUB"), Some(Product::Hub));
        assert_eq!(Product::parse("nodes"), Some(Product::Node));
        assert_eq!(Product::parse("x"), None);
        assert_eq!(chip_label("esp32c3"), "ESP32-C3");
        assert_eq!(normalise("http://x/y"), "http://x/y/");
    }
}
