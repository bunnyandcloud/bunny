#!/usr/bin/env bash
# Build bunny from source and install the CLI binary on PATH.
#
# On Debian/Ubuntu, system prerequisites are installed automatically unless skipped.
#
# Usage:
#   ./scripts/install.sh                         # prerequisites + build
#   ./scripts/install.sh --minimal               # core prerequisites only (no browser stack)
#   ./scripts/install.sh --skip-prerequisites    # build only (Rust must already be installed)
set -euo pipefail

SKIP_PREREQS=0
PREREQ_ARGS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-prerequisites | --skip-prereqs)
      SKIP_PREREQS=1
      shift
      ;;
    --minimal)
      PREREQ_ARGS+=(--minimal)
      shift
      ;;
    -h | --help)
      sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "Unknown option: $1 (try --help)" >&2
      exit 1
      ;;
  esac
done

BUNNY_VERSION="${BUNNY_VERSION:-latest}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "Installing bunny ${BUNNY_VERSION}..."

if [[ "$SKIP_PREREQS" -eq 0 ]] && command -v apt-get >/dev/null 2>&1; then
  echo "→ Installing prerequisites (Debian/Ubuntu)…"
  "$SCRIPT_DIR/install-prerequisites.sh" "${PREREQ_ARGS[@]}"
fi

source_cargo_env() {
  if [[ -f "${HOME}/.cargo/env" ]]; then
    # shellcheck source=/dev/null
    source "${HOME}/.cargo/env"
  fi
}

source_cargo_env

if ! command -v cargo >/dev/null 2>&1; then
  echo "Error: Rust toolchain required." >&2
  if command -v apt-get >/dev/null 2>&1; then
    echo "  Run without --skip-prerequisites, or: ./scripts/install-prerequisites.sh" >&2
  else
    echo "  Install from https://rustup.rs (macOS/other: ./scripts/install-prerequisites.sh is Debian/Ubuntu only)" >&2
  fi
  exit 1
fi

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
echo "Dev checkout: ./bunny configure && ./bunny run"
