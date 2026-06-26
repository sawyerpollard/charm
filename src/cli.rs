//! CLI presentation: call the core (app / keys / domains), render text. The TUI
//! is the other frontend over the same core, so nothing here is business logic.

use anyhow::Result;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::Command;

use crate::app::{self, Health};
use crate::domains;
use crate::keys;
use crate::style;

// --- apps -----------------------------------------------------------------

pub fn list() -> Result<()> {
    let apps = app::summaries()?;
    if apps.is_empty() {
        println!("no apps deployed");
        return Ok(());
    }
    println!(
        "{}",
        style::bold(&format!("{:<18} {:<8} {:<28} UPSTREAM", "APP", "KIND", "URL"))
    );
    for a in &apps {
        println!(
            "{:<18} {:<8} {:<28} {}:{}",
            a.name,
            a.kind,
            format!("https://{}", a.host),
            a.ip,
            a.port
        );
    }
    Ok(())
}

pub fn status(name: &str) -> Result<()> {
    let s = app::status(name)?;
    println!("{}", style::bold(&s.name));
    field("kind", s.kind);
    field("url", &format!("https://{}", s.host));
    field("upstream", &format!("{}:{}", s.ip, s.port));
    if let Some(image) = &s.image {
        field("image", image);
    }
    field("container", &color_state(&s.container_state));
    field(
        "route",
        &if s.routed {
            style::green("published")
        } else {
            style::red("missing")
        },
    );
    println!();
    println!("{}", verdict(&s));
    Ok(())
}

pub fn start(name: &str) -> Result<()> {
    app::start(name)?;
    println!("charm: started '{name}'");
    Ok(())
}

pub fn stop(name: &str) -> Result<()> {
    app::stop(name)?;
    println!("charm: stopped '{name}' (route removed; image + repo kept)");
    Ok(())
}

pub fn restart(name: &str) -> Result<()> {
    app::restart(name)?;
    println!("charm: restarted '{name}'");
    Ok(())
}

pub fn rm(name: &str, volumes: bool) -> Result<()> {
    app::rm(name, volumes)?;
    println!("charm: removed '{name}'");
    Ok(())
}

pub fn publish(name: &str) -> Result<()> {
    app::publish(name)?;
    println!("charm: published '{name}'");
    Ok(())
}

pub fn unpublish(name: &str) -> Result<()> {
    app::unpublish(name)?;
    println!("charm: '{name}' removed from the proxy (container left running)");
    Ok(())
}

pub fn sync() -> Result<()> {
    let synced = app::sync()?;
    if synced.is_empty() {
        println!("charm: no apps to sync");
    }
    for name in synced {
        println!("charm: synced '{name}'");
    }
    Ok(())
}

pub fn logs(name: &str) -> Result<()> {
    let (prog, args) = app::logs_command(name)?;
    // exec replaces this process so `-f` streams until the user interrupts.
    let err = Command::new(prog).args(args).exec();
    Err(anyhow::anyhow!("failed to exec docker logs: {err}"))
}

fn field(label: &str, value: &str) {
    println!("  {:<10} {value}", label);
}

fn color_state(s: &str) -> String {
    match s {
        "running" => style::green(s),
        "missing" => style::red(s),
        other => style::yellow(other),
    }
}

fn verdict(s: &app::AppStatus) -> String {
    match s.health() {
        Health::Healthy => style::green("healthy"),
        Health::NotRouted => style::yellow("running but not routed - run `charm sync`"),
        Health::Stopped => style::yellow(&format!("stopped - run `charm start {}`", s.name)),
        Health::Missing => {
            style::red(&format!("container missing - run `charm start {}` or redeploy", s.name))
        }
    }
}

// --- keys -----------------------------------------------------------------

pub fn key_add(key: Option<String>) -> Result<()> {
    crate::util::require_root()?;
    let key = match key {
        Some(k) => k,
        None => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            s
        }
    };
    let desc = keys::add(key.trim())?;
    println!("authorized {desc}");
    Ok(())
}

pub fn key_list() -> Result<()> {
    let entries = keys::entries()?;
    if entries.is_empty() {
        println!("no keys authorized - add one with `charm key add`");
        return Ok(());
    }
    for (i, e) in entries.iter().enumerate() {
        println!("{}. {e}", i + 1);
    }
    Ok(())
}

pub fn key_remove(number: usize) -> Result<()> {
    let removed = keys::remove(number)?;
    println!("removed {removed}");
    Ok(())
}

// --- domains --------------------------------------------------------------

pub fn domain_add(domain: &str) -> Result<()> {
    let normalized = domain.trim().to_ascii_lowercase();
    if domains::add(domain)? {
        println!("registered domain: {normalized}");
    } else {
        println!("already registered: {normalized}");
    }
    Ok(())
}

pub fn domain_list() -> Result<()> {
    let entries = domains::entries()?;
    if entries.is_empty() {
        println!("no domains registered - add one with `charm domain add`");
        return Ok(());
    }
    for d in entries {
        println!("{d}");
    }
    Ok(())
}

pub fn domain_remove(domain: &str) -> Result<()> {
    domains::remove(domain)?;
    println!("removed domain: {}", domain.trim().to_ascii_lowercase());
    Ok(())
}
