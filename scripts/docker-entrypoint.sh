#!/usr/bin/env bash
# Production Docker entrypoint — bunny on PATH, install dir set.
set -euo pipefail

export BUNNY_INSTALL_DIR="${BUNNY_INSTALL_DIR:-/opt/bunny}"
export PATH="${BUNNY_INSTALL_DIR}/bin:/usr/local/bin:${PATH}"
export LANG=C.UTF-8
export LC_ALL=C.UTF-8

exec "$@"
