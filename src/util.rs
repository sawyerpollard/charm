//! Small shared helpers.

use std::process::Command;

/// True if running as root (euid 0). Uses `id -u` to avoid a libc dependency.
pub fn is_root() -> bool {
    Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim() == "0")
        .unwrap_or(false)
}
