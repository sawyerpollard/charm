#!/bin/sh
# Install charm: download the static binary for this architecture into /usr/bin.
#
#   curl -fsSL https://raw.githubusercontent.com/sawyerpollard/charm/main/install.sh | sh
#
# Then provision the host:
#   sudo charm install --domain <your-domain> --ssh-key "$(cat ~/.ssh/id_ed25519.pub)"
set -e

REPO="sawyerpollard/charm"

case "$(uname -s)" in
  Linux) ;;
  *) echo "charm: only Linux is supported (got $(uname -s))" >&2; exit 1 ;;
esac

case "$(uname -m)" in
  x86_64 | amd64) ARCH=x86_64 ;;
  aarch64 | arm64) ARCH=aarch64 ;;
  *) echo "charm: unsupported architecture $(uname -m)" >&2; exit 1 ;;
esac

ASSET="charm-${ARCH}-unknown-linux-musl.tar.gz"
URL="https://github.com/${REPO}/releases/latest/download/${ASSET}"

echo "Downloading ${ASSET}…"
tmp="$(mktemp -d)"
curl -fsSL "$URL" | tar -xz -C "$tmp"

# /usr/bin (not /usr/local/bin) so `sudo charm` works on every distro.
sudo install -m 0755 "$tmp/charm" /usr/bin/charm
rm -rf "$tmp"

echo
echo "Installed: $(command -v charm)"
echo
echo "Next - provision this host (needs git, systemd, Docker, and Caddy):"
echo "  sudo charm install"
echo
echo "Register the domain(s) you'll serve apps under:"
echo "  sudo charm domain add example.com"
echo
echo "Authorize the key you'll push with - your LAPTOP's public key, pasted here:"
echo "  sudo charm key add \"ssh-ed25519 AAAA... you@laptop\""
echo
echo "Then, on your laptop, in an app repo:"
echo "  git remote add charm charm@<your-server>:my-app && git push charm main"
