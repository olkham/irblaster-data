//! IR Blaster Recovery: reinstalls the firmware of an IR Blaster node or hub over USB.
//!
//! With no arguments it opens a window; with arguments it runs on the command line (see `--help`).

// A release build on Windows is a GUI program: no console window opens behind it. The command line
// still works from a terminal because main() attaches to the parent console.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod cli;
mod device;
mod failure;
mod gui;
mod image;
mod recover;
mod release;
mod signature;

use std::ffi::OsString;
use std::process::ExitCode;

fn main() -> ExitCode {
    // macOS passes -psn_* when an app is opened from the Finder on older systems: not a real argument.
    let args: Vec<OsString> = std::env::args_os()
        .skip(1)
        .filter(|a| !a.to_string_lossy().starts_with("-psn_"))
        .collect();
    if args.is_empty() {
        return gui::run();
    }
    #[cfg(windows)]
    attach_console();
    cli::run(args)
}

#[cfg(windows)]
fn attach_console() {
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE,
    };
    // SAFETY: plain Win32 calls without pointers. Only attach when there is no output handle yet
    // (started from a console), so redirected output (`> file`, pipes) keeps going where it was sent.
    unsafe {
        if GetStdHandle(STD_OUTPUT_HANDLE).is_null() {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}
