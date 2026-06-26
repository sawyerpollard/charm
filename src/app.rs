//! App lifecycle: deploy + the manual lifecycle/observability commands.
//!
//! Two deploy kinds:
//! - **Dockerfile** - one container Charm builds and runs as `charm_<app>`.
//! - **Compose** - the user's stack, brought up as project `charm_<app>`; the
//!   public service is attached to the `charm` network with a static IP via
//!   `docker network connect` (keeps the project's own networks intact).
//!
//! v0 scope: a single `[routes]` entry. Multi-route and zero-downtime swaps
//! are deferred.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::caddy;
use crate::paths;
use crate::style;

/// How an app runs - recorded in the manifest so lifecycle commands know how to
/// stop/start/remove it.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Runtime {
    Dockerfile { image: String },
    Compose { compose_file: String, service: String },
}

/// Per-app desired state - the source of truth Charm reconciles into Caddy.
#[derive(Serialize, Deserialize)]
pub struct AppState {
    pub host: String,
    pub port: u16,
    pub ip: String,
    pub runtime: Runtime,
}

/// Deploy-time plan, derived from the pushed repo's `Charm.toml`.
enum Plan {
    Dockerfile { host: String, port: u16 },
    Compose { host: String, compose_file: String, service: String, port: u16 },
}

fn container(app: &str) -> String {
    format!("charm_{app}")
}

/// Compose project name for an app.
fn project(app: &str) -> String {
    format!("charm_{app}")
}

// --- deploy (push-triggered: build + run + route) -------------------------

pub fn deploy(app: &str, repo: &str, gitref: &str, sha: &str) -> Result<()> {
    let branch = match gitref.strip_prefix("refs/heads/") {
        Some(b) if b == "main" || b == "master" => b,
        _ => {
            println!("charm: ignoring push to {gitref} (only main/master deploy)");
            return Ok(());
        }
    };
    let short = sha.get(..7).unwrap_or(sha);
    println!("charm: deploying '{app}' ({branch} @ {short})");

    // 1. Materialize the pushed tree (bare repos have no working tree).
    let build_dir = format!("{}/{}", paths::builds(), app);
    fs::create_dir_all(&build_dir)?;
    run("git", &["--git-dir", repo, "--work-tree", &build_dir, "checkout", "-f", branch])?;

    // 2. Plan from Charm.toml, 3. assign a stable IP.
    let plan = load_plan(&build_dir)?;
    let ip = assign_ip(app)?;

    // 4. Build + run, producing the runtime record + routing host/port.
    let (host, port, runtime) = match plan {
        Plan::Dockerfile { host, port } => {
            let image = format!("{}:{short}", container(app));
            run("docker", &["build", "-t", &image, &build_dir])?;
            rm_container(app);
            run_dockerfile(app, &ip, &image)?;
            (host, port, Runtime::Dockerfile { image })
        }
        Plan::Compose { host, compose_file, service, port } => {
            deploy_compose(app, &compose_file, &service, &ip)?;
            (host, port, Runtime::Compose { compose_file, service })
        }
    };

    // 5. Record desired state, then route it.
    let st = AppState { host, port, ip, runtime };
    save_state(app, &st)?;
    caddy::publish(app, &st.host, &st.ip, st.port)?;
    println!(
        "{}",
        style::green(&format!("charm: '{app}' is live at https://{}", st.host))
    );
    Ok(())
}

/// Bring a compose stack up and attach its public service to the charm net.
fn deploy_compose(app: &str, compose_file: &str, service: &str, ip: &str) -> Result<()> {
    let project = project(app);
    run("docker", &["compose", "-p", &project, "-f", compose_file, "up", "-d", "--build"])?;

    let cid = compose_container(&project, compose_file, service)?;
    // Clean (re)attach with the assigned static IP - a redeploy recreates the
    // container, so disconnect any stale attachment first (best-effort).
    let _ = Command::new("docker")
        .args(["network", "disconnect", paths::NETWORK, &cid])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    run("docker", &["network", "connect", "--ip", ip, paths::NETWORK, &cid])?;
    Ok(())
}

fn compose_container(project: &str, compose_file: &str, service: &str) -> Result<String> {
    let out = Command::new("docker")
        .args(["compose", "-p", project, "-f", compose_file, "ps", "-q", service])
        .output()
        .context("docker compose ps")?;
    let cid = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if cid.is_empty() {
        bail!("compose service '{service}' has no running container");
    }
    Ok(cid)
}

// --- routing layer --------------------------------------------------------

pub fn publish(app: &str) -> Result<()> {
    let st = load_state(app)?;
    caddy::publish(app, &st.host, &st.ip, st.port)
}

