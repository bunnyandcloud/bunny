#!/usr/bin/env bash
# Deprecated wrapper — use: bunny discord bridge
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
exec ./bunny discord bridge "$@"
