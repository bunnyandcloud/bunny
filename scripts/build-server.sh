#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release -p bunny-server
echo "Built: target/release/bunny"