pub fn unpublish(app: &str) -> Result<()> {
    load_state(app)?; // errors "no deployed app '<app>'" if it doesn't exist
    caddy::unpublish(app)?;
    println!("charm: '{app}' removed from the proxy (container left running)");
    Ok(())
}

pub fn sync() -> Result<()> {
    ensure_state_access()?;
    let apps = list_states();
    if apps.is_empty() {
        println!("charm: no apps to sync");
        return Ok(());
    }
    for (app, st) in &apps {
        caddy::publish(app, &st.host, &st.ip, st.port)
            .with_context(|| format!("syncing '{app}'"))?;
        println!("charm: synced '{app}' -> https://{}", st.host);
    }
    Ok(())
}

// --- container layer (no rebuild) -----------------------------------------

pub fn stop(app: &str) -> Result<()> {
    let st = load_state(app)?;
    let _ = caddy::unpublish(app);
    match &st.runtime {
        Runtime::Dockerfile { .. } => run_quiet("docker", &["stop", &container(app)])?,
        Runtime::Compose { compose_file, .. } => {
            run_quiet("docker", &["compose", "-p", &project(app), "-f", compose_file, "stop"])?
        }
    }
    println!("charm: stopped '{app}' (route removed; image + repo kept)");
    Ok(())
}

pub fn start(app: &str) -> Result<()> {
    let st = load_state(app)?;
    match &st.runtime {
        Runtime::Dockerfile { image } => {
            if container_exists(app) {
                run_quiet("docker", &["start", &container(app)])?;
            } else {
                run_dockerfile(app, &st.ip, image)?;
            }
        }
        Runtime::Compose { compose_file, .. } => {
            run_quiet("docker", &["compose", "-p", &project(app), "-f", compose_file, "start"])?;
        }
    }
    caddy::publish(app, &st.host, &st.ip, st.port)?;
    println!("charm: started '{app}' -> https://{}", st.host);
    Ok(())
}

pub fn restart(app: &str) -> Result<()> {
    let st = load_state(app)?;
    match &st.runtime {
        Runtime::Dockerfile { image } => {
            if container_exists(app) {
                run_quiet("docker", &["restart", &container(app)])?;
            } else {
                run_dockerfile(app, &st.ip, image)?;
            }
        }
        Runtime::Compose { compose_file, .. } => {
            run_quiet("docker", &["compose", "-p", &project(app), "-f", compose_file, "restart"])?;
        }
    }
    caddy::publish(app, &st.host, &st.ip, st.port)?;
    println!("charm: restarted '{app}'");
    Ok(())
}

pub fn logs(app: &str) -> Result<()> {
    let st = load_state(app)?;
    // exec replaces this process so `-f` streams until the user interrupts.
    let err = match &st.runtime {
        Runtime::Dockerfile { .. } => Command::new("docker")
            .args(["logs", "-f", "--tail", "100", &container(app)])
            .exec(),
        Runtime::Compose { compose_file, .. } => Command::new("docker")
            .args(["compose", "-p", &project(app), "-f", compose_file, "logs", "-f", "--tail", "100"])
            .exec(),
    };
    Err(anyhow!("failed to exec docker logs: {err}"))
}

// --- full lifecycle -------------------------------------------------------

