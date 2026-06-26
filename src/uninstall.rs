//! `charm uninstall` - remove Charm's control plane.
//!
//! By default this leaves deployed app containers running (removing the *tool*
//! shouldn't ambush workloads). `--all` tears them down too; `--volumes` also
//! drops their data. Per-app teardown lives in `charm rm`; `--all` reuses it.

use anyhow::{bail, Result};
use std::process::{Command, Stdio};

use crate::paths;
use crate::util;

pub fn run(all: bool, volumes: bool) -> Result<()> {
    if !util::is_root() {
        bail!("charm uninstall must run as root (try: sudo charm uninstall ...)");
    }

    // Workloads first: containers Charm started are named `charm_<app>`.
    let containers = charm_containers();
    if !containers.is_empty() {
        if all {
            for c in &containers {
                teardown_container(c, volumes);
            }
        } else {
            eprintln!("{} app container(s) still deployed:", containers.len());
            for c in &containers {
                eprintln!("  - {c}");
            }
            eprintln!("Remove them with `charm rm <app>`, or re-run with --all (add --volumes to drop data).");
            bail!("refusing to remove the control plane while apps are deployed");
        }
    }

    // TODO(publish): best-effort `DELETE` of every `charm_*` route from Caddy's
    // admin API once routes are created. None exist yet.

    // Control plane:
    // 1. the user + all contained state (repos, builds, state, authorized_keys).
    if user_exists() {
        best_effort("userdel", &["-r", paths::USER]);
    }
    // 2. the Docker network (safe now that no charm containers are attached).
    best_effort("docker", &["network", "rm", paths::NETWORK]);
    // 3. the binary, last - safe to unlink a running executable on Linux.
    let _ = std::fs::remove_file(paths::BIN);

    println!("charm uninstalled. The box is back to its prior state.");
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

fn charm_containers() -> Vec<String> {
    let out = Command::new("docker")
        .args(["ps", "-a", "--filter", "name=charm_", "--format", "{{.Names}}"])
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

fn teardown_container(name: &str, volumes: bool) {
    let args: &[&str] = if volumes {
        &["rm", "-f", "-v", name]
    } else {
        &["rm", "-f", name]
    };
    best_effort("docker", args);
}

/// Run a command, ignoring failure - uninstall should never get stuck.
fn best_effort(bin: &str, args: &[&str]) {
    let _ = Command::new(bin)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}
