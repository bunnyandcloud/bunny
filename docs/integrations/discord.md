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

## Thread workflow (@mention)

In a linked channel, **@mention the bridge bot** with your task. The bot:

1. Creates a **Discord thread** named from your message
2. Opens a **dedicated shell** on the server (cwd = session `project_path`, or `/bunny project path:…`)
3. Starts **Claude Code** interactively (auto-accepts the “trust this folder” prompt)
4. Streams shell output into the thread
5. Optional: creates a **git branch** per thread when the project cwd is a git repo

In the thread:

- **Reply to the bot** or **@mention the bot** → input to Claude in that shell
- Other messages → stored as discussion context for the next Claude input
- **Goal!** / **Cancel** buttons → close shell; Cancel resets git to thread start when git was enabled
- **⛔ / 🛑** reaction on your last bot-directed message → interrupt Claude
- `/bunny project` / `/bunny git` — see below

Legacy `@bunny and claude …` mentions are removed.

## Commands (Discord)

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
| `/bunny stop` | Cancel task record (does not kill an in-flight `claude` process in tmux) |

## Watch links

`/bunny stream_browser_start` starts headless Chromium if needed, then returns a URL like `https://host/watch/<token>`. By default Chromium opens the first registered preview port for the session, or `http://127.0.0.1:3000`. Use **`port:`** (e.g. `port:5173`) to target a specific local dev server without a full URL; **`url:`** takes precedence when both are set. By default the watch page is **read-only** (noVNC `view_only`). Pass **`interactive:true`** to allow mouse and keyboard on the shared link — anyone with the URL can control the browser until the link expires.

**Security:** only use `interactive:true` when you intend to grant remote control via the watch link. Read-only links use a locked noVNC profile (settings hidden, `view_only` forced in the embedded UI). This stops casual bypass via noVNC settings; **server-side RFB input blocking** is not implemented yet — see [novnc-readonly-server-enforcement](../improvements/novnc-readonly-server-enforcement.md).

`/bunny stream_browser_stop` revokes watch link(s) for the **current Discord channel** (interactive and read-only alike). Without `url:`, **all** active watch tokens for that channel are stopped. With `url:` set to a watch URL from `stream_browser_start`, only that token is revoked (the URL must belong to the same channel). Open watch pages disconnect their noVNC WebSocket immediately; the watch shell polls every 2s and shows an error without requiring a manual refresh. Chromium keeps running; only the public `/watch/:token` access is invalidated.

## Claude from Discord

- **`ask` / `plan`**: each linked Discord channel keeps a Claude `session_id`. Follow-up prompts reuse context (`claude -p --resume`). Use `/bunny claude_reset` to start fresh.
- **`do`**: same resume session as `ask`/`plan`, plus `--permission-mode acceptEdits` so landing pages and file writes complete in Discord without the interactive welcome screen. Follow up with another `/bunny do` on the same channel to continue the task.
- **Autoriser / Refuser** buttons apply to risky shell commands and (legacy) tmux tool prompts; most `do` file edits do not need a button.
- **Viewing whole files:** Discord chat is limited to ~2000 characters per message. Use **`/bunny file path:landing-page.html`** to receive the complete file as an attachment (open in browser or editor). For files larger than 24 MB or interactive viewing, use the **Web UI** terminal (`cat`, `less`, preview in browser).

## Security

- Bridge uses `Authorization: Bearer` with hashed token stored in config
- Shell/agent commands require Discord account linked to a Bunny user with Editor+ on the session
- Tool approvals and risky shell commands require **DiscordApprove** (buttons or API)
- All actions are written to `discord_audit_log`

## OAuth

`GET /api/v1/auth/discord/start` redirects to Discord OAuth for identity; link the returned `discord_user_id` via `POST /api/v1/auth/discord/link` while logged in.
