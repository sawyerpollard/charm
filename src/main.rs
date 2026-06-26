mod app;
mod caddy;
mod doctor;
mod install;
mod paths;
mod shell;
mod style;
mod uninstall;
mod util;

use clap::{Parser, Subcommand};

/// Charm - git-push-to-deploy for a single VPS.
///
/// See `design-notes.md` for the architecture this skeleton will grow into.
#[derive(Parser)]
#[command(name = "charm", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Provision this host: create the `charm` user, SSH forced command, the
    /// `charm` Docker network, and record the install manifest.
    Install {
        /// Base domain apps are published under (e.g. `saw.dog`).
        #[arg(long)]
        domain: String,
        /// Public SSH key authorized to push (the key contents, not a path).
        #[arg(long)]
        ssh_key: String,
    },

    /// Remove Charm's control plane. By default leaves deployed apps running;
    /// `--all` also tears them down (`--volumes` to drop their data).
    Uninstall {
        #[arg(long)]
        all: bool,
        #[arg(long)]
        volumes: bool,
    },

    /// Internal: SSH forced-command entry point. Parses `$SSH_ORIGINAL_COMMAND`,
    /// lazily creates the bare repo + hook, then execs `git-receive-pack`.
    #[command(hide = true)]
    Shell,

    /// Internal: invoked by a repo's `post-receive` hook to build + run an app.
    #[command(hide = true)]
    Deploy {
        app: String,
        repo: String,
        gitref: String,
        sha: String,
    },

    // --- routing layer (Caddy) ---
    /// Assert an app's route in Caddy (idempotent, with conflict check).
    Publish {
        app: String,
    },

    /// Remove an app's route from Caddy; the container is left running.
    Unpublish {
        app: String,
    },

    /// Reconcile all routes from the manifest (recovery after a Caddy restart).
    Sync,

    // --- container layer (Docker) ---
    /// Start a deployed app from its last-built image (no rebuild) and route it.
    Start {
        app: String,
    },

    /// Stop an app's container and remove its route; keeps the repo + image.
    Stop {
        app: String,
    },

    /// Restart an app's container (stop + start).
    Restart {
        app: String,
    },

    // --- full lifecycle ---
    /// Remove a single app: container(s), route, image. `--volumes` drops data.
    #[command(visible_aliases = ["remove", "delete"])]
    Rm {
        app: String,
        #[arg(long)]
        volumes: bool,
    },

    // --- observability ---
    /// List deployed apps and their status.
    #[command(visible_aliases = ["apps", "ls", "ps"])]
    List,

    /// Show one app's detail: config + live container/route state.
    #[command(visible_aliases = ["info", "inspect"])]
    Status {
        app: String,
    },

    /// Stream an app's logs.
    #[command(visible_alias = "log")]
    Logs {
        app: String,
    },

    /// Check host prerequisites (git, systemd, Docker, Caddy admin API).
    Doctor,
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Install { domain, ssh_key } => install::run(&domain, &ssh_key)?,
        Command::Uninstall { all, volumes } => uninstall::run(all, volumes)?,
        Command::Shell => shell::run()?,
        Command::Deploy { app, repo, gitref, sha } => app::deploy(&app, &repo, &gitref, &sha)?,
        Command::Publish { app } => app::publish(&app)?,
        Command::Unpublish { app } => app::unpublish(&app)?,
        Command::Sync => app::sync()?,
        Command::Start { app } => app::start(&app)?,
        Command::Stop { app } => app::stop(&app)?,
        Command::Restart { app } => app::restart(&app)?,
        Command::Rm { app, volumes } => app::rm(&app, volumes)?,
        Command::List => app::list()?,
        Command::Status { app } => app::status(&app)?,
        Command::Logs { app } => app::logs(&app)?,
        Command::Doctor => doctor::run()?,
    }
    Ok(())
}
