#!/usr/bin/env bash
# Install bunny from GitHub Releases (Linux amd64/arm64). macOS/Windows → use Docker.
set -euo pipefail

GITHUB_REPO="${BUNNY_GITHUB_REPO:-bunnyandcloud/bunny}"
VERSION="${BUNNY_VERSION:-latest}"
INSTALL_PREFIX="${BUNNY_INSTALL_PREFIX:-}"
MINIMAL="${BUNNY_MINIMAL:-0}"

case "$(uname -s)" in
  Linux) ;;
  Darwin|MINGW*|MSYS*|CYGWIN*)
    cat <<'EOF'
Bunny native install is Linux-only (browser tab requires Xvfb on Linux).

On macOS or Windows, use the pre-built Docker image:

  curl -fsSL https://raw.githubusercontent.com/bunnyandcloud/bunny/main/scripts/docker-quickstart.sh | sh
  docker compose exec -it bunny bunny configure
  docker compose exec -it bunny bunny run --host 0.0.0.0 --port 7681

Docs: https://docs.bunnyandcloud.com/getting-started/install-docker
EOF
    exit 0
    ;;
  *)
    echo "Unsupported OS: $(uname -s)" >&2
    exit 1
    ;;
esac

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64)  TARGET_ARCH="linux-amd64" ;;
  aarch64|arm64) TARGET_ARCH="linux-arm64" ;;
  *)
    echo "Unsupported architecture: $ARCH" >&2
    exit 1
    ;;
esac

if [[ -z "$INSTALL_PREFIX" ]]; then
  if [[ "$(id -u)" -eq 0 ]]; then
    INSTALL_PREFIX="/opt/bunny"
    BIN_DIR="/usr/local/bin"
  else
    INSTALL_PREFIX="${HOME}/.local/share/bunny"
    BIN_DIR="${HOME}/.local/bin"
  fi
else
  BIN_DIR="${BUNNY_BIN_DIR:-${INSTALL_PREFIX}/bin}"
fi

if [[ "$VERSION" == "latest" ]]; then
  RELEASE_URL="https://github.com/${GITHUB_REPO}/releases/latest/download/bunny-${TARGET_ARCH}.tar.gz"
else
  RELEASE_URL="https://github.com/${GITHUB_REPO}/releases/download/${VERSION}/bunny-${VERSION}-${TARGET_ARCH}.tar.gz"
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "→ Downloading bunny (${TARGET_ARCH}, ${VERSION})…"
curl -fsSL "$RELEASE_URL" -o "$TMP/bunny.tar.gz"

echo "→ Installing to ${INSTALL_PREFIX}…"
mkdir -p "$INSTALL_PREFIX" "$BIN_DIR"
tar -xzf "$TMP/bunny.tar.gz" -C "$TMP"
EXTRACTED="$(find "$TMP" -mindepth 1 -maxdepth 1 -type d | head -1)"
if [[ -z "$EXTRACTED" || ! -d "$EXTRACTED/bin" ]]; then
  echo "Invalid release archive layout" >&2
  exit 1
fi
cp -a "${EXTRACTED}/." "$INSTALL_PREFIX/"

ln -sf "${INSTALL_PREFIX}/bin/bunny" "${BIN_DIR}/bunny"
ln -sf "${INSTALL_PREFIX}/bin/bunny-discord-bridge" "${BIN_DIR}/bunny-discord-bridge" 2>/dev/null || true

export BUNNY_INSTALL_DIR="$INSTALL_PREFIX"
RUNTIME_ARGS=()
if [[ "$MINIMAL" == "1" ]]; then
  RUNTIME_ARGS+=(--minimal)
fi
bash "${INSTALL_PREFIX}/scripts/install-runtime.sh" "${RUNTIME_ARGS[@]}"

if [[ ":$PATH:" != *":${BIN_DIR}:"* ]]; then
  echo ""
  echo "Add to PATH: export PATH=\"${BIN_DIR}:\$PATH\""
fi

echo ""
echo "✓ bunny installed"
echo "  Next: bunny configure && bunny run"
echo "  Docs: https://docs.bunnyandcloud.com/getting-started/first-run"
