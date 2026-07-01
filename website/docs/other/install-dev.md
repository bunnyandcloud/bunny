---
sidebar_position: 4
---

# Developer install

For contributors working on bunny itself. Compiles from source on first run.

## From a git clone

```bash
git clone https://github.com/bunnyandcloud/bunny.git && cd bunny
./bunny setup
bunny configure
bunny run
```

On Debian/Ubuntu, `./bunny setup` installs Rust, Node, tmux, and the browser stack automatically.

Skip browser tab:

```bash
./bunny setup --minimal
```

**First install takes 5–10 minutes.** Subsequent starts are much faster.

### Setup and PATH

`./bunny setup` symlinks the launcher and builds the release binary:

- **`/usr/local/bin`** when writable (root, Docker, or `sudo ./bunny setup`) → `bunny` available immediately
- **`~/.local/bin`** otherwise → updates `~/.bashrc` / `~/.zshrc`; open a new shell after setup

On a multi-user VM, prefer **`sudo ./bunny setup`** for instant PATH behavior.

Alternative install script (from a clone, Debian/Ubuntu):

```bash
./scripts/install.sh
bunny configure
bunny run
```

Skip browser stack: `./scripts/install.sh --minimal`. Already have Rust/Node: `./scripts/install.sh --skip-prerequisites`.

## Prerequisites

Install **before** `cargo build` or `bunny run`:

| Tool | Notes |
|------|--------|
| **Rust 1.86+** | [rustup](https://rustup.rs) — then `source "$HOME/.cargo/env"` |
| **Node.js 20+** + **npm** | Web UI build + WebRTC/CDP sidecars |
| **build-essential**, **pkg-config**, **libssl-dev** | Linux compile deps |
| **tmux** | Persistent terminals |
| **Neovim** (`nvim`) | Default editor on the remote host |

**Browser tab** (optional; not needed for port preview):

| Tool | Notes |
|------|--------|
| **Chromium** | Playwright bundle — Ubuntu 24.04 apt is snap-only |
| **Xvfb**, **x11vnc**, **websockify**, **novnc** | Browser tab (noVNC) |
| **Sidecar npm** | `apps/server/webrtc-sidecar`, `apps/server/cdp-sidecar` |

On Debian/Ubuntu / Docker:

```bash
./scripts/install-prerequisites.sh
source "$HOME/.cargo/env"
bunny doctor
```

Minimal (no browser stack):

```bash
./scripts/install-prerequisites.sh --minimal
source "$HOME/.cargo/env"
```

Verify: `bunny doctor`

## Docker dev {#docker-dev}

Agent in Ubuntu container with source mounted:

```bash
./scripts/docker-dev.sh bootstrap
./scripts/docker-dev.sh shell
bunny run
```

Browser tab needs one-time `./scripts/docker-dev.sh browser-setup`.

Mac: Discord bridge runs on the host — see [Discord + Docker on Mac](../team-chats/discord/docker-mac).

Manual container:

```bash
docker compose -f docker-compose.dev.yml up -d
docker compose -f docker-compose.dev.yml exec bunny-dev bash
cd /opt/bunny && ./bunny setup && bunny configure && bunny run
```

## Other commands

| Command | Purpose |
|---------|---------|
| `bunny start` | API only |
| `./bunny run --no-web-ui` | Agent without UI |
| `bunny dev --cmd "…"` | Dev session + terminal |
| `bunny secrets init` | Encrypted secrets vault — see [Security](../security/) |

## Secrets vault

```bash
bunny secrets init
bunny secrets set OPENAI_API_KEY --scope system
export BUNNY_SECRETS_PASSPHRASE='your-vault-passphrase'
```

Full details: [Security](../security/).

## macOS (local)

```bash
xcode-select --install   # if needed
curl -fsSL https://sh.rustup.rs | sh
# Node from https://nodejs.org/ or brew install node
git clone https://github.com/bunnyandcloud/bunny.git && cd bunny
./bunny setup
bunny configure
bunny run
```

For full browser tab support on Mac, prefer [Docker dev](#docker-dev).

## Next steps

[First run](../getting-started/first-run)
