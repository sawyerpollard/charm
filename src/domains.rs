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

pub fn add(domain: &str) -> Result<()> {
    util::require_root()?;
    let domain = domain.trim().to_ascii_lowercase();
    if !looks_like_domain(&domain) {
        bail!("'{domain}' doesn't look like a domain (e.g. example.com)");
    }
    let mut list = registered();
    if list.contains(&domain) {
        println!("already registered: {domain}");
        return Ok(());
    }
    list.push(domain.clone());
    write(&list)?;
    println!("registered domain: {domain}");
    Ok(())
}

pub fn list() -> Result<()> {
    let content = match fs::read_to_string(file()) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            bail!("cannot read domains - run as root (sudo) or the charm user")
        }
        Err(e) => return Err(e).context("reading domains"),
    };
    let domains: Vec<&str> = content.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if domains.is_empty() {
        println!("no domains registered - add one with `charm domain add`");
        return Ok(());
    }
    for d in domains {
        println!("{d}");
    }
    Ok(())
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
    write(&list)?;
    println!("removed domain: {domain}");
    Ok(())
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
