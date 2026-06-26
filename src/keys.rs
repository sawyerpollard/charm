//! Manage the SSH public keys authorized to push - the forced-command lines in
//! /home/charm/.ssh/authorized_keys. `install` can seed an optional first key;
//! `charm key add/list/remove` manage them afterward.

use anyhow::{bail, Context, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;

use crate::paths;
use crate::util;

fn forced_command_line(key: &str) -> String {
    format!(
        "command=\"{} shell\",no-agent-forwarding,no-port-forwarding,no-X11-forwarding {}",
        paths::BIN,
        key.trim()
    )
}

/// Authorize a public key (idempotent; keyed on the key material, so re-adding
/// the same key even with a different comment is a no-op). Returns a one-line
/// description of the key. Shared by `install`.
pub fn add(key: &str) -> Result<String> {
    let key = key.trim();
    let material = match key_material(key) {
        Some(m) => m,
        None => bail!("not an SSH public key (expected e.g. `ssh-ed25519 AAAA... you@host`)"),
    };
    let desc = describe(&forced_command_line(key));

    fs::create_dir_all(paths::ssh_dir())?;
    let _ = fs::set_permissions(paths::ssh_dir(), fs::Permissions::from_mode(0o700));

    let path = paths::authorized_keys();
    let existing = fs::read_to_string(&path).unwrap_or_default();
    if existing.contains(material) {
        return Ok(desc);
    }
    let mut content = existing;
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&forced_command_line(key));
    content.push('\n');
    fs::write(&path, &content).with_context(|| format!("writing {path}"))?;
    let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    util::chown_charm(&path);
    Ok(desc)
}

/// Authorized keys as one-line descriptions, in order (index = number - 1).
pub fn entries() -> Result<Vec<String>> {
    let content = match fs::read_to_string(paths::authorized_keys()) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            bail!("cannot read authorized keys - run as root (sudo) or the charm user")
        }
        Err(_) => String::new(),
    };
    Ok(content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(describe)
        .collect())
}

/// Remove one authorized key by its number (from `key list`). A number is the
/// only handle: it's unambiguous even with missing or duplicate comments.
/// Returns the description of the removed key.
pub fn remove(number: usize) -> Result<String> {
    util::require_root()?;
    let path = paths::authorized_keys();
    let content = fs::read_to_string(&path).context("reading authorized_keys")?;
    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();

    if number == 0 || number > lines.len() {
        bail!("no key #{number} (see `charm key list`)");
    }
    let idx = number - 1;

    let removed = describe(lines[idx]);
    let kept: Vec<&str> = lines
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != idx)
        .map(|(_, l)| *l)
        .collect();
    let mut out = kept.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    fs::write(&path, &out).with_context(|| format!("writing {path}"))?;
    let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    util::chown_charm(&path);
    Ok(removed)
}

/// The base64 key material (2nd field), or None if it doesn't look like a key.
fn key_material(key: &str) -> Option<&str> {
    let mut it = key.split_whitespace();
    let typ = it.next()?;
    if !(typ.starts_with("ssh-") || typ.starts_with("ecdsa-") || typ.starts_with("sk-")) {
        return None;
    }
    it.next()
}

/// The "type b64 comment" part of a line, skipping the leading forced-command
/// options (which contain a space inside the `command="…"` quotes).
fn key_str(line: &str) -> &str {
    for marker in [" ssh-", " ecdsa-", " sk-"] {
        if let Some(idx) = line.find(marker) {
            return line[idx + 1..].trim();
        }
    }
    line.trim()
}

/// Human description, e.g. `ssh-ed25519  you@laptop`.
fn describe(line: &str) -> String {
    let mut it = key_str(line).split_whitespace();
    let typ = it.next().unwrap_or("?");
    let _b64 = it.next();
    let comment: Vec<&str> = it.collect();
    let comment = if comment.is_empty() {
        "(no comment)".to_string()
    } else {
        comment.join(" ")
    };
    format!("{typ}  {comment}")
}
