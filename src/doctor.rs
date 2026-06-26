//! `charm doctor` - verify host prerequisites without mutating anything.

use crate::style;
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::Duration;

pub fn run() -> anyhow::Result<()> {
    let root = crate::util::is_root();
    println!("charm doctor\n");
    let mut all_ok = true;
    all_ok &= report("git", cmd_ok("git", &["--version"]));
    // Reaching the Docker socket needs root or the docker group; a non-root
    // failure means "no access", not "daemon down" - so skip rather than fail.
    if root {
        all_ok &= report("docker daemon", cmd_ok("docker", &["info"]));
    } else {
        skip("docker daemon", "needs root (re-run with sudo)");
    }
    all_ok &= report("caddy", cmd_ok("caddy", &["version"]));
    all_ok &= report("systemd", std::path::Path::new("/run/systemd/system").exists());
    all_ok &= report("caddy admin API (127.0.0.1:2019)", admin_reachable());
    println!();
    if !root {
        println!("Docker check skipped - run `sudo charm doctor` for the full check.");
    }
    if !all_ok {
        println!("some checks failed - see above");
    } else if root {
        println!("all checks passed");
    }
    Ok(())
}

fn skip(name: &str, why: &str) {
    println!("  [{}] {name} {}", style::yellow("skip"), style::dim(why));
}

fn report(name: &str, ok: bool) -> bool {
    let tag = if ok {
        style::green("ok")
    } else {
        style::red("FAIL")
    };
    println!("  [{tag}] {name}");
    ok
}

fn cmd_ok(bin: &str, args: &[&str]) -> bool {
    Command::new(bin)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn admin_reachable() -> bool {
    let addr = "127.0.0.1:2019".parse().expect("valid socket addr");
    TcpStream::connect_timeout(&addr, Duration::from_secs(2)).is_ok()
}
