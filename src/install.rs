//! `charm install` - provision this host. Idempotent; safe to re-run.

use anyhow::{bail, Context, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

use crate::paths;

pub fn run(ssh_key: Option<&str>) -> Result<()> {
    if !crate::util::is_root() {
        bail!("charm install must run as root (try: sudo charm install ...)");
    }
    ensure_user()?;
    ensure_dirs()?;
    if let Some(key) = ssh_key {
        crate::keys::add(key)?;
    }
    ensure_docker_group()?;
    ensure_network()?;
    chown_home()?;

    println!("charm installed.");
    println!("  network : {} ({})", paths::NETWORK, paths::SUBNET);
    println!("  next    : register a domain  - sudo charm domain add <domain>");
    println!("            authorize a push key - sudo charm key add \"<your public key>\"");
    Ok(())
}

/// Run a command, erroring on non-zero exit.
fn sh(bin: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(bin)
        .args(args)
        .status()
        .with_context(|| format!("running `{bin}`"))?;
    if !status.success() {
        bail!("`{bin} {}` failed", args.join(" "));
    }
    Ok(())
}

fn user_exists() -> bool {
    Command::new("id")
        .arg(paths::USER)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn ensure_user() -> Result<()> {
    if user_exists() {
        return Ok(());
    }
    // A real shell is required so sshd can run the forced command; access is
    // restricted by the forced command + `charm shell`, not by a nologin shell.
    sh("useradd", &["-m", "-s", "/bin/bash", paths::USER])
}

fn ensure_dirs() -> Result<()> {
    for d in [paths::ssh_dir(), paths::repos(), paths::builds(), paths::state()] {
        fs::create_dir_all(&d).with_context(|| format!("creating {d}"))?;
    }
    fs::set_permissions(paths::ssh_dir(), fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn ensure_docker_group() -> Result<()> {
    // The `docker` group exists because Docker is installed.
    sh("usermod", &["-aG", "docker", paths::USER])
}

fn network_exists() -> bool {
    Command::new("docker")
        .args(["network", "inspect", paths::NETWORK])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn ensure_network() -> Result<()> {
    if network_exists() {
        return Ok(());
    }
    sh(
        "docker",
        &["network", "create", "--subnet", paths::SUBNET, paths::NETWORK],
    )
}

fn chown_home() -> Result<()> {
    let owner = format!("{}:{}", paths::USER, paths::USER);
    sh("chown", &["-R", &owner, paths::HOME])
}
