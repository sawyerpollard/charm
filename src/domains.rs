//! The domains Charm serves apps under - an allowlist, stored one-per-line in
//! `/home/charm/state/domains`. Every route's host must be, or be a subdomain
//! of, a registered domain. There is no bare-label expansion and no default.

use anyhow::{bail, Context, Result};
use std::fs;

use crate::paths;
use crate::util;

fn file() -> String {
    format!("{}/domains", paths::state())
}

/// Registered domains (lowercased, deduped on write). Empty if none/unreadable.
pub fn registered() -> Vec<String> {
    fs::read_to_string(file())
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// Register a domain. Returns true if newly added, false if already present.
pub fn add(domain: &str) -> Result<bool> {
    util::require_root()?;
    let domain = domain.trim().to_ascii_lowercase();
    if !looks_like_domain(&domain) {
        bail!("'{domain}' doesn't look like a domain (e.g. example.com)");
    }
    let mut list = registered();
    if list.contains(&domain) {
        return Ok(false);
    }
    list.push(domain);
    write(&list)?;
    Ok(true)
}

/// Registered domains as data (errors only on a real read failure).
pub fn entries() -> Result<Vec<String>> {
    match fs::read_to_string(file()) {
        Ok(c) => Ok(c.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            bail!("cannot read domains - run as root (sudo) or the charm user")
        }
        Err(e) => Err(e).context("reading domains"),
    }
}

pub fn remove(domain: &str) -> Result<()> {
    util::require_root()?;
    let domain = domain.trim().to_ascii_lowercase();
    let mut list = registered();
    let before = list.len();
    list.retain(|d| *d != domain);
    if list.len() == before {
        bail!("'{domain}' is not registered (see `charm domain list`)");
    }
    write(&list)
}

/// Error unless `host` is, or is a subdomain of, a registered domain.
pub fn ensure_allowed(host: &str) -> Result<()> {
    let host = host.to_ascii_lowercase();
    let list = registered();
    if list.is_empty() {
        bail!("no domains registered - run `charm domain add <domain>` first (for '{host}')");
    }
    let ok = list
        .iter()
        .any(|d| host == *d || host.ends_with(&format!(".{d}")));
    if !ok {
        bail!("host '{host}' is not under a registered domain - `charm domain add` it (see `charm domain list`)");
    }
    Ok(())
}

fn write(list: &[String]) -> Result<()> {
    let mut out = list.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    fs::write(file(), &out).with_context(|| format!("writing {}", file()))?;
    util::chown_charm(&file());
    Ok(())
}

fn looks_like_domain(d: &str) -> bool {
    d.contains('.')
        && !d.starts_with('.')
        && !d.ends_with('.')
        && d.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
}
