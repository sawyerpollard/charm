//! Small shared helpers.

use anyhow::{bail, Result};
use std::process::Command;

use crate::paths;

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

pub fn require_root() -> Result<()> {
    if !is_root() {
        bail!("this command must run as root (try with sudo)");
    }
    Ok(())
}

/// `chown charm:charm <path>` - keep files Charm writes owned by the charm user.
pub fn chown_charm(path: &str) {
    let _ = Command::new("chown")
        .arg(format!("{}:{}", paths::USER, paths::USER))
        .arg(path)
        .status();
}
