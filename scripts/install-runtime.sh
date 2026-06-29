#!/usr/bin/env bash
# Runtime dependencies for bunny (no Rust/cargo). Used by release install and Docker image.
#
# Usage:
#   ./scripts/install-runtime.sh              # full browser stack
#   ./scripts/install-runtime.sh --minimal    # no Chromium / noVNC
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

echo "→ Installing runtime packages (curl, tmux, neovim, git)…"
export DEBIAN_FRONTEND=noninteractive
$SUDO apt-get update -qq
$SUDO apt-get install -y curl ca-certificates git tmux neovim

if [[ "$INSTALL_BROWSER" -eq 1 ]]; then
  echo "→ Installing browser stack (Xvfb, x11vnc, websockify, noVNC)…"
  $SUDO apt-get install -y xvfb x11vnc websockify novnc
fi

if ! command -v node >/dev/null 2>&1 || ! command -v npm >/dev/null 2>&1; then
  echo "→ Installing Node.js 20.x (NodeSource)…"
  curl -fsSL https://deb.nodesource.com/setup_20.x | $SUDO bash -
  $SUDO apt-get install -y nodejs
else
  echo "✓ Node already installed: $(node --version), npm $(npm --version)"
fi

sidecar_roots=()
if [[ -d "$REPO_ROOT/apps/server/webrtc-sidecar" ]]; then
  sidecar_roots+=("$REPO_ROOT/apps/server")
elif [[ -n "${BUNNY_INSTALL_DIR:-}" && -d "${BUNNY_INSTALL_DIR}/share/bunny/webrtc-sidecar" ]]; then
  sidecar_roots+=("${BUNNY_INSTALL_DIR}/share/bunny")
elif [[ -d "/opt/bunny/share/bunny/webrtc-sidecar" ]]; then
  sidecar_roots+=("/opt/bunny/share/bunny")
fi

if [[ "$INSTALL_BROWSER" -eq 1 && ${#sidecar_roots[@]} -gt 0 ]]; then
  for root in "${sidecar_roots[@]}"; do
    for sidecar in webrtc-sidecar cdp-sidecar; do
      sidecar_dir="$root/$sidecar"
      if [[ -f "$sidecar_dir/package.json" && ! -d "$sidecar_dir/node_modules" ]]; then
        echo "→ npm install ($sidecar)…"
        (cd "$sidecar_dir" && npm install --omit=dev --no-fund --no-audit)
      fi
    done
  done

  webrtc_dir=""
  for root in "${sidecar_roots[@]}"; do
    if [[ -f "$root/webrtc-sidecar/package.json" ]]; then
      webrtc_dir="$root/webrtc-sidecar"
      break
    fi
  done

  if [[ -n "$webrtc_dir" ]]; then
    echo "→ Installing Chromium via Playwright…"
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
echo "✓ Runtime dependencies ready"
echo "  node:   $(node --version)"
echo "  npm:    $(npm --version)"
echo "  nvim:   $(nvim --version | head -1)"
if [[ "$INSTALL_BROWSER" -eq 1 ]]; then
  echo "  browser: chromium=$(command -v chromium || echo missing)"
fi
