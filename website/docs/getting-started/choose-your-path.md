---
sidebar_position: 1
---

# Choose your install path

Pick how you want to run bunny. Every path ends the same way: **`bunny configure`** → **`bunny run`** → open the web UI in your browser.

| You are… | Recommended | Compile? |
|----------|-------------|----------|
| **Trying bunny, Mac/Windows, or prod with Docker** | [Install with Docker](./install-docker) | No (pre-built image) |
| **Linux VPS without Docker** | [Install on Linux](./install-linux) | No (release tarball) |
| **Developing bunny itself** | [Developer install](../other/install-dev) | Yes (first time) |

## Production on Linux: Docker or native?

Both work on a VPS. Same commands after install.

| | Docker (recommended) | Native (`curl \| sh`) |
|--|---------------------|----------------------|
| Simplicity | Pull image, compose up | Download tarball + runtime deps |
| Upgrades | `docker compose pull` | Re-run install script |
| systemd | Container restart policy | See [Install on Linux](./install-linux#systemd) |
| Disk | ~2–4 GB image | ~500 MB + runtime deps |

Mac and Windows have **no native install** (browser tab needs Xvfb on Linux). Use Docker.

## What every path shares

```bash
bunny configure   # owner account, MFA, optional Discord
bunny run         # start the agent
```

From your laptop, use an SSH tunnel (recommended):

```bash
ssh -L 7681:127.0.0.1:7681 user@your-server
```

Open **http://127.0.0.1:7681** in your browser.

See [Configure the server](./configure-server) and [First run](./first-run) for the full walkthrough.
