#!/usr/bin/env bash
# Package release tarball from a local release build.
# Usage: ./scripts/package-release.sh [version] [linux-amd64|linux-arm64]
set -euo pipefail

VERSION="${1:?version required, e.g. v0.1.0}"
ARCH="${2:-linux-amd64}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
STAGING="$(mktemp -d)"
trap 'rm -rf "$STAGING"' EXIT

PKG="${STAGING}/bunny-${VERSION}-${ARCH}"
mkdir -p "${PKG}/bin" "${PKG}/share/bunny" "${PKG}/scripts"

cp "${ROOT}/target/release/bunny" "${PKG}/bin/"
cp "${ROOT}/target/release/bunny-discord-bridge" "${PKG}/bin/"
cp "${ROOT}/scripts/install-runtime.sh" "${PKG}/scripts/"
cp -R "${ROOT}/apps/web/dist" "${PKG}/share/bunny/web/"
cp -R "${ROOT}/apps/server/webrtc-sidecar" "${PKG}/share/bunny/"
cp -R "${ROOT}/apps/server/cdp-sidecar" "${PKG}/share/bunny/"

# Production node_modules for sidecars
for sidecar in webrtc-sidecar cdp-sidecar; do
  (cd "${ROOT}/apps/server/${sidecar}" && npm ci --omit=dev --no-fund --no-audit)
  rm -rf "${PKG}/share/bunny/${sidecar}/node_modules"
  cp -R "${ROOT}/apps/server/${sidecar}/node_modules" "${PKG}/share/bunny/${sidecar}/"
done

OUT="${ROOT}/dist/bunny-${VERSION}-${ARCH}.tar.gz"
mkdir -p "${ROOT}/dist"
tar -czf "$OUT" -C "${STAGING}" "bunny-${VERSION}-${ARCH}"
echo "✓ ${OUT}"
