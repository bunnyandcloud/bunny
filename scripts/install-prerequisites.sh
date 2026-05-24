#!/usr/bin/env bash
# Install build prerequisites on Debian/Ubuntu (Docker, VM). macOS: use rustup.rs and nodejs.org.
set -euo pipefail

if [[ "$(id -u)" -eq 0 ]]; then
  SUDO=""
else
  SUDO="sudo"
fi

echo "→ Installing system packages (curl, build tools, SSL)…"
export DEBIAN_FRONTEND=noninteractive
$SUDO apt-get update -qq
$SUDO apt-get install -y curl ca-certificates build-essential pkg-config libssl-dev git tmux

if ! command -v cargo >/dev/null 2>&1; then
  echo "→ Installing Rust (rustup)…"
  curl -fsSL https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
else
  echo "✓ Rust already installed: $(cargo --version)"
fi

# shellcheck disable=SC1091
source "${HOME}/.cargo/env"

if ! command -v node >/dev/null 2>&1 || ! command -v npm >/dev/null 2>&1; then
  echo "→ Installing Node.js 20.x (NodeSource)…"
  curl -fsSL https://deb.nodesource.com/setup_20.x | $SUDO bash -
  $SUDO apt-get install -y nodejs
else
  echo "✓ Node already installed: $(node --version), npm $(npm --version)"
fi

echo ""
echo "✓ Prerequisites ready"
echo "  rustc:  $(rustc --version)"
echo "  cargo:  $(cargo --version)"
echo "  node:   $(node --version)"
echo "  npm:    $(npm --version)"
echo ""
echo "Next (from the bunny repo):"
echo "  cd /opt/bunny   # or your clone path"
echo "  ./bunny setup"
echo "  bunny configure"
echo "  bunny run --web-ui"
