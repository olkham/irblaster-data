//! Command-line mode, for support and scripting. Without arguments the app opens its window.

use std::ffi::OsString;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use crate::device::{self, Installed};
use crate::failure::{Failure, Result};
use crate::recover::{self, Event, Firmware, Job};
use crate::release::{BootIndex, DEFAULT_BASE_URL, Product, Release, chip_label};

const USAGE: &str = "\
IR Blaster Recovery: reinstall the firmware of an IR Blaster node or hub over USB.

Usage:
  irblaster-recovery                         open the window
  irblaster-recovery --list                  list serial ports
  irblaster-recovery --info [--port PORT]    show the connected board and what it runs
  irblaster-recovery --recover [options]     reinstall the firmware

Options for --recover:
  --port PORT        the board's serial port (default: the only recognised one)
  --product node|hub what to install (default: what the board has now)
  --image FILE       flash this firmware file instead of downloading the published one
  --allow-unsigned   accept an --image file without a valid IR Blaster signature
  --erase            erase everything first: settings, WiFi and learned codes
  --yes              don't ask for confirmation

  --no-reset         the board is already in download mode (hold BOOT while plugging it in);
                     for boards that don't reset themselves (with --info and --recover)

Other options:
  --base-url URL     where firmware is published (default: https://olkham.github.io/irblaster-data/)
  --version          print the version
  --help             print this help
";

#[derive(Default)]
struct Args {
    mode: Option<&'static str>,
    port: Option<String>,
    product: Option<Product>,
    image: Option<PathBuf>,
    allow_unsigned: bool,
    erase: bool,
    yes: bool,
    no_reset: bool,
    base_url: Option<String>,
}

fn parse(args: Vec<OsString>) -> std::result::Result<Args, lexopt::Error> {
    use lexopt::prelude::*;
    let mut parser = lexopt::Parser::from_args(args);
    let mut out = Args::default();
    while let Some(arg) = parser.next()? {
        match arg {
            Long("list") => out.mode = Some("list"),
            Long("info") => out.mode = Some("info"),
            Long("recover") => out.mode = Some("recover"),
            Long("help") | Short('h') => out.mode = Some("help"),
            Long("version") | Short('V') => out.mode = Some("version"),
            Long("port") | Short('p') => out.port = Some(parser.value()?.string()?),
            Long("product") => {
                let value = parser.value()?.string()?;
                out.product = Some(
                    Product::parse(&value)
                        .ok_or_else(|| format!("--product is node or hub, not {value:?}"))?,
                );
            }
            Long("image") => out.image = Some(parser.value()?.into()),
            Long("allow-unsigned") => out.allow_unsigned = true,
            Long("erase") => out.erase = true,
            Long("yes") | Short('y') => out.yes = true,
            Long("no-reset") => out.no_reset = true,
            Long("base-url") => out.base_url = Some(parser.value()?.string()?),
            _ => return Err(arg.unexpected()),
        }
    }
    Ok(out)
}

pub fn run(args: Vec<OsString>) -> ExitCode {
    let args = match parse(args) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let result = match args.mode {
        Some("help") | None => {
            print!("{USAGE}");
            Ok(())
        }
        Some("version") => {
            println!("irblaster-recovery {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("list") => {
            list();
            Ok(())
        }
        Some("info") => info(&args),
        Some("recover") => recover_cmd(&args),
        Some(_) => unreachable!(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            eprintln!("error: {}", failure.message);
            if let Some(hint) = failure.hint {
                eprintln!("{hint}");
            }
            ExitCode::FAILURE
        }
    }
}

fn list() {
    let ports = device::list_ports();
    if ports.is_empty() {
        println!("No USB serial ports found.");
    }
    for port in ports {
        let mark = if port.recognised { "*" } else { " " };
        println!("{mark} {:<24} {}", port.name, port.description);
    }
}

fn pick_port(args: &Args) -> Result<String> {
    if let Some(port) = &args.port {
        return Ok(port.clone());
    }
    let ports = device::list_ports();
    let recognised: Vec<_> = ports.iter().filter(|p| p.recognised).collect();
    match recognised.as_slice() {
        [only] => Ok(only.name.clone()),
        _ if ports.len() == 1 => Ok(ports[0].name.clone()),
        _ if ports.is_empty() => Err(device::describe_failure_to_find(&ports)),
        _ => Err(device::describe_failure_to_find(&ports)
            .hint("Pass --port with one of the ports --list shows.")),
    }
}

fn fetch_release(args: &Args) -> Option<Release> {
    let base = args.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL);
    match Release::fetch(base) {
        Ok(release) => Some(release),
        Err(failure) => {
            eprintln!(
                "note: offline ({}); using the boot files built into this app",
                failure.message
            );
            None
        }
    }
}

fn installed_text(installed: Installed) -> String {
    match installed {
        Installed::Product(p) => p.label().to_owned(),
        Installed::Blank => "nothing (blank flash)".to_owned(),
        Installed::Unknown => "other firmware, or an older layout".to_owned(),
    }
}

fn info(args: &Args) -> Result<()> {
    let port = pick_port(args)?;
    let release = fetch_release(args);
    let boot = release
        .as_ref()
        .map(|r| r.boot.clone())
        .unwrap_or_else(BootIndex::embedded);
    println!("Connecting to {port}…");
    let info = device::probe(&port, args.no_reset, &boot)?;
    println!(
        "Chip:       {} {}",
        info.chip_label(),
        info.revision.as_deref().unwrap_or("")
    );
    println!("MAC:        {}", info.mac.as_deref().unwrap_or("unknown"));
    println!("Flash:      {}", info.flash_size);
    println!("Installed:  {}", installed_text(info.installed));
    if let Some(release) = &release {
        for product in release.products_for(&info.chip) {
            if let Some(build) = release.manifest.build(product, &info.chip) {
                println!("Available:  {} {}", product.label(), build.version);
            }
        }
    }
    Ok(())
}

fn recover_cmd(args: &Args) -> Result<()> {
    let port = pick_port(args)?;
    let release = fetch_release(args);
    let boot = release
        .as_ref()
        .map(|r| r.boot.clone())
        .unwrap_or_else(BootIndex::embedded);
    println!("Connecting to {port}…");
    let mut flasher = device::connect(&port, args.no_reset)?;
    let result = recover_connected(args, port, release, &boot, &mut flasher);
    if result.is_err() {
        device::restart(&mut flasher);
    }
    result
}

fn recover_connected(
    args: &Args,
    port: String,
    release: Option<Release>,
    boot: &BootIndex,
    flasher: &mut espflash::flasher::Flasher,
) -> Result<()> {
    let info = device::describe(flasher, &port, boot)?;
    println!(
        "Found an {} ({}), running {}.",
        info.chip_label(),
        info.mac.as_deref().unwrap_or("no MAC"),
        installed_text(info.installed)
    );

    let product = match (args.product, info.installed) {
        (Some(product), _) => product,
        (None, Installed::Product(product)) => product,
        (None, _) => {
            return Err(
                Failure::new("Can't tell whether this board is a node or a hub.")
                    .hint("Pass --product node or --product hub."),
            );
        }
    };
    let firmware = match &args.image {
        Some(path) => {
            let data = std::fs::read(path)
                .map_err(|e| Failure::new(format!("Can't read {}: {e}", path.display())))?;
            Firmware::File {
                path: path.clone(),
                data,
            }
        }
        None => {
            let release = release.as_ref().ok_or_else(|| {
                Failure::new("Can't download the firmware while offline.")
                    .hint("Pass --image with a firmware file you downloaded earlier.")
            })?;
            let build = release.manifest.build(product, &info.chip).ok_or_else(|| {
                Failure::new(format!(
                    "No {} firmware is published for {}.",
                    product.label(),
                    chip_label(&info.chip)
                ))
            })?;
            Firmware::Published(build.clone())
        }
    };
    let what = match &firmware {
        Firmware::Published(build) => format!("{} {}", product.label(), build.version),
        Firmware::File { path, .. } => format!("{} from {}", product.label(), path.display()),
    };
    let erase_note = if args.erase {
        " and ERASE everything on it first"
    } else {
        ""
    };
    println!("This will install {what} on the board on {port}{erase_note}.");
    if !args.yes && !confirm("Continue? [y/N] ") {
        return Err(Failure::new("Cancelled."));
    }

    let job = Job {
        port,
        chip: info.chip,
        product,
        firmware,
        erase: args.erase,
        allow_unsigned: args.allow_unsigned,
        no_reset: args.no_reset,
        release,
    };
    let mut last_percent = -1i32;
    let outcome = recover::run_connected(flasher, &job, &mut |event| match event {
        Event::Step(text) => println!("{text}"),
        Event::Log(text) => println!("  {text}"),
        Event::Progress(p) => {
            let percent = (p * 100.0) as i32;
            if percent / 10 != last_percent / 10 {
                println!("  {percent}%");
                last_percent = percent;
            }
        }
    })?;
    println!(
        "Done: {} {} installed{}. The board is restarting.",
        outcome.product.label(),
        outcome.version.as_deref().unwrap_or(""),
        if outcome.erased {
            " on an erased flash"
        } else {
            ""
        }
    );
    Ok(())
}

fn confirm(prompt: &str) -> bool {
    print!("{prompt}");
    let _ = io::stdout().flush();
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line).is_ok()
        && matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_options() {
        let parsed = parse(args(&[
            "--recover",
            "--port",
            "COM5",
            "--product",
            "hub",
            "--erase",
            "-y",
        ]))
        .unwrap();
        assert_eq!(parsed.mode, Some("recover"));
        assert_eq!(parsed.port.as_deref(), Some("COM5"));
        assert_eq!(parsed.product, Some(Product::Hub));
        assert!(parsed.erase && parsed.yes && !parsed.allow_unsigned);
        assert!(parse(args(&["--product", "toaster"])).is_err());
        assert!(parse(args(&["--frobnicate"])).is_err());
    }
}