pub fn rm(app: &str, volumes: bool) -> Result<()> {
    let st = load_state(app)?; // errors "no deployed app '<app>'" if it doesn't exist
    let _ = caddy::unpublish(app);

    match &st.runtime {
        Runtime::Compose { compose_file, .. } => {
            let proj = project(app);
            let mut args = vec!["compose", "-p", proj.as_str(), "-f", compose_file.as_str(), "down"];
            if volumes {
                args.push("--volumes");
            }
            let _ = quiet("docker", &args);
        }
        Runtime::Dockerfile { .. } => {
            let rm_args: &[&str] = if volumes {
                &["rm", "-f", "-v"]
            } else {
                &["rm", "-f"]
            };
            let _ = Command::new("docker")
                .args(rm_args)
                .arg(container(app))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let _ = Command::new("sh")
                .arg("-c")
                .arg(format!(
                    "docker image ls -q '{}:*' | xargs -r docker image rm -f",
                    container(app)
                ))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }

    let _ = fs::remove_file(format!("{}/{app}.json", paths::state()));
    let _ = fs::remove_file(format!("{}/{app}.ip", paths::state()));
    let _ = fs::remove_dir_all(format!("{}/{app}", paths::builds()));
    println!("charm: removed '{app}'");
    Ok(())
}

pub fn list() -> Result<()> {
    ensure_state_access()?;
    let apps = list_states();
    if apps.is_empty() {
        println!("no apps deployed");
        return Ok(());
    }
    println!(
        "{}",
        style::bold(&format!("{:<18} {:<8} {:<28} UPSTREAM", "APP", "KIND", "URL"))
    );
    for (app, st) in &apps {
        let kind = match st.runtime {
            Runtime::Dockerfile { .. } => "docker",
            Runtime::Compose { .. } => "compose",
        };
        println!(
            "{app:<18} {kind:<8} {:<28} {}:{}",
            format!("https://{}", st.host),
            st.ip,
            st.port
        );
    }
    Ok(())
}

pub fn status(app: &str) -> Result<()> {
    let st = load_state(app)?;

    let (kind, image, cstate) = match &st.runtime {
        Runtime::Dockerfile { image } => {
            ("docker", Some(image.clone()), docker_status(&container(app)))
        }
        Runtime::Compose { compose_file, service } => {
            ("compose", None, compose_status(app, compose_file, service))
        }
    };
    let routed = caddy::is_published(app);

    println!("{}", style::bold(app));
    field("kind", kind);
    field("url", &format!("https://{}", st.host));
    field("upstream", &format!("{}:{}", st.ip, st.port));
    if let Some(image) = &image {
        field("image", image);
    }
    field("container", &color_state(&cstate));
    field(
        "route",
        &if routed {
            style::green("published")
        } else {
            style::red("missing")
        },
    );

    println!();
    println!("{}", verdict(app, &cstate, routed));
    Ok(())
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

fn verdict(app: &str, cstate: &str, routed: bool) -> String {
    if cstate == "missing" {
        style::red(&format!("container missing - run `charm start {app}` or redeploy"))
    } else if cstate != "running" {
        style::yellow(&format!("stopped - run `charm start {app}`"))
    } else if !routed {
        style::yellow("running but not routed - run `charm sync`")
    } else {
        style::green("healthy")
    }
}

/// Container state string ("running" / "exited" / … / "missing").
fn docker_status(name: &str) -> String {
    match Command::new("docker")
        .args(["inspect", "-f", "{{.State.Status}}", name])
        .output()
    {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() {
                "missing".into()
            } else {
                s
            }
        }
        _ => "missing".into(),
    }
}

fn compose_status(app: &str, compose_file: &str, service: &str) -> String {
    let cid = match Command::new("docker")
        .args(["compose", "-p", &project(app), "-f", compose_file, "ps", "-a", "-q", service])
        .output()
    {
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_string(),
        _ => String::new(),
    };
    if cid.is_empty() {
        "missing".into()
    } else {
        docker_status(&cid)
    }
}

// --- config / planning ----------------------------------------------------

/// Derive the deploy plan from `Charm.toml` + repo contents.
fn load_plan(build_dir: &str) -> Result<Plan> {
    let path = format!("{build_dir}/Charm.toml");
    let table: toml::Table = if Path::new(&path).exists() {
        toml::from_str(&fs::read_to_string(&path)?).context("parsing Charm.toml")?
    } else {
        toml::Table::new()
    };

    let routes = table
        .get("routes")
        .and_then(|v| v.as_table())
        .context("Charm.toml needs a [routes] table")?;
    let (host_key, value) = routes.iter().next().context("[routes] is empty")?;
    // Hosts are literal FQDNs (no expansion) and must be under a registered domain.
    let host = host_key.to_string();
    crate::domains::ensure_allowed(&host)?;

    // Compose if Charm.toml says so, or a compose file is present in the repo.
    let compose_file = table
        .get("compose")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| detect_compose(build_dir));

    match compose_file {
        Some(rel) => {
            // v0 compose: route value must be "service:port".
            let s = value
                .as_str()
                .context("compose apps need a \"service:port\" route value")?;
            let (service, port) = parse_service_port(s)?;
            let compose_file = format!("{build_dir}/{rel}");
            reject_container_name(&compose_file)?;
            Ok(Plan::Compose {
                host,
                compose_file,
                service,
                port,
            })
        }
        None => {
            let port = value
                .as_integer()
                .context("Dockerfile apps need an integer port route (e.g. \"app\" = 8080)")?
                as u16;
            Ok(Plan::Dockerfile { host, port })
        }
    }
}

fn detect_compose(dir: &str) -> Option<String> {
    ["docker-compose.yml", "docker-compose.yaml", "compose.yml", "compose.yaml"]
        .into_iter()
        .find(|name| Path::new(&format!("{dir}/{name}")).exists())
        .map(String::from)
}

/// `container_name:` bypasses Compose's per-project naming - it's a global
/// Docker name, so two apps that set the same one would collide. Refuse it;
/// Charm namespaces every container under the `charm_<app>` project.
fn reject_container_name(compose_file: &str) -> Result<()> {
    let text =
        fs::read_to_string(compose_file).with_context(|| format!("reading {compose_file}"))?;
    for (i, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("container_name:") {
            bail!(
                "compose: `container_name:` (line {}) is not allowed - it's a global name \
                 that can collide across apps. Remove it; Charm names containers per app.",
                i + 1
            );
        }
    }
    Ok(())
}

