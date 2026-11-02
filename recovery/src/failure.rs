//! Errors as the user sees them: what went wrong, and what to try next.

use std::fmt;

#[derive(Debug, Clone)]
pub struct Failure {
    pub message: String,
    pub hint: Option<String>,
}

impl Failure {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            hint: None,
        }
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(hint) = &self.hint {
            write!(f, "\n{hint}")?;
        }
        Ok(())
    }
}

pub type Result<T, E = Failure> = std::result::Result<T, E>;

const BOOT_HINT: &str = "Use a USB cable that carries data (some only charge). If it still fails, hold the board's \
BOOT button while you plug it in, or hold BOOT and press RST/EN once, then try again.";

/// Text of an error and everything that caused it, for matching and for the details log.
fn chain(err: &(dyn std::error::Error + 'static)) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        if !text.contains(&cause_text) {
            text.push_str(": ");
            text.push_str(&cause_text);
        }
        source = cause.source();
    }
    text
}

fn io_kind(err: &(dyn std::error::Error + 'static)) -> Option<std::io::ErrorKind> {
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(err);
    while let Some(e) = current {
        if let Some(io) = e.downcast_ref::<std::io::Error>() {
            return Some(io.kind());
        }
        if let Some(serial) = e.downcast_ref::<serialport::Error>() {
            if let serialport::ErrorKind::Io(kind) = serial.kind() {
                return Some(kind);
            }
            if serial.kind() == serialport::ErrorKind::NoDevice {
                return Some(std::io::ErrorKind::NotFound);
            }
        }
        current = e.source();
    }
    None
}

/// Opening the serial port failed.
pub fn port_error(port: &str, err: &serialport::Error) -> Failure {
    let kind = match err.kind() {
        serialport::ErrorKind::Io(kind) => Some(kind),
        serialport::ErrorKind::NoDevice => Some(std::io::ErrorKind::NotFound),
        _ => None,
    };
    let text = err.to_string().to_lowercase();
    match kind {
        Some(std::io::ErrorKind::PermissionDenied) if cfg!(target_os = "linux") => {
            Failure::new(format!("This computer won't let the app use {port}."))
                .hint("Add yourself to the serial-port group, then log out and back in:\n    sudo usermod -aG dialout $USER\n(on Arch and Fedora the group may be uucp).")
        }
        Some(std::io::ErrorKind::PermissionDenied) | Some(std::io::ErrorKind::ResourceBusy) => {
            Failure::new(format!("{port} is in use by another program."))
                .hint("Close anything else that talks to the board, such as the Arduino IDE, PlatformIO or a serial monitor, then try again.")
        }
        _ if text.contains("access is denied") || text.contains("busy") => {
            Failure::new(format!("{port} is in use by another program."))
                .hint("Close anything else that talks to the board, such as the Arduino IDE, PlatformIO or a serial monitor, then try again.")
        }
        Some(std::io::ErrorKind::NotFound) => Failure::new(format!("{port} is no longer there."))
            .hint("Check the board is still plugged in, then pick its port again."),
        _ => Failure::new(format!("Couldn't open {port}: {err}")),
    }
}

/// A flashing operation failed.
pub fn flash_error(port: &str, err: espflash::Error) -> Failure {
    let detail = chain(&err);
    let lower = detail.to_lowercase();
    if io_kind(&err) == Some(std::io::ErrorKind::NotFound)
        || lower.contains("no such device")
        || lower.contains("device not configured")
    {
        return Failure::new("The board was unplugged or reset while working.")
            .hint("Plug it back in and start again. Nothing is lost by trying again.");
    }
    if lower.contains("secure download mode") {
        return Failure::new(
            "This board is locked (secure download mode), so it can't be reflashed over USB.",
        );
    }
    match err {
        espflash::Error::ChipMismatch(expected, found) => {
            Failure::new(format!("Expected an {expected} but found an {found}.")).hint("Connect the board again before recovering.")
        }
        espflash::Error::Connection(_) | espflash::Error::ChipDetectError(_) => {
            Failure::new(format!("Couldn't talk to the board on {port}.")).hint(BOOT_HINT)
        }
        espflash::Error::VerifyFailed | espflash::Error::DigestMismatch(..) => Failure::new("The board's flash didn't read back correctly after writing.")
            .hint("Try again, ideally with a shorter cable or another USB port. If it keeps failing, the board's flash may be faulty."),
        _ if lower.contains("timed out") || lower.contains("timeout") => {
            Failure::new("The board stopped answering.").hint(BOOT_HINT)
        }
        _ => Failure::new(format!("Flashing failed: {detail}")).hint(BOOT_HINT),
    }
}
