#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../apps/web"
npm ci
npm run build
echo "Web assets: apps/web/dist"
