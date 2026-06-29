---
sidebar_position: 1
---

# CLI reference

The `bunny` command is the agent CLI. Run from a git checkout (`./bunny …`) or from an installed binary (`/usr/local/bin/bunny`).

```bash
bunny --help
bunny <command> --help
```

## Setup and run

| Command | Description |
|---------|-------------|
| `bunny configure` | First-run wizard: owner account, MFA, optional Discord |
| `bunny config-init` | Write default `~/.config/bunny/config.yaml` if missing |
| `bunny run` | Start agent + Web UI (builds UI on first run in dev) |
| `bunny start` | Start API only (UI if `dist` exists) |
| `bunny doctor` | Check dependencies (Chromium, Node, sidecars, tmux) |
| `bunny status` | Show terminals, previews, bind address |

### `bunny run` flags

| Flag | Default | Description |
|------|---------|-------------|
| `--host` | `127.0.0.1` | Bind address (`0.0.0.0` in Docker) |
| `--port` | `7681` | HTTP port |
| `--no-web-ui` | off | Skip Web UI build/serve |
| `--web-ui-rebuild` | off | Force `npm run build` in apps/web |
| `--no-discord-bridge` | off | Do not start Discord bridge with agent |

## Auth and users

| Command | Description |
|---------|-------------|
| `bunny init-auth` | Initialize auth database |
| `bunny auth-status` | Show auth bootstrap status |
| `bunny user invite …` | Create invitation (MVP: owner CLI) |
| `bunny user revoke …` | Revoke user by email |

## Sessions and dev

| Command | Description |
|---------|-------------|
| `bunny dev --cmd "…"` | Dev session + terminal + optional preview/browser |
| `bunny stop --session-id …` | Stop session |
| `bunny recover <session_id>` | Recover session |
| `bunny reset <session_id>` | Reset session |

## Secrets

| Command | Description |
|---------|-------------|
| `bunny secrets init` | Create encrypted vault |
| `bunny secrets set NAME --scope system\|project\|session` | Store secret |
| `bunny secrets list` / `get` / `remove` | Manage vault |
| `bunny secrets unlock` | Unlock for current shell |
| `bunny secrets status` | Vault state |

See [Security](../security/).

## Discord

| Command | Description |
|---------|-------------|
| `bunny discord setup` | Write agent + bridge config (bot token, OAuth) |
| `bunny discord bridge` | Run Discord bot (needs running agent) |
| `bunny discord sync` | Sync agent `config.yaml` from bridge YAML (token mismatch fix) |

### `bunny discord setup` flags

| Flag | Description |
|------|-------------|
| `--application-id` / `DISCORD_APPLICATION_ID` | Discord application ID |
| `--bot-token` / `DISCORD_BOT_TOKEN` | Bot token |
| `--guild-id` / `DISCORD_GUILD_ID` | Server ID for instant slash command registration |
| `--bridge-out` | Bridge YAML path (default `.discord/bridge.yaml`) |
| `--skip-oauth` | Bot only, no user linking |
| `--public-url` / `BUNNY_PUBLIC_URL` | Public base URL for watch links and OAuth |

Full Discord usage: [Discord setup](../team-chats/discord/setup), [slash commands](../team-chats/discord/commands).

## Service (systemd)

| Command | Description |
|---------|-------------|
| `bunny service install` | Install systemd unit |
| `bunny service status` | Service status |

## Shell wrapper (git checkout only)

| Command | Description |
|---------|-------------|
| `./bunny setup` | Install prerequisites + build release binary + PATH symlink |
| `./bunny setup --minimal` | Skip browser stack |

## Environment

| Variable | Description |
|----------|-------------|
| `BUNNY_INSTALL_DIR` | Pre-built install root (`/opt/bunny`) |
| `BUNNY_SERVER__BIND_HOST` | Override `server.bind_host` |
| `BUNNY_SERVER__PORT` | Override `server.port` |
| `BUNNY_SECRETS_PASSPHRASE` | Auto-unlock secrets vault |
| `BUNNY_DOCKER_DEV=1` | Docker dev hints for Discord setup |

Config file: `~/.config/bunny/config.yaml`. See [Configure the server](../getting-started/configure-server).
