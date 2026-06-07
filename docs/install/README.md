# Installation

## Prerequisites

Install **before** `cargo build` or `bunny run`:

| Tool | Notes |
|------|--------|
| **Rust** | [rustup](https://rustup.rs) — then `source "$HOME/.cargo/env"` |
| **Node.js 20+** + **npm** | Web UI build + WebRTC/CDP sidecars |
| **build-essential**, **pkg-config**, **libssl-dev** | Linux compile deps |
| **tmux** | Persistent terminals (included in install script) |
| **Neovim** (`nvim`) | Default editor on the remote host (included in install script) |

**Browser tab** (optional; not needed for port Preview):

| Tool | Notes |
|------|--------|
| **Chromium** | Playwright bundle (`npx playwright install chromium`) — Ubuntu 24.04 apt is snap-only |
| **Xvfb**, **x11vnc**, **websockify**, **novnc** | Browser tab (noVNC) |
| **Chromium via Playwright** | Installed by `install-prerequisites.sh` (apt `chromium` is a snap stub on Ubuntu 24.04 Docker) |
| **Sidecar npm** | `apps/server/webrtc-sidecar`, `apps/server/cdp-sidecar` |

Ubuntu / Debian / Docker (as root) — full install:

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

Or run `./scripts/install-prerequisites.sh` manually for the full step-by-step install inside the script.

## From a git clone (recommended for development)

```bash
git clone https://github.com/bunny-dev/bunny.git && cd bunny
./bunny setup
bunny configure
./bunny run
```

On a fresh Debian/Ubuntu host, `./bunny setup` installs missing prerequisites (Rust, Node, browser stack) before building. Use `./bunny setup --minimal` to skip the browser stack.

The launcher `./bunny` at the repo root **is** the CLI. It runs a release binary when built, otherwise compiles via Cargo on first use. See [Setup and permissions](#setup-and-permissions) below for `setup` vs `sudo`.

**Optional — type `bunny` without `./` (one-time):**

```bash
./bunny setup
bunny configure
bunny run
```

### Setup and permissions

`setup` chooses the install directory from your permissions:

- **`/usr/local/bin`** when that directory is writable (root, Docker, or after `sudo ./bunny setup`) → `bunny` is available immediately.
- **`~/.local/bin`** otherwise → updates `~/.bashrc` / `~/.zshrc`; open a **new shell** or run `source ~/.bashrc` once.

On a multi-user VM as a normal user, prefer **`sudo ./bunny setup`** if you want the same instant behavior as root. `sudo` is not required when `/usr/local/bin` is already writable.

Open **http://127.0.0.1:7681/** (map the port in Docker; see below).

**Requirements:** Rust (rustup), Node.js 20+ and npm (for the first web UI build).

### Docker

```bash
docker run -dit --name bunny-dev \
  -v "$(pwd)":/opt/bunny \
  -w /opt/bunny \
  -p 127.0.0.1:7681:7681 \
  --shm-size=2g \
  ubuntu:24.04 sleep infinity

docker exec -it bunny-dev bash
cd /opt/bunny
./bunny setup
bunny configure
bunny doctor
bunny run
```

## Install script (from a clone)

On Debian/Ubuntu, prerequisites are installed automatically:

```bash
./scripts/install.sh
bunny configure
bunny run
```

Skip browser stack: `./scripts/install.sh --minimal`. Already have Rust/Node: `./scripts/install.sh --skip-prerequisites`.

Copies the binary to `~/.local/bin`. You still need the repo checkout (for `apps/web`) — first `bunny run` builds the web UI. Expect several minutes of compilation on a fresh server.

## Other commands

| Command | Purpose |
|---------|---------|
| `bunny start` | API only (UI if `apps/web/dist` exists) |
| `./bunny run --no-web-ui` | Agent without UI build |
| `bunny dev --cmd "…"` | Dev session + terminal + preview/browser |
| `bunny doctor` | Check dependencies |

## Secrets vault

```bash
./bunny secrets init
./bunny secrets set OPENAI_API_KEY --scope system
export BUNNY_SECRETS_PASSPHRASE='your-vault-passphrase'
```

See [security](../security/README.md).

## Discord (optional)

```bash
bunny discord setup
bunny discord bridge    # alongside bunny run
```

See [Discord integration](../integrations/discord.md). Docker on Mac: [discord-docker-dev.md](../integrations/discord-docker-dev.md).

## SSH tunnel mode

```bash
./bunny run --host 127.0.0.1 --port 7681
ssh -L 7681:127.0.0.1:7681 user@server
```

## Systemd

```bash
sudo cp infra/systemd/bunny-agent.service /etc/systemd/system/
sudo systemctl enable --now bunny-agent
```

## Requirements

- Linux (primary target) or macOS (dev)
- Rust 1.86+
- Node.js 20+ and npm (first `bunny run` build)
- **Browser tab:** Chromium, Xvfb, x11vnc, websockify + sidecar `npm install` (see `./scripts/install-prerequisites.sh`; use `--minimal` to skip)
- Verify with `bunny doctor`

## macOS (local development)

```bash
xcode-select --install   # if needed
curl -fsSL https://sh.rustup.rs | sh
# Node: https://nodejs.org/ or brew install node
# Neovim: brew install neovim
git clone https://github.com/bunny-dev/bunny.git && cd bunny
./bunny setup
bunny configure
bunny run
```

After installing Rust, open a new shell or run `source "$HOME/.cargo/env"`. For Docker-based dev on Mac, see [Discord + Docker (Mac dev)](../integrations/discord-docker-dev.md).
