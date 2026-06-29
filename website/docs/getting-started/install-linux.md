---
sidebar_position: 3
---

# Install on Linux

Native install for a VPS or cloud VM without Docker. No compilation required.

## Quick start

```bash
curl -fsSL https://raw.githubusercontent.com/bunnyandcloud/bunny/main/scripts/install-release.sh | sh
bunny configure
bunny run
```

Pin a version:

```bash
BUNNY_VERSION=v0.1.0 curl -fsSL .../install-release.sh | sh
```

Supported: **linux-amd64** and **linux-arm64**. Binaries are published on each [GitHub release tag](https://github.com/bunnyandcloud/bunny/releases).

The script downloads a release tarball from GitHub Releases, installs runtime dependencies, and puts `bunny` on your PATH.

## Prerequisites

The install script runs `install-runtime.sh` which installs:

- tmux, Node.js 20+, Neovim
- Browser stack (Xvfb, x11vnc, websockify, noVNC, Playwright Chromium) unless `--minimal`

Skip browser tab:

```bash
BUNNY_MINIMAL=1 curl -fsSL .../install-release.sh | sh
```

## systemd

```bash
sudo cp infra/systemd/bunny-agent.service /etc/systemd/system/
sudo systemctl enable --now bunny-agent
```

The unit expects `bunny` at `/usr/local/bin/bunny`.

## SSH tunnel mode

```bash
bunny run --host 127.0.0.1 --port 7681
ssh -L 7681:127.0.0.1:7681 user@server
```

## Expose on public IP

Less secure — use only with firewall rules and MFA:

```bash
bunny run --host 0.0.0.0 --port 7681
```

## Alternative: Docker on Linux

You can also run the [pre-built Docker image](./install-docker) on a Linux VPS — same result, often simpler upgrades.

## Developer install from source

See [Developer install](./install-dev) if you need a git checkout.

## Next steps

[First run](./first-run)
