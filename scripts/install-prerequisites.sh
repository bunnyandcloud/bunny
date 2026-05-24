#!/usr/bin/env bash
# Install build prerequisites on Debian/Ubuntu (Docker, VM). macOS: use rustup.rs and nodejs.org.
#
# Usage:
#   ./scripts/install-prerequisites.sh              # core + browser stack + sidecar npm
#   ./scripts/install-prerequisites.sh --minimal    # core only (no Chromium / noVNC / sidecars)
set -euo pipefail

INSTALL_BROWSER=1
if [[ "${1:-}" == "--minimal" ]]; then
  INSTALL_BROWSER=0
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

if [[ "$(id -u)" -eq 0 ]]; then
  SUDO=""
else
  SUDO="sudo"
fi

echo "→ Installing system packages (curl, build tools, SSL, tmux, neovim)…"
export DEBIAN_FRONTEND=noninteractive
$SUDO apt-get update -qq
$SUDO apt-get install -y curl ca-certificates build-essential pkg-config libssl-dev git tmux neovim

if [[ "$INSTALL_BROWSER" -eq 1 ]]; then
  echo "→ Installing browser stack (Xvfb, x11vnc, websockify, noVNC)…"
  $SUDO apt-get install -y xvfb x11vnc websockify novnc
  # Ubuntu 24.04+ ships `chromium-browser` as a snap stub (broken in Docker).
  # Real Chromium is installed below via Playwright after sidecar npm install.
fi

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

if [[ "$INSTALL_BROWSER" -eq 1 ]]; then
  for sidecar in webrtc-sidecar cdp-sidecar; do
    sidecar_dir="$REPO_ROOT/apps/server/$sidecar"
    if [[ -f "$sidecar_dir/package.json" ]]; then
      echo "→ npm install ($sidecar)…"
      (cd "$sidecar_dir" && npm install --no-fund --no-audit)
    fi
  done

  webrtc_dir="$REPO_ROOT/apps/server/webrtc-sidecar"
  if [[ -f "$webrtc_dir/package.json" ]]; then
    echo "→ Installing Chromium via Playwright (works in Docker; Ubuntu apt uses snap stubs)…"
    (cd "$webrtc_dir" && npx playwright install chromium)
    (cd "$webrtc_dir" && npx playwright install-deps chromium)
    playwright_chrome="$(find "${HOME}/.cache/ms-playwright" -path '*/chromium-*/chrome-linux*/chrome' -type f 2>/dev/null | sort -V | tail -1)"
    if [[ -n "$playwright_chrome" && -x "$playwright_chrome" ]]; then
      $SUDO ln -sf "$playwright_chrome" /usr/local/bin/chromium
      echo "✓ Linked /usr/local/bin/chromium → $playwright_chrome"
    else
      echo "⚠ Playwright Chromium binary not found under ~/.cache/ms-playwright" >&2
    fi
  fi
fi

echo ""
echo "✓ Prerequisites ready"
echo "  rustc:  $(rustc --version)"
echo "  cargo:  $(cargo --version)"
echo "  node:   $(node --version)"
echo "  npm:    $(npm --version)"
echo "  nvim:   $(nvim --version | head -1)"
if [[ "$INSTALL_BROWSER" -eq 1 ]]; then
  echo "  browser: chromium=$(command -v chromium || command -v chromium-browser || command -v google-chrome || echo missing)"
  echo "           Xvfb=$(command -v Xvfb || echo missing)"
  echo "           x11vnc=$(command -v x11vnc || echo missing)"
  echo "           websockify=$(command -v websockify || echo missing)"
fi
