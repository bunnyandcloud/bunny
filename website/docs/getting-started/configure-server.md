---
sidebar_position: 3
---

# Configure the server

After [installing](./choose-your-path) bunny, configure the agent once per machine or container. This page covers `bunny configure`, config files, network access, and production setup.

## Overview

```text
Install → bunny configure → bunny run → connect (SSH tunnel or public URL)
              │
              ├─ Owner account + MFA
              ├─ ~/.config/bunny/config.yaml
              ├─ SQLite auth DB (~/.config/bunny/)
              └─ Optional: Discord (bunny discord setup)
```

## `bunny configure` (first run)

Run once on a fresh install:

```bash
bunny configure
```

Inside Docker:

```bash
docker compose exec -it bunny bunny configure
```

The wizard:

1. **Owner account** — email + password (Argon2id). Only one owner at bootstrap; more users via invitations in the Web UI.
2. **MFA (recommended)** — TOTP (Google Authenticator, etc.). Recovery codes are shown once — save them.
3. **Public URL** — how browsers reach this agent (watch links, OAuth, team chat deep links). Prompted for remote/production deployments.
4. **Discord (optional)** — if you accept, runs the same flow as `bunny discord setup` (Application ID, bot token, OAuth). See [Discord setup](../team-chats/discord/setup#discord-application-and-server).

Re-run `bunny configure` later to change Discord settings or re-bootstrap Discord when tokens rotate.

Non-interactive flags:

```bash
bunny configure --email you@example.com --password '…' --public-url https://your-host.example.com
```

## Config files

| File | Purpose |
|------|---------|
| `~/.config/bunny/config.yaml` | Main agent config (server, security, team chats, agents, Discord, terminals) |
| `~/.config/bunny/*.db` | Auth, sessions, audit (SQLite) |
| `~/.config/bunny/secrets.enc` | Encrypted secrets vault (optional) |
| `.discord/bridge.yaml` | Discord bridge bot token + guild (dev; or path from `bunny discord setup`) |
| `.bunny.yaml` | Optional override in repo cwd (dev) |

Create a default `config.yaml` without the full wizard:

```bash
bunny config-init
```

### Key settings (`config.yaml`)

```yaml
server:
  bind_host: "127.0.0.1"   # use 0.0.0.0 in Docker
  port: 7681
  data_dir: "~/.config/bunny"
  public_url: "https://your-host.example.com"   # external URL (proxy, HTTPS); distinct from bind_host

terminal:
  shell: "/bin/bash"
  backend: "tmux"          # persistent shells across restarts

browser:
  enabled: true

team_chats:
  link_code_ttl_minutes: 15   # session ↔ channel link codes (all team chat connectors)

agents:
  max_turns: 30               # max turns per agent invocation (Claude today; future agents)

discord:
  enabled: true
```

`team_chats` and `agents` apply to all team chat connectors (Discord today; Slack/Teams planned). Discord-specific credentials (`bridge_token_hash`, OAuth) stay under `discord:`.

`server.public_url` is the URL **browsers** use to reach the agent (watch links, OAuth callbacks, future team chat deep links). It is often different from `bind_host`/`port` when behind a reverse proxy or HTTPS terminator.

### Environment variables

Any YAML key can be overridden with `BUNNY_` + nested keys in `SCREAMING_SNAKE`:

| Variable | Effect |
|----------|--------|
| `BUNNY_SERVER__BIND_HOST=0.0.0.0` | Listen on all interfaces (Docker) |
| `BUNNY_SERVER__PORT=7681` | HTTP port |
| `BUNNY_SERVER__PUBLIC_URL=https://…` | Agent public URL (set during `bunny configure`) |
| `BUNNY_TEAM_CHATS__LINK_CODE_TTL_MINUTES=15` | TTL for session ↔ channel link codes |
| `BUNNY_AGENTS__MAX_TURNS=30` | Max agent turns per invocation |
| `BUNNY_SECRETS_PASSPHRASE=…` | Unlock secrets vault on start |

Double underscore `__` = nested YAML key.

## Start the agent

```bash
bunny run
```

| Flag | Purpose |
|------|---------|
| `--host 0.0.0.0` | Required inside Docker for port mapping |
| `--port 7681` | HTTP port (default 7681) |
| `--no-web-ui` | API only |
| `--no-discord-bridge` | Do not auto-start Discord bridge |

`bunny start` is similar but does not build the Web UI — use when `dist` is already present.

## Network access

### Recommended: SSH tunnel

Agent binds to **localhost on the server**:

```bash
bunny run --host 127.0.0.1 --port 7681
```

From your laptop:

```bash
ssh -L 7681:127.0.0.1:7681 user@your-server
```

Open **http://127.0.0.1:7681** locally.

### Public IP

```bash
bunny run --host 0.0.0.0 --port 7681
```

Open firewall port 7681, use MFA, prefer HTTPS reverse proxy in production.

## Production: systemd

After [native Linux install](./install-linux):

```bash
sudo cp infra/systemd/bunny-agent.service /etc/systemd/system/
sudo systemctl enable --now bunny-agent
```

Expects `bunny` at `/usr/local/bin/bunny`. Data and config stay in the service user's `~/.config/bunny/`.

## Verify

```bash
bunny doctor    # Chromium, Node, sidecars, tmux, web UI
bunny status    # Running terminals, bind address
bunny auth-status
```

## Web UI after configure

1. Open the UI (tunnel or public URL).
2. Log in with the owner account (+ MFA if enabled).
3. Create a **session** (project workspace).
4. Open **terminals**, **preview**, or **browser** tabs.
5. Invite teammates (session members) from the session UI.

## Optional: secrets vault

```bash
bunny secrets init
bunny secrets set OPENAI_API_KEY --scope system
export BUNNY_SECRETS_PASSPHRASE='your-passphrase'
```

See [Security](../security/).

## Discord next steps

1. [Discord application and server setup](../team-chats/discord/setup#discord-application-and-server)
2. [Discord workflows](../team-chats/discord/workflows) — linking channels, threads, Claude
3. [Discord slash commands](../team-chats/discord/commands) — full `/bunny` reference

## See also

- [First run](./first-run)
- [CLI reference](../reference/cli)
- [Install on Linux](./install-linux) — native release + systemd
