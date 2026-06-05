#!/usr/bin/env bash
# Verify packages/i18n/messages.json has en/fr for every key.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
cargo test -p bunny-i18n --quiet
echo "✓ i18n catalog parity (en/fr)"
