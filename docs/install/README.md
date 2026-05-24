# Installation

## Prerequisites

Install **before** `cargo build` or `bunny run --web-ui`:

| Tool | Notes |
|------|--------|
| **Rust** | [rustup](https://rustup.rs) — then `source "$HOME/.cargo/env"` |
| **Node.js 20+** + **npm** | Web UI build |
| **build-essential**, **pkg-config**, **libssl-dev** | Linux compile deps |

Ubuntu / Debian / Docker (as root):

```bash
./scripts/install-prerequisites.sh
source "$HOME/.cargo/env"
```

See [README prerequisites](../README.md#prerequisites) for step-by-step commands.

## From a git clone (recommended for development)

```bash
git clone https://github.com/bunny-dev/bunny.git && cd bunny
./bunny setup
bunny configure
./bunny run --web-ui
```

The launcher `./bunny` at the repo root **is** the CLI. It runs a release binary when built, otherwise compiles via Cargo on first use. See [README](../README.md#setup-and-permissions) for `setup` vs `sudo`.

**Optional — type `bunny` without `./` (one-time):**

```bash
./bunny setup
bunny configure
bunny run --web-ui
```

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
./scripts/install-prerequisites.sh
source "$HOME/.cargo/env"
./bunny setup
bunny configure
bunny run --web-ui
```

## System install (release binary on PATH)

```bash
curl -fsSL https://get.bunny.dev | sh   # or:
./scripts/install.sh
bunny configure
bunny run --web-ui
```

Copies the binary to `~/.local/bin`. You still need the repo (or a release bundle with `apps/web`) to build the UI.

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

## SSH tunnel mode

```bash
./bunny run --web-ui --host 127.0.0.1 --port 7681
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
- Node.js 20+ and npm (first `bunny run --web-ui` build)
- Optional: Chromium, Xvfb, x11vnc, websockify (for `--browser`)
