<p align="center">
  <img src="docs/assets/logo.png" alt="bunny" width="280">
</p>

# Build products together on the same remote environment

This project turns a remote server or hosted container into a collaborative product-making station: web terminals, live port previews, live-streamed browsers, Discord workflows, and a unified timeline where code, feedback, decisions, and experiments come together. A real server-side agent gives teams an open workspace they control, without being locked into a proprietary cloud. It lets teams install and orchestrate the tools they already use — shell commands, project CLIs, scripts, and AI agents like Codex or Claude — through prompts, commands, and shared workflows in one open environment.

It is designed for engineers, designers, operators, founders, and non-technical contributors to participate in the same development flow — and for versioning to evolve beyond code commits into a more organic record of how the product is actually made.

## Install on a Linux server

Best for a VPS, cloud VM, or any machine where you develop over SSH.

**From a git clone** (recommended for development):

```bash
git clone https://github.com/bunny-dev/bunny.git && cd bunny
./bunny setup
bunny configure
bunny run
```

On Debian/Ubuntu, `./bunny setup` installs missing prerequisites (Rust, Node, tmux, browser stack) automatically.

**First install takes a few minutes** — `./bunny setup` compiles the Rust agent from source; the first `bunny run` also builds the web UI (Node.js required). Subsequent starts are much faster.

By default, the agent listens on **localhost only** (`127.0.0.1` on the server). From your laptop, use an **SSH tunnel** (recommended):

```bash
ssh -L 7681:127.0.0.1:7681 user@your-server
```

Then open **http://127.0.0.1:7681** in your browser — on your laptop, not the server’s public IP.

To reach the UI via the server’s IP directly, bind on all interfaces and open the firewall (less secure — see [Installation](docs/install/README.md)):

```bash
bunny run --host 0.0.0.0 --port 7681
# then http://YOUR_SERVER_IP:7681
```

Run `bunny doctor` to verify dependencies.

## Install with Docker

Run the agent inside a container — useful for trying bunny locally or keeping a clean dev environment.

**Quick container** (Ubuntu 24.04):

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
bunny run
```

Same as above: first `./bunny setup` and `bunny run` compile from source — allow a few minutes.

Open **http://127.0.0.1:7681** on the host.

**Mac / local dev** — use the helper script (agent in Docker, optional Discord bridge on the host):

```bash
./scripts/docker-dev.sh bootstrap
./scripts/docker-dev.sh shell
bunny run
```

Browser tab (noVNC) needs a one-time `./scripts/docker-dev.sh browser-setup`. Details: [Installation → Docker](docs/install/README.md#docker) and [Discord + Docker on Mac](docs/integrations/discord-docker-dev.md).

## Community

Discord server for questions, feedback, and release announcements — **invite link coming soon**.

## Documentation

| Topic | Link |
|-------|------|
| Full install guide (systemd, secrets) | [docs/install/README.md](docs/install/README.md) |
| macOS local dev | [docs/install/README.md#macos-local-development](docs/install/README.md#macos-local-development) |
| Architecture | [docs/architecture/overview.md](docs/architecture/overview.md) |
| Security | [docs/security/README.md](docs/security/README.md) |
| API | [docs/api/README.md](docs/api/README.md) |
| Discord integration | [docs/integrations/discord.md](docs/integrations/discord.md) |
| Mobile app | [docs/mobile/README.md](docs/mobile/README.md) |
| Everything else | [docs/README.md](docs/README.md) |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT
