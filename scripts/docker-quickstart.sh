#!/usr/bin/env bash
# Download docker-compose.yml and start the pre-built bunny image.
set -euo pipefail

GITHUB_REPO="${BUNNY_GITHUB_REPO:-bunnyandcloud/bunny}"
BRANCH="${BUNNY_BRANCH:-main}"
COMPOSE_DIR="${BUNNY_COMPOSE_DIR:-./bunny-docker}"

mkdir -p "$COMPOSE_DIR"
cd "$COMPOSE_DIR"

base="https://raw.githubusercontent.com/${GITHUB_REPO}/${BRANCH}"

echo "→ Downloading docker-compose.yml…"
curl -fsSL "${base}/docker-compose.yml" -o docker-compose.yml

if ! command -v docker >/dev/null 2>&1; then
  echo "Docker is required. Install Docker Desktop or Docker Engine first." >&2
  echo "Docs: https://docs.bunnyandcloud.com/getting-started/install-docker" >&2
  exit 1
fi

echo "→ Pulling image (may take a few minutes, ~2–4 GB)…"
docker compose pull

echo "→ Starting bunny…"
docker compose up -d

cat <<EOF

✓ Bunny is running in Docker (${COMPOSE_DIR})

Next steps:

  cd ${COMPOSE_DIR}
  docker compose exec -it bunny bunny configure
  docker compose exec -it bunny bunny run --host 0.0.0.0 --port 7681

Then open http://127.0.0.1:7681 on this machine.

Docs: https://docs.bunnyandcloud.com/getting-started/first-run
EOF
