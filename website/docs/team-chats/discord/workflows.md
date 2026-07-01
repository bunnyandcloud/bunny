---
sidebar_position: 2
---

# Discord workflows

How the Discord extension works day-to-day: architecture, linking, threads, and Claude agents.

## What runs where

| Component | Role |
|-----------|------|
| **bunny agent** (`bunny run`) | API, Web UI, terminals, browser, internal Discord API |
| **Discord bridge** (`bunny discord bridge`) | Discord bot — slash commands, threads, buttons |
| **Web UI** | Sessions, link codes, OAuth « Connect Discord », live terminals |

`bunny run` can auto-start the bridge when Discord is configured. On Mac Docker dev, the bridge sometimes runs on the host — see [Docker on Mac](./docker-mac).

```text
Discord users  →  bridge bot  →  agent API (/api/v1/internal/discord/*)
Web UI users   →  agent API   →  same sessions / shells
```

## One-time setup

Follow [Discord application and server setup](./setup#discord-application-and-server):

1. Create Discord application + bot token
2. Invite bot to your server
3. `bunny configure` or `bunny discord setup`
4. `bunny run` (+ bridge if separate)

## Link a channel to a Bunny session

A **channel link** connects one Discord text channel to one Bunny session.

1. Web UI → open session → **Discord** → enter password → **Generate code**
2. In Discord (channel on the server where the bot was invited):

```
/bunny link YOUR_CODE
```

3. Test: `/bunny status`

Codes expire (default 15 minutes). Generate a new code if needed.

Remove link: `/bunny unlink`

## Link your Discord user (teammates)

Separate from channel linking — each person links **their Discord account** to **their Bunny account**:

1. Admin links the channel (above)
2. Teammate has a Bunny account on the session (Editor+ for control)
3. Teammate opens Web UI **home page** → **Connect Discord** (OAuth)

After OAuth, actions in the linked channel are attributed to their Bunny user. Permissions follow session role:

| Bunny role | In Discord |
|------------|------------|
| Viewer | Discussion, read-only watch links |
| Editor | Shell commands, `/bunny run`, Claude `ask`/`plan`/`do` |
| Admin / Owner | Above + Approve/Deny risky commands |

## @mention thread workflow

In a **linked channel**, @mention the bot with a task:

1. Bot creates a **Discord thread** (title from your message)
2. Opens a **dedicated shell** on the server (session project path)
3. Runs **Claude Code** headlessly and posts the reply in the thread
4. Optional: git branch per thread when cwd is a git repo

In the thread:

- **Reply or @mention the bot** → continues with thread context (`claude -p --resume`)
- **Goal!** / **Cancel** buttons → close shell; Cancel may reset git
- **⛔** reaction on your last message → interrupt running Claude
- **AskUserQuestion** → bot posts choice buttons; answer continues the agent

The matching `discord-*` shell appears in the Web UI terminal list.

## Slash commands vs threads

| Use case | How |
|----------|-----|
| One-off shell command | `/bunny run command:…` |
| Claude read/plan with memory | `/bunny ask` or `/bunny plan` |
| Claude edits files | `/bunny do` |
| Screenshots | `/bunny snapshot`, `/bunny full_snapshot` |
| Browser watch link | `/bunny stream_browser_start` |
| Project directory | `/bunny project path:…` |
| Git operations | `/bunny git action:status` (etc.) |

Full list: [Slash commands](./commands).

## Claude sessions per channel

- **`ask` / `plan` / `do`** share a per-channel Claude `session_id` (`--resume`)
- Reset context: `/bunny claude_reset`
- `do` uses auto-approved file edits (`acceptEdits`) for landing pages and file writes in Discord
- Long output: use `/bunny file path:…` for full file as attachment (up to 24 MB)

## Watch links (browser stream)

`/bunny stream_browser_start` returns a URL like `https://host/watch/<token>`. The host comes from `server.public_url` in `config.yaml` (set during `bunny configure`).

- Default: **read-only** noVNC
- `interactive:true` — remote control (use carefully)
- `port:` or `url:` — target dev server

Stop: `/bunny stream_browser_stop`

## Approvals

Risky shell commands may post **Approve / Deny** buttons (Admin+). Audit entries go to `discord_audit_log`.

## Troubleshooting

| Symptom | Fix |
|---------|-----|
| `/bunny` missing | Bridge not running — `bunny discord bridge` or restart `bunny run` |
| Link code rejected | New code from Web UI; check expiry |
| `invalid bridge token` | `bunny discord sync`, restart agent |
| User can't run commands | Connect Discord on home page; need Editor+ on session |
| Commands on wrong server | Set `guild_id` in bridge YAML, restart bridge |

Setup details: [Discord setup](./setup#quick-troubleshooting).
