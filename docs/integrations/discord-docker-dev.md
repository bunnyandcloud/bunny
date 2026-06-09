# Discord + Docker (Mac dev) — quick guide

Everything runs **inside the container**. On your Mac you only need Docker and the `docker-dev.sh` scripts.

## Simplest path (recommended)

```bash
./scripts/docker-dev.sh bootstrap
```

One command: installs Rust (first time), builds, runs `bunny configure` (email, password, Discord).

**First time with Discord?** Create the application and invite the bot to your server first — see [Discord application and server setup](discord.md#discord-application-and-server) (Steps 1–2), then continue below.

Then:

```bash
./scripts/docker-dev.sh shell
bunny run                    # terminal 1 — keep open
```

In another terminal:

```bash
./scripts/docker-dev.sh start-bridge
```

- Web UI: http://127.0.0.1:7681
- Discord: `/bunny help` then `/bunny link` (code from Web UI → session → **Discord**)

## Manual steps (equivalent)

```bash
./scripts/docker-dev.sh up
./scripts/docker-dev.sh shell    # installs Rust automatically if needed
bunny configure
bunny run                        # terminal 1
# terminal 2:
bunny discord bridge
```

## `bunny` commands

| Command | Purpose |
|---------|---------|
| `bunny setup --minimal` | Install Rust/tools (first time in container) |
| `bunny configure` | Admin account + optional Discord |
| `bunny discord setup` | Discord config only |
| `bunny discord bridge` | Start Discord bot |
| `bunny run` | Agent + Web UI (:7681) |

## Mac scripts

| Command | Purpose |
|---------|---------|
| `./scripts/docker-dev.sh bootstrap` | Install + interactive `bunny configure` |
| `./scripts/docker-dev.sh browser-setup` | Xvfb + Chromium + noVNC for Browser tab |
| `./scripts/docker-dev.sh shell` | Shell (auto `setup` if Rust missing) |
| `./scripts/docker-dev.sh start-agent` | `bunny run` |
| `./scripts/docker-dev.sh start-bridge` | `bunny discord bridge` |
| `./scripts/docker-dev.sh check-network` | Test DNS/HTTPS to Discord from container |
| `./scripts/docker-dev.sh status` | Health check |
| `./scripts/docker-dev.sh down -v` | Full reset |

## Troubleshooting

| Problem | Action |
|---------|--------|
| `Rust toolchain required` | `bunny setup --minimal` then `bunny configure` |
| Blank page on :7681 | `bunny run` |
| Browser: `Xvfb` / `No such file` | `./scripts/docker-dev.sh browser-setup` (`setup --minimal` does not install the browser stack) |
| `/bunny` does not respond | `start-bridge` (dedicated terminal). Stop: **Ctrl+C** in that terminal, or `./scripts/docker-dev.sh stop-bridge` |
| `405 Method Not Allowed` on `shell_close` / new command | Rebuild + **restart agent**: in container `cargo build --release -p bunny-server`, then Ctrl+C on `bunny run` and relaunch |
| Unknown `discord` subcommand | `bunny setup --minimal` (rebuilds CLI) |
| `DisallowedGatewayIntents` | Discord Portal → your bot → **Privileged Gateway Intents** → enable **Message Content Intent** (save), then restart `start-bridge` |
| `failed to lookup address` / `HTTP request to get gateway URL failed` when starting bridge | **Container DNS** (not Discord) — run `./scripts/docker-dev.sh check-network` then `down` + `up` to apply compose DNS; restart Docker Desktop if it persists |
| `invalid bridge token` on `/bunny link` | Token in `.discord/bridge.yaml` not in agent config — `bunny discord sync` then **restart `bunny run`** (Ctrl+C, relaunch) |
| `discord account not linked to bunny user` on `run` | Restart `bunny run` (recent fix), retry `/bunny run` — or redo `/bunny link` with a new Web UI code |
| `/bunny run` + `npm run dev` → 40s timeout | Rebuild `bunny-server`: `/bunny run` injects into tmux; if the command does not finish in ~8s, Discord reports **persistent process** + excerpt; full logs in Terminal tab |
| Stop `npm run dev` (or similar) | `/bunny run_stop` — Ctrl+C on the channel shell (`shell:<name>` if needed). Not `/bunny stop` (Claude tasks) |
| Pick a shell | `/bunny shell_list` then `/bunny run shell:<name> command:pwd` (without `shell:` = first shell created) |
| Create a shell | `/bunny shell_new` or `/bunny shell_new name:debug` |
| Close a shell | `/bunny shell_close shell:shell 1` (omit `shell:` if only one tab) |
| Shell snapshot | `/bunny snapshot` or `/bunny snapshot shell:shell 1` — Discord caption shows the shell |
| Full snapshot | `/bunny full_snapshot` — shell + browser (headless Chromium auto-started on :3000 or first preview) |
| Browser stream | `/bunny stream_browser_start` — read-only by default; `port:5173` for a local port; `interactive:true` for mouse/keyboard |
| Stop browser stream | `/bunny stream_browser_stop` — all active links for the channel; `url:<watch URL>` for one link |
| Browser: black screen in **Stream** / watch | Normal in Docker with legacy WebRTC — rebuild Web UI + agent; Stream/watch then use noVNC read-only (tunnel :7681). Restart `bunny run` |
| Duplicate slash commands (`run` + `shell_run`, each cmd ×2) | **Global + guild** registered in parallel. `./scripts/docker-dev.sh stop-bridge` then **one** `start-bridge`. Check `guild_id` in `.discord/bridge.yaml`. Quit Discord (Cmd+Q). Expected log: `removed stale global slash commands` |
| `@bunny` mention → no reply in thread | Rebuild + restart agent + bridge: `cargo build --release -p bunny-server -p bunny-discord-bridge`, then Ctrl+C on `bunny run` and `start-bridge`. Threads use `claude -p` (headless) — reply arrives in Discord after the call finishes (up to ~5 min) |
| Thread: `discord-*` shell, no Discord reply | Verify Claude Code is installed (`?claude=setup` in Web UI). Thread shell shows transcript of `claude -p` commands, not an interactive session |
| Thread: typing then silence (no GOAL) | Old bug: `claude -p` was blocked as an "interactive" command. Rebuild `bunny-server` + `bunny-discord-bridge`, restart agent + bridge |

### Discord threads (`@bunny` mention)

- Mention the bot in a channel → auto thread + `discord-*` shell in Web UI (project cwd).
- Claude runs **headless** (`claude -p --output-format json`); the response is posted **directly** in the thread (no tmux polling).
- Follow-up messages (reply or @mention) re-inject **Goal** (context) + thread history; only the user closes with **Goal!** (Claude does not declare the goal done).
- **Goal!** / **Cancel** close the thread shell (tab disappears from Web UI).
- ⛔ reaction on the last input message → interrupt the running Claude subprocess.
- `discord-*` shell in Web UI: transcript `[discord] $ claude -p …` appears after the call (reload tab if needed).
- `error_max_turns`: agent hit `discord.claude_max_turns` (default **30**); partial plan extracted from JSON if present — raise the limit in `config.yaml` or continue in the thread.
- **AskUserQuestion**: if Claude needs a choice, the bot posts **buttons** in the thread; after click, `claude -p --resume` continues with your answers. Multiple questions → one message/buttons per question.

### Enable Discord intents (required once)

1. [Discord Developer Portal](https://discord.com/developers/applications) → your application → **Bot**
2. **Privileged Gateway Intents** section
3. Enable **Message Content Intent** (for `/bunny` and `@bunny` mentions)
4. **Save Changes**
5. Restart `./scripts/docker-dev.sh start-bridge` (`bunny run` already running in the other terminal)

Full guide: [discord.md](discord.md).
