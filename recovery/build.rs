//! Build script: embeds the boot set (firmware/boot/) so recovery from a local firmware file works
//! offline, and gives the Windows executable its icon and version details.

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let boot_dir = manifest_dir.join("..").join("firmware").join("boot");
    let index_path = boot_dir.join("index.json");
    println!("cargo:rerun-if-changed={}", boot_dir.display());
    println!("cargo:rerun-if-changed=trusted_keys.txt");
    println!("cargo:rerun-if-changed=assets/icon.ico");

    let index_text = fs::read_to_string(&index_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", index_path.display()));
    let index: serde_json::Value =
        serde_json::from_str(&index_text).expect("firmware/boot/index.json is not JSON");

    let mut files = Vec::new();
    for group in ["nodes", "hubs"] {
        if let Some(chips) = index.get(group).and_then(|g| g.as_object()) {
            for target in chips.values() {
                for image in target["images"].as_array().expect("images must be a list") {
                    let file = image["file"]
                        .as_str()
                        .expect("image without file")
                        .to_owned();
                    println!("cargo:rerun-if-changed={}", boot_dir.join(&file).display());
                    files.push(file);
                }
            }
        }
    }
    files.sort();
    files.dedup();

    let mut code = String::new();
    writeln!(
        code,
        "pub const INDEX: &str = include_str!({:?});",
        index_path.display().to_string()
    )
    .unwrap();
    writeln!(code, "pub fn file(path: &str) -> Option<&'static [u8]> {{").unwrap();
    writeln!(code, "    match path {{").unwrap();
    for file in &files {
        let full = boot_dir.join(file);
        assert!(
            full.is_file(),
            "{} is listed in index.json but missing",
            full.display()
        );
        writeln!(
            code,
            "        {file:?} => Some(include_bytes!({:?})),",
            full.display().to_string()
        )
        .unwrap();
    }
    writeln!(code, "        _ => None,").unwrap();
    writeln!(code, "    }}").unwrap();
    writeln!(code, "}}").unwrap();
    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("embedded_boot.rs"),
        code,
    )
    .unwrap();

    #[cfg(windows)]
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "IR Blaster Recovery");
        res.set("FileDescription", "IR Blaster Recovery");
        res.compile().expect("cannot compile the Windows resources");
    }
}
