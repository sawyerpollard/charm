# charm

Charm is a git-push-to-deploy tool for a single Linux VPS. You push a repo, it builds a container and serves it over HTTPS.

## Requirements

Your server needs these installed before Charm:

- `git`
- systemd
- Docker
- Caddy (with the admin API enabled on `localhost:2019`)

Run `charm doctor` after install to verify all four are reachable.

## Install

On the server, as root:

```sh
curl -fsSL https://raw.githubusercontent.com/sawyerpollard/charm/main/install.sh | sh
sudo charm install
sudo charm domain add yourdomain.com
sudo charm key add "ssh-ed25519 AAAA... you@laptop"
```

`charm install` creates a `charm` user, a Docker bridge network, and sets up SSH access. It does not install system packages or modify `sshd_config`.

You can add more domains and keys at any time.

## Deploy an app

In your app's repo on your laptop:

```sh
git remote add charm charm@your-server:my-app
git push charm main
```

That's a deploy. Every subsequent push to `main` (or `master`) redeploys.

The app name comes from the remote path (`my-app` above). The URL comes from `Charm.toml`.

## Charm.toml

Every app needs a `Charm.toml` at the repo root. It declares where to serve the app.

**Dockerfile app:**

```toml
[routes]
"blog.yourdomain.com" = 8080
```

The value is the port your container listens on.

**Compose app:**

```toml
[routes]
"blog.yourdomain.com" = "web:8080"
```

The value is `service:port`. Charm detects Compose automatically if a `docker-compose.yml` (or `compose.yml`) is present. You can also name the file explicitly:

```toml
compose = "docker/compose.yml"

[routes]
"blog.yourdomain.com" = "web:8080"
```

The host in `[routes]` must be under a domain you've registered with `charm domain add`. Charm checks this before deploying.

## Everyday commands

```sh
# see all deployed apps
charm list

# one app's config and live state
charm status my-app

# stream logs
charm logs my-app

# stop and start without rebuilding
charm stop my-app
charm start my-app
charm restart my-app

# remove an app (keeps volumes by default)
charm rm my-app
charm rm my-app --volumes   # also drops volumes
```

All of these run on the server (or over SSH). The TUI shows the same information interactively:

```sh
ssh charm@your-server   # opens the management console
```

## Routing

Caddy routes are managed automatically on deploy. If you need to manage them manually:

```sh
charm publish my-app     # add route to Caddy (app must already be deployed)
charm unpublish my-app   # remove route; container keeps running
charm sync               # re-apply all routes (use after a Caddy restart)
```

`charm sync` is the recovery command. Caddy's routes are in-memory; a Caddy restart drops them. Run `charm sync` to restore everything.

## Domains

```sh
sudo charm domain add yourdomain.com
sudo charm domain list
sudo charm domain remove olddomain.com
```

Apps can only serve hosts under registered domains. Adding a domain doesn't change DNS — you handle that separately (point `*.yourdomain.com` at your server's IP).

## SSH keys

```sh
sudo charm key add "ssh-ed25519 AAAA..."   # paste the key directly
sudo charm key list                         # see numbered list
sudo charm key remove 2                     # remove by number
```

Keys authorize git pushes only. SSH sessions (`ssh charm@host`) open the TUI; there is no shell access.

## Uninstall

```sh
# remove Charm's control plane, leave apps running
sudo charm uninstall

# also tear down all apps
sudo charm uninstall --all

# tear down apps and drop their volumes
sudo charm uninstall --all --volumes
```

`charm uninstall` lists any running apps and stops before doing anything if `--all` is not passed.
