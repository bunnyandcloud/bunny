---
sidebar_position: 1
---

# Discord setup

Bunny can be controlled from Discord via a separate **Discord Bridge** process while live streaming stays in the Web UI (WebRTC).

**Also read:**
- [Workflows](./workflows) — how linking, threads, and Claude work day-to-day
- [Slash commands](./commands) — full `/bunny` command reference
- [Docker on Mac](./docker-mac)

## Architecture

- `bunny-server` — internal API `/api/v1/internal/discord/*`, link codes, snapshots, watch tokens, agent tasks
- `bunny-discord-bridge` — Discord bot (slash commands `/bunny …`)
- Web UI — generate link codes from session members modal (MFA password)

## Discord application and server

This section is the **step-by-step guide** for beginners: create the Discord app/bot, add it to a server, then wire it to Bunny.

### Three things to understand

| Piece | What it is | Where it lives |
|-------|------------|----------------|
| **Discord application** | Your bot’s identity (ID, token, OAuth) | [Discord Developer Portal](https://discord.com/developers/applications) |
| **Discord server** (guild) | The place where you chat and run `/bunny` | Your Discord client — you **invite** the bot there |
| **Channel link** | Connects one Discord channel to one Bunny session | Web UI link code + `/bunny link` in Discord |

An application is **not** tied to any server until you invite the bot. Bunny does not pick a server for you.

```text
Developer Portal (app + bot token)
        ↓ invite URL
Discord server (guild) — bot appears in member list
        ↓ bunny configure (Discord setup) or bunny discord setup (+ optional guild_id)
Bunny bridge — registers /bunny commands
        ↓ link code from Web UI
/bunny link — channel ↔ Bunny session
```

### Step 1 — Create the application and bot

1. Open [Discord Developer Portal](https://discord.com/developers/applications) → **New Application** → name it (e.g. “Bunny”).
2. **General Information** → copy **Application ID** (long number). You need it for Bunny.
3. **Bot** → **Add Bot** (or **Reset Token** if you already have one) → copy the **Token**.  
   This is **not** the OAuth Client Secret (that is under OAuth2).
4. Under **Privileged Gateway Intents**, enable **Message Content Intent** → **Save Changes**.  
   Required for `@mention` threads.

### Step 2 — Invite the bot to your Discord server

The bot must be a **member** of the server where you will use `/bunny`.

1. In the portal: **OAuth2 → URL Generator**.
2. Scopes: check **`bot`** and **`applications.commands`**.
3. Bot permissions (minimum): Send Messages, Read Message History, Use Slash Commands, Create Public Threads (and Send Messages in Threads).
4. Copy the generated URL, open it in a browser, pick your **Discord server**, authorize.

**Check:** the bot appears in the server’s member list. If not, repeat this step — nothing else will work.

**Server ID (recommended for dev):** enable Discord **Developer Mode** (Settings → Advanced), right-click your server icon → **Copy Server ID**. You will pass this as `guild_id` so `/bunny` registers on that server immediately (see Step 4).

### Step 3 — Start Bunny and configure Discord

On the agent host (or inside the Docker container), after Steps 1–2 (portal + bot invite):

```bash
bunny configure    # owner account; prompts for Discord setup if you accept
bunny run          # agent + Web UI (bridge auto-starts when configured)
```

During **`bunny configure`**, if you accept Discord setup, Bunny runs the same flow as `bunny discord setup`: it asks for **Application ID** and **Bot Token** (Step 1), writes `~/.config/bunny/config.yaml` and `.discord/bridge.yaml`, and optionally configures OAuth. **If you completed that, skip Step 4** and go to Step 5.

**Docker on Mac:** see [Docker on Mac](./docker-mac) — `./scripts/docker-dev.sh bootstrap` runs `bunny configure` for you, then `bunny run` + `start-bridge`.

### Step 4 — Configure Bunny for your application (if you skipped Discord in Step 3)

**Only needed** if you declined Discord during `bunny configure`, or you are reconfiguring after a new application / token rotation.

```bash
bunny discord setup
```

This writes the same files as the configure wizard:

- **Agent:** `~/.config/bunny/config.yaml` (bridge token hash, public URL, optional OAuth)
- **Bridge:** `.discord/bridge.yaml` in the repo (or `--bridge-out` path) — `application_id`, `bot_token`, optional `guild_id`

For local dev, public URL is usually `http://127.0.0.1:7681`.

**Guild ID (optional but recommended):** neither `bunny configure` nor `bunny discord setup` prompts for it interactively — add your server ID (Step 2) so `/bunny` registers on that server immediately:

```bash
bunny discord setup --guild-id YOUR_SERVER_ID
```

Or edit `.discord/bridge.yaml` after setup:

```yaml
discord:
  application_id: 123456789012345678
  bot_token: "YOUR_BOT_TOKEN"
  guild_id: 987654321098765432
```

Restart `bunny run` (and the bridge if it runs separately) after changing config.

**Re-run:** `bunny configure` also offers to reconfigure Discord when `.discord/bridge.yaml` already exists — equivalent to running `bunny discord setup` again.

### Step 5 — Link a channel to a Bunny session

1. Web UI → open a session → **Discord** → enter password → **Generate code**.
2. In Discord, in a channel on the server where the bot was invited: `/bunny link YOUR_CODE`.

Test with `/bunny status`. You can `/bunny unlink` to remove the link.

### Changing server or application

| Situation | What to do |
|-----------|------------|
| **New Discord server** | Invite the same bot (Step 2), update `guild_id` in bridge config, restart bridge, `/bunny link` again on the new channel |
| **New Discord application** | Full Step 1 + `bunny discord setup` with new ID/token; re-invite bot; update OAuth redirect URI if you use home-page linking |
| **Commands missing or duplicated** | Set `guild_id`, restart bridge once; quit Discord (Cmd+Q) if autocomplete is stale — see [Docker on Mac](./docker-mac#troubleshooting) |

Stale channel links in Bunny’s database (old server deleted) do not block a new server — generate a fresh link code and run `/bunny link` on the new channel.

### Quick troubleshooting

| Symptom | Likely fix |
|---------|------------|
| Bot not in server member list | Repeat Step 2 (invite URL) |
| `/bunny` does not appear | Bridge not running — `bunny discord bridge` or `./scripts/docker-dev.sh start-bridge` |
| `invalid bridge token` on link | `bunny discord sync` then restart `bunny run` |
| Link code rejected | New code from Web UI; codes expire (default 15 min) |

## Setup (reference)

The wizard above is the supported path. Equivalent manual layout:

**Agent** (`~/.config/bunny/config.yaml`):

```yaml
discord:
  enabled: true
  bridge_token_hash: "<sha256 of bridge_token — written by bunny discord setup>"
  public_url: "https://your-bunny-host.example.com"
  link_code_ttl_minutes: 15
  claude_max_turns: 30   # `--max-turns` for @mention thread agents (default 30)
```

**Bridge** (`.discord/bridge.yaml` or `~/.config/bunny/discord-bridge.yaml`):

```yaml
discord:
  application_id: 123456789012345678
  bot_token: "YOUR_BOT_TOKEN"
  guild_id: 987654321098765432   # optional; recommended for dev
bunny:
  internal_url: "http://127.0.0.1:7681"
  bridge_token: "plaintext bridge token"
  public_url: "https://your-bunny-host.example.com"
```

Run the bridge alongside `bunny run`, or separately:

```bash
bunny discord bridge
# or: cargo run -p bunny-discord-bridge
```

**Docker on Mac:** [Docker on Mac](./docker-mac).

## Link your Discord account (user)

Each Bunny user links **their own** Discord identity once from the **home page** (`Discord account` card → **Connect Discord**). This is separate from linking a **channel** to a session (above).

Requirements:

1. OAuth configured on the agent (`bunny discord setup` includes OAuth, or set `config.yaml` fields below).
2. Redirect URI registered in the [Discord Developer Portal](https://discord.com/developers/applications) → OAuth2:

```text
https://your-bunny-host.example.com/api/v1/auth/discord/callback
```

```yaml
discord:
  oauth_client_id: "<Discord Application ID>"
  oauth_client_secret: "<OAuth client secret>"
  oauth_redirect_uri: "https://your-bunny-host.example.com/api/v1/auth/discord/callback"
```

After linking, any session member can interact on **already linked Discord channels** without running `/bunny link` themselves. Actions are attributed to their Bunny account; permissions follow their **session role**:

| Role | Discord capabilities |
|------|----------------------|
| **Viewer** | Thread discussion, watch links (read-only) |
| **Editor** | Control threads, shell commands, Claude agents |
| **Admin / Owner** | Above + **Approve / Deny** approval buttons |

Typical team flow:

1. Admin links the Discord **channel** to the session (`/bunny link` + link code).
2. Admin invites teammates to the session (Editor or higher for control).
3. Each teammate creates a Bunny account, opens the **home page**, and clicks **Connect Discord**.
4. Teammates use the linked channel — no per-user `/bunny link` needed.

## Thread workflow (@mention)

See [Workflows — @mention threads](./workflows#mention-thread-workflow) for the full guide.

In a linked channel, **@mention the bridge bot** with your task. The bot creates a thread, opens a shell, and runs Claude Code. In-thread replies continue the conversation.

## Commands (Discord)

Full reference: **[Slash commands](./commands)**.

Quick list:

| Command | Description |
|---------|-------------|
| `/bunny link` | Link channel to session |
| `/bunny project` | Show or set project directory (`path:` optional) |
| `/bunny git` | Git in project cwd (`action:` status, diff, log, checkout, branch, merge, reset_hard) |
| `/bunny unlink` | Remove link |
| `/bunny status` | Show link status |
| `/bunny snapshot` | Shell PNG (`shell:` optional — see `shell_list`) |
| `/bunny full_snapshot` | Shell + browser PNG (starts headless Chromium if needed; optional `url:`) |
| `/bunny shell_list` | List shells |
| `/bunny shell_new` | Create shell (`name:` optional — auto `shell N`) |
| `/bunny shell_close` | Close shell (`shell:` required if multiple) |
| `/bunny run` | Run shell command (Editor+ Bunny user linked) in the Web UI shell. Commands that finish within ~8s return full output; **long / persistent** processes get an immediate Discord reply with a live excerpt — full logs in the Terminal tab |
| `/bunny run_stop` | Send **Ctrl+C** to the foreground process in the shell (default: last shell used in this channel; optional `shell:`). Stops e.g. `npm run dev` started via `/bunny run` |
| `/bunny file` | Download a file from the shell cwd as a **Discord attachment** (full file up to 24 MB) |
| `/bunny stream_browser_start` | Start browser + watch URL (optional `url:` or `port:`; `interactive:true` for read+write) |
| `/bunny stream_browser_stop` | Stop browser watch stream(s) in this channel (optional `url:` for one link) |
| `/bunny ask/plan` | Claude session with **context** (`claude -p --resume` per channel) — read/plan style prompts |
| `/bunny do` | Claude agent with **context** (`--resume`) and auto-approved file edits (`acceptEdits`) — creates/updates files without stalling on the welcome screen |
| `/bunny claude_reset` | Clear the stored `ask`/`plan` conversation id for this channel |
| `/bunny language` | Set UI locale (`locale:fr` or `locale:en`; requires linked Bunny user) |
| `/bunny stop` | Cancel task record (does not kill an in-flight `claude` process in tmux) |

Details and examples: [Slash commands](./commands).

## Watch links

`/bunny stream_browser_start` starts headless Chromium if needed, then returns a URL like `https://host/watch/<token>`. By default Chromium opens the first registered preview port for the session, or `http://127.0.0.1:3000`. Use **`port:`** (e.g. `port:5173`) to target a specific local dev server without a full URL; **`url:`** takes precedence when both are set. By default the watch page is **read-only** (noVNC `view_only`). Pass **`interactive:true`** to allow mouse and keyboard on the shared link — anyone with the URL can control the browser until the link expires.

**Security:** only use `interactive:true` when you intend to grant remote control via the watch link. Read-only links use a locked noVNC profile (settings hidden, `view_only` forced in the embedded UI). This stops casual bypass via noVNC settings; **server-side RFB input blocking** is not implemented yet — see [novnc-readonly-server-enforcement](../../improvements/novnc-readonly-server-enforcement.md).

`/bunny stream_browser_stop` revokes watch link(s) for the **current Discord channel** (interactive and read-only alike). Without `url:`, **all** active watch tokens for that channel are stopped. With `url:` set to a watch URL from `stream_browser_start`, only that token is revoked (the URL must belong to the same channel). Open watch pages disconnect their noVNC WebSocket immediately; the watch shell polls every 2s and shows an error without requiring a manual refresh. Chromium keeps running; only the public `/watch/:token` access is invalidated.

## Claude from Discord

See [Workflows — Claude sessions](./workflows#claude-sessions-per-channel) and [Slash commands — Claude agents](./commands#claude-agents).

## Security

- Bridge uses `Authorization: Bearer` with hashed token stored in config
- Shell/agent commands require Discord account linked to a Bunny user (home page OAuth) with Editor+ on the session
- Tool approvals and risky shell commands require **DiscordApprove** (Admin+; buttons or API)
- All actions are written to `discord_audit_log`

## OAuth (admin setup)

`bunny discord setup` configures the **bot/bridge** and then prompts for **OAuth** (user account linking from the home page). You can skip OAuth with `--skip-oauth`, or configure OAuth only with `--oauth-only`.

```bash
bunny discord setup
```

Legacy alias (OAuth only): `bunny discord oauth-setup`.

Or set `oauth_client_id`, `oauth_client_secret`, and `oauth_redirect_uri` in `~/.config/bunny/config.yaml` manually. Users then link from the Web UI home page; `GET /api/v1/auth/discord/start` requires a Bunny session cookie.

During **`bunny configure`**, accepting Discord setup runs the same merged flow (bridge + optional OAuth).
