#!/usr/bin/env bash
# Install build prerequisites on Debian/Ubuntu (Docker, VM). macOS: use rustup.rs and nodejs.org.
#
# Usage:
#   ./scripts/install-prerequisites.sh              # core + browser stack + sidecar npm
#   ./scripts/install-prerequisites.sh --minimal    # core only (no Chromium / noVNC / sidecars)
set -euo pipefail

ARGS=()
for arg in "$@"; do
  ARGS+=("$arg")
done
if [[ ${#ARGS[@]} -eq 0 ]]; then
  ARGS=()
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [[ "$(id -u)" -eq 0 ]]; then
  SUDO=""
else
  SUDO="sudo"
fi

echo "→ Installing compile dependencies (build-essential, SSL)…"
export DEBIAN_FRONTEND=noninteractive
$SUDO apt-get update -qq
$SUDO apt-get install -y build-essential pkg-config libssl-dev

bash "$SCRIPT_DIR/install-runtime.sh" "$@"

if ! command -v cargo >/dev/null 2>&1; then
  echo "→ Installing Rust (rustup)…"
  curl -fsSL https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
else
  echo "✓ Rust already installed: $(cargo --version)"
fi

# shellcheck disable=SC1091
source "${HOME}/.cargo/env"

echo ""
echo "✓ Prerequisites ready"
echo "  rustc:  $(rustc --version)"
echo "  cargo:  $(cargo --version)"
echo "  node:   $(node --version)"
echo "  npm:    $(npm --version)"
