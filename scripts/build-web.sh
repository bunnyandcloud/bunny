#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../apps/web"
# ci reinstalls node_modules for this platform (rollup native binary, etc.).
npm ci --no-fund --no-audit
npm run build
echo "Web assets: apps/web/dist"
