#!/usr/bin/env bash
set -euo pipefail
exec "$(dirname "$0")/../target/release/bunny" doctor 2>/dev/null || {
  echo "Build bunny first: cargo build --release -p bunny-server"
  exit 1
}
