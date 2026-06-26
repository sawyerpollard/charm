//! Filesystem locations Charm owns.
//!
//! The `charm` user's home is fixed, so these are constants rather than
//! env-derived - `install` runs as root, `shell` runs as `charm`, and both must
//! agree on the same paths.

pub const USER: &str = "charm";
pub const HOME: &str = "/home/charm";
// /usr/bin (not /usr/local/bin) so `sudo charm` works everywhere: RHEL-family
// `secure_path` excludes /usr/local/bin, and /usr/bin is on every interactive PATH.
pub const BIN: &str = "/usr/bin/charm";
pub const NETWORK: &str = "charm";
pub const SUBNET: &str = "172.20.0.0/16";

pub fn ssh_dir() -> String {
    format!("{HOME}/.ssh")
}
pub fn authorized_keys() -> String {
    format!("{HOME}/.ssh/authorized_keys")
}
pub fn repos() -> String {
    format!("{HOME}/repos")
}
pub fn builds() -> String {
    format!("{HOME}/builds")
}
pub fn state() -> String {
    format!("{HOME}/state")
}
