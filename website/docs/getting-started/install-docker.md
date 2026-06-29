---
sidebar_position: 2
---

# Install with Docker

Use the pre-built image for local trials, Mac/Windows, or production on a Linux VPS. No compilation required.

## Quick start

```bash
curl -fsSL https://raw.githubusercontent.com/bunnyandcloud/bunny/main/scripts/docker-quickstart.sh | sh
```

Or manually:

```bash
curl -fsSL -o docker-compose.yml \
  https://raw.githubusercontent.com/bunnyandcloud/bunny/main/docker-compose.yml
docker compose pull
docker compose up -d
docker compose exec -it bunny bunny configure
docker compose exec -it bunny bunny run --host 0.0.0.0 --port 7681
```

Open **http://127.0.0.1:7681** on the host (port is mapped to localhost).

Image: `ghcr.io/bunnyandcloud/bunny:latest` (~2–4 GB with browser stack). Published on each [GitHub release tag](https://github.com/bunnyandcloud/bunny/releases).

## Production on a VPS

Same image and commands. Recommended settings in `docker-compose.yml`:

- **Persistent config:** volume `bunny-config:/root/.config/bunny`
- **Restart:** `restart: unless-stopped`
- **Port:** `127.0.0.1:7681:7681` + SSH tunnel from your laptop
- **Shared memory:** `shm_size: 2g` (browser tab)

After `docker compose up -d`, SSH to the VPS and run `configure` + `run` as above.

## What is included

The image contains:

- `bunny` and `bunny-discord-bridge` binaries
- Pre-built web UI and Node sidecars
- Browser stack (Xvfb, Playwright Chromium, noVNC)

## Dev Docker (source mount)

If you are **developing bunny itself**, use the dev flow with a mounted git checkout:

```bash
./scripts/docker-dev.sh bootstrap
./scripts/docker-dev.sh shell
bunny run
```

See [Developer install](./install-dev#docker-dev) and [Discord + Docker on Mac](../team-chats/discord/docker-mac).

## Next steps

[First run](./first-run) — configure, tunnel, Discord.