fn parse_service_port(s: &str) -> Result<(String, u16)> {
    let (svc, port) = s
        .split_once(':')
        .with_context(|| format!("expected \"service:port\", got '{s}'"))?;
    let port = port.parse().with_context(|| format!("invalid port in '{s}'"))?;
    Ok((svc.to_string(), port))
}

// --- state (the per-app manifest) -----------------------------------------

/// Assign (and persist) a stable IP on the charm subnet, reused across deploys.
fn assign_ip(app: &str) -> Result<String> {
    let file = format!("{}/{app}.ip", paths::state());
    if let Ok(existing) = fs::read_to_string(&file) {
        let existing = existing.trim();
        if !existing.is_empty() {
            return Ok(existing.to_string());
        }
    }
    let mut used = std::collections::HashSet::new();
    if let Ok(entries) = fs::read_dir(paths::state()) {
        for e in entries.flatten() {
            if e.path().extension().and_then(|x| x.to_str()) == Some("ip") {
                if let Ok(s) = fs::read_to_string(e.path()) {
                    used.insert(s.trim().to_string());
                }
            }
        }
    }
    for n in 2..=254 {
        let ip = format!("172.20.0.{n}");
        if !used.contains(&ip) {
            fs::write(&file, &ip)?;
            return Ok(ip);
        }
    }
    bail!("no free IPs left in 172.20.0.0/24");
}

fn save_state(app: &str, st: &AppState) -> Result<()> {
    fs::write(
        format!("{}/{app}.json", paths::state()),
        serde_json::to_string_pretty(st)?,
    )?;
    Ok(())
}

fn load_state(app: &str) -> Result<AppState> {
    use std::io::ErrorKind;
    let path = format!("{}/{app}.json", paths::state());
    let s = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == ErrorKind::NotFound => bail!("no deployed app '{app}'"),
        Err(e) if e.kind() == ErrorKind::PermissionDenied => {
            bail!("cannot read Charm state - run as root (sudo) or the charm user")
        }
        Err(e) => return Err(anyhow::Error::new(e).context("reading app state")),
    };
    serde_json::from_str(&s).context("parsing app state")
}

/// Management commands need access to /home/charm/state (root or the charm user);
/// otherwise reads silently come back empty, which reads as "nothing deployed".
fn ensure_state_access() -> Result<()> {
    use std::io::ErrorKind;
    match fs::read_dir(paths::state()) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => {
            bail!("Charm isn't installed (no {}). Run `charm install` first.", paths::state())
        }
        Err(e) if e.kind() == ErrorKind::PermissionDenied => {
            bail!(
                "cannot read Charm state at {} - run as root (sudo charm ...) or the charm user",
                paths::state()
            )
        }
        Err(e) => Err(anyhow::Error::new(e).context("reading Charm state")),
    }
}

fn list_states() -> Vec<(String, AppState)> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(paths::state()) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let app = p.file_stem().and_then(|x| x.to_str()).unwrap_or("").to_string();
            if let Ok(s) = fs::read_to_string(&p) {
                if let Ok(st) = serde_json::from_str::<AppState>(&s) {
                    out.push((app, st));
                }
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

// --- docker helpers -------------------------------------------------------

fn run_dockerfile(app: &str, ip: &str, image: &str) -> Result<()> {
    run(
        "docker",
        &[
            "run", "-d",
            "--name", &container(app),
            "--network", paths::NETWORK,
            "--ip", ip,
            "--restart", "unless-stopped",
            image,
        ],
    )
}

fn rm_container(app: &str) {
    let _ = Command::new("docker")
        .args(["rm", "-f", &container(app)])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn container_exists(app: &str) -> bool {
    Command::new("docker")
        .args(["inspect", &container(app)])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Run a command, streaming its output (we're in the post-receive hook, whose
/// stdout/stderr are relayed to the client as `remote:` lines - safe to print).
fn run(bin: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(bin)
        .args(args)
        .status()
        .with_context(|| format!("running `{bin}`"))?;
    if !status.success() {
        bail!("`{bin} {}` failed", args.join(" "));
    }
    Ok(())
}

/// Like `run`, but suppress stdout (e.g. the container id docker echoes back).
fn run_quiet(bin: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(bin)
        .args(args)
        .stdout(Stdio::null())
        .status()
        .with_context(|| format!("running `{bin}`"))?;
    if !status.success() {
        bail!("`{bin} {}` failed", args.join(" "));
    }
    Ok(())
}

/// Best-effort variant for teardown paths that should never get stuck.
fn quiet(bin: &str, args: &[&str]) -> std::io::Result<std::process::ExitStatus> {
    Command::new(bin)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
}
