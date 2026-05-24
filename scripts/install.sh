#!/usr/bin/env bash
set -euo pipefail

BUNNY_VERSION="${BUNNY_VERSION:-latest}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

echo "Installing bunny ${BUNNY_VERSION}..."

if ! command -v cargo >/dev/null 2>&1; then
  echo "Error: Rust toolchain required. Install from https://rustup.rs"
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$ROOT"
cargo build --release -p bunny-server
mkdir -p "$INSTALL_DIR"
cp target/release/bunny "$INSTALL_DIR/bunny"
chmod +x "$INSTALL_DIR/bunny"

echo "✓ Installed to $INSTALL_DIR/bunny"
echo "✓ Ensure $INSTALL_DIR is on your PATH"
echo "✓ Run: bunny configure"
echo "✓ Run: bunny doctor"
echo ""
echo "Dev checkout: ./bunny configure && ./bunny run --web-ui"
