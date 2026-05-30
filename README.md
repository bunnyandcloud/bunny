# bunny

Remote development and debugging tool for Linux servers: PTY terminals, port previews, instrumented desktop browser, unified timeline, and secure local authentication..

## Quick start

```bash
git clone https://github.com/bunny-dev/bunny.git && cd bunny
./bunny setup
bunny configure
bunny run
```

On Debian/Ubuntu, `./bunny setup` installs prerequisites (Rust, Node, browser stack) automatically when needed.

Open the UI in your browser (see [where to connect](#where-to-open-the-ui) below). Verbose Rust build: `BUNNY_VERBOSE_BUILD=1 ./bunny setup`

## Prerequisites

| Tool | Version | Used for |
|------|---------|----------|
| **Rust** (rustup) | stable, ≥ 1.86 | Agent + CLI (`bunny-server`) |
| **Node.js** + **npm** | 20+ | Web UI (`apps/web`) + WebRTC/CDP sidecars |
| **Git** | any | Clone |
| **tmux** | any | Persistent terminals |
| **Neovim** (`nvim`) | any | Default editor on the remote host |

**Browser preview & streaming** (installed automatically by `./bunny setup` on Debian/Ubuntu):

| Tool | Used for |
|------|----------|
| **Chromium** | Remote desktop browser (via **Playwright** on Ubuntu 24.04+ Docker — apt packages are snap stubs) |
| **Xvfb** | Virtual display for headless Chromium |
| **x11vnc** | VNC server on the virtual display |
| **websockify** | noVNC WebSocket bridge (interactive browser tab) |
| **Sidecar npm deps** | `apps/server/webrtc-sidecar`, `apps/server/cdp-sidecar` (WebRTC stream + CDP capture) |

Port **Preview** (iframe proxy to e.g. `:3000`) works without the browser stack. The **Browser** tab (noVNC / WebRTC) needs the packages above — run `bunny doctor` to verify.

### Linux (Ubuntu / Debian / Docker)

Inside the container or VM:

```bash
apt-get update
apt-get install -y curl ca-certificates build-essential pkg-config libssl-dev git tmux neovim

# Browser stack (Preview tab works without this; Browser tab needs it)
apt-get install -y xvfb x11vnc websockify

# Chromium — use Playwright on Ubuntu 24.04+ (apt `chromium-browser` is a snap stub)
cd /path/to/bunny/apps/server/webrtc-sidecar
npm install
npx playwright install chromium
npx playwright install-deps chromium
ln -sf "$(find ~/.cache/ms-playwright -path '*/chromium-*/chrome-linux*/chrome' -type f | sort -V | tail -1)" /usr/local/bin/chromium

# CDP sidecar
cd ../cdp-sidecar && npm install

# Rust
curl -fsSL https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustc --version
cargo --version

# Node.js 20
curl -fsSL https://deb.nodesource.com/setup_20.x | bash -
apt-get install -y nodejs
node --version
npm --version
```

Or run the helper script manually:

```bash
./scripts/install-prerequisites.sh
source "$HOME/.cargo/env"
bunny doctor
```

### macOS

```bash
xcode-select --install   # if needed
curl -fsSL https://sh.rustup.rs | sh
# Node: https://nodejs.org/ or brew install node
# Neovim: brew install neovim
```

After installing Rust, **open a new shell** or run `source "$HOME/.cargo/env"` so `cargo` is on your `PATH`.

### Docker (fresh Ubuntu container)

On the **host**:

```bash
docker run -dit --name bunny-dev \
  -v "$(pwd)":/opt/bunny \
  -w /opt/bunny \
  -p 127.0.0.1:7681:7681 \
  --shm-size=2g \
  ubuntu:24.04 sleep infinity
```

Inside the container (or use the helper script from the host):

```bash
./scripts/docker-dev.sh up
./scripts/docker-dev.sh init
./scripts/docker-dev.sh shell
bunny run
```

`init` creates `~/.config/bunny/config.yaml` automatically. For **Discord**, see [docs/integrations/discord-docker-dev.md](docs/integrations/discord-docker-dev.md).

Legacy manual flow:

```bash
docker exec -it bunny-dev bash
cd /opt/bunny && ./bunny setup && bunny configure && bunny run
```

In Docker, the launcher and agent bind **`0.0.0.0:7681`** automatically so port publishing works. Confirm you see:

`✓ Listening on http://0.0.0.0:7681` — not `127.0.0.1` only inside the container.

### Where to open the UI

| Where you run `bunny run` | URL in your browser |
|-----------------------------------|---------------------|
| **Same machine** (Linux VM with desktop, or SSH with port forward) | `http://127.0.0.1:7681` |
| **Docker** on your laptop (`-p 127.0.0.1:7681:7681`) | `http://127.0.0.1:7681` on the laptop |
| **Remote VM** (SSH tunnel) | `ssh -L 7681:127.0.0.1:7681 user@vm` then `http://127.0.0.1:7681` on your laptop |
| **Remote VM** (public IP) | See [Server with a public IP](#server-with-a-public-ip) below |

For day-to-day work, prefer an **SSH tunnel** (first remote row). Binding on all interfaces is only for quick solo tests or when you explicitly need direct browser access.

### Server with a public IP

Use this when the agent runs on a VPS or cloud VM and you want to open the web UI from your laptop **without** `ssh -L`.

**1. Listen on all interfaces**

```bash
bunny run --host 0.0.0.0 --port 7681
```

Or set it once in config (`~/.config/bunny/config.yaml`, or `.bunny.yaml` in the repo — see [.bunny.yaml.example](.bunny.yaml.example)):

```yaml
server:
  bind_host: 0.0.0.0
  port: 7681
```

Confirm the log shows `Listening on http://0.0.0.0:7681`, not `127.0.0.1` only.

**2. Open the port on the host**

- **Linux firewall** (example): allow TCP `7681` from your IP only if possible (`ufw`, `firewalld`, or your cloud security group).
- **Cloud**: add an inbound rule for TCP **7681** on the instance security group / network ACL.

**3. Open in the browser**

```text
http://YOUR_PUBLIC_IP:7681
```

Replace `YOUR_PUBLIC_IP` with the VM’s public IPv4 (or DNS name).

**Security**

- Traffic is **HTTP by default** — login and session cookies are not encrypted on the wire. Do not expose `0.0.0.0` on the open internet for anything sensitive.
- **Prefer** `ssh -L 7681:127.0.0.1:7681 user@host` and keep the agent on `127.0.0.1` for production-style use (see [docs/install/README.md](docs/install/README.md)).
- Restrict firewall / security group source IPs to your home or office when you must bind publicly.
- For a durable public setup, put **HTTPS** (reverse proxy + TLS) in front of the agent instead of raw port 7681 on the internet.

Production-style deployments should keep **`127.0.0.1`** on the server and use an SSH tunnel unless you have deliberately hardened the network path.

### `setup` and permissions

`./bunny setup` picks the install location automatically:

| Situation | Where `bunny` is installed | `bunny` works right away? |
|-----------|----------------------------|---------------------------|
| **root**, or `/usr/local/bin` writable | `/usr/local/bin` | Yes |
| Normal user on a VM | `~/.local/bin` + line in `~/.bashrc` | After a **new SSH session** (or `source ~/.bashrc` once) |
| Normal user, system-wide CLI | Run **`sudo ./bunny setup`** | Yes (same as root row) |

You do **not** need `sudo` on Docker as root or on a dev VM where you are already root. Use `sudo` on a shared Linux VM if you want `/usr/local/bin` without logging out and back in.

Override: `INSTALL_DIR=/custom/bin ./bunny setup`

### Without global install

From the repo root you can skip `setup` and call the launcher directly:

```bash
./bunny configure
./bunny run
```

## Monorepo layout

| Path | Description |
|------|-------------|
| `apps/server` | Rust agent + CLI (`bunny`) |
| `apps/web` | React + Vite web UI |
| `apps/mobile` | Flutter iOS/Android app |
| `crates/*` | Shared Rust libraries |
| `packages/*` | OpenAPI + WebSocket protocol contracts |
| `infra/` | Docker, systemd, packaging scripts |
| `scripts/` | Install, doctor, release helpers |

## Documentation

- [Architecture](docs/architecture/overview.md)
- [Installation](docs/install/README.md)
- [Security](docs/security/README.md)
- [API](docs/api/README.md)

## License

MIT
