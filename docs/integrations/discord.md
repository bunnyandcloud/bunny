# Discord integration

Bunny can be controlled from Discord via a separate **Discord Bridge** process while live streaming stays in the Web UI (WebRTC).

## Architecture

- `bunny-server` — internal API `/api/v1/internal/discord/*`, link codes, snapshots, watch tokens, agent tasks
- `bunny-discord-bridge` — Discord bot (slash commands `/bunny …`)
- Web UI — generate link codes from session members modal (MFA password)

## Setup

1. Create a Discord application and bot; enable **Message Content** and **applications.commands** scopes.

2. Generate a bridge token on the agent host:

```bash
bunny discord token
```

Add to `~/.config/bunny/config.yaml`:

```yaml
discord:
  enabled: true
  bridge_token_hash: "<sha256 hex from command>"
  public_url: "https://your-bunny-host.example.com"
  link_code_ttl_minutes: 15
```

3. Configure `~/.config/bunny/discord-bridge.yaml`:

```yaml
discord:
  application_id: 123456789012345678
  bot_token: "YOUR_BOT_TOKEN"
bunny:
  internal_url: "http://127.0.0.1:7681"
  bridge_token: "same plaintext token as above"
  public_url: "https://your-bunny-host.example.com"
```

4. Run the bridge (alongside `bunny run`):

```bash
cargo run -p bunny-discord-bridge
```

5. In the Web UI, open a session → **Discord** → enter password → **Generate code**, then in Discord: `/bunny link CODE`.

**Docker on Mac:** see [discord-docker-dev.md](discord-docker-dev.md) (`docker-compose.dev.yml` + `scripts/run-discord-bridge.sh`).

## Commands (Discord)

| Command | Description |
|---------|-------------|
| `/bunny link` | Link channel to session |
| `/bunny unlink` | Remove link |
| `/bunny status` | Show link status |
| `/bunny snapshot` | PNG snapshot in channel |
| `/bunny shell_list` | List shells |
| `/bunny run` | Run shell command (Editor+ Bunny user linked) |
| `/bunny stream_start` | Post read-only watch URL |
| `/bunny ask/plan/do` | Claude agent task |
| `/bunny stop` | Cancel task |

## Watch links

`/bunny stream_start` returns a URL like `https://host/watch/<token>`. Opens a read-only WebRTC browser stream (same stack as the Web UI **Stream** tab).

## Security

- Bridge uses `Authorization: Bearer` with hashed token stored in config
- Shell/agent commands require Discord account linked to a Bunny user with Editor+ on the session
- Sensitive shell commands may require approval (Phase 2)
- All actions are written to `discord_audit_log`

## OAuth

`GET /api/v1/auth/discord/start` redirects to Discord OAuth for identity; link the returned `discord_user_id` via `POST /api/v1/auth/discord/link` while logged in.
