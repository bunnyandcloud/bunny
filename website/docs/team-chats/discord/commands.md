---
sidebar_position: 3
---

# Discord slash commands

All commands are prefixed with `/bunny`. Requires a **linked channel** unless noted. Shell and agent commands need a **linked Discord user** with Editor+ on the session.

Workflow context: [Workflows](./workflows).

## Linking and status

| Command | Description |
|---------|-------------|
| `/bunny link <code>` | Link channel to session (code from Web UI) |
| `/bunny unlink` | Remove channel link |
| `/bunny status` | Show current link and session info |
| `/bunny language locale:en` | Set locale (`en` or `fr`; linked user required) |

## Project and git

| Command | Description |
|---------|-------------|
| `/bunny project` | Show project directory |
| `/bunny project path:/path/to/repo` | Set project directory for session |
| `/bunny git action:status` | Git status in project cwd |
| `/bunny git action:diff` | Git diff |
| `/bunny git action:log` | Git log |
| `/bunny git action:checkout branch:name` | Checkout branch |
| `/bunny git action:branch name:feature` | Create branch |
| `/bunny git action:merge branch:name` | Merge branch |
| `/bunny git action:reset_hard` | Hard reset (destructive) |

## Shells

| Command | Description |
|---------|-------------|
| `/bunny shell_list` | List shells in session |
| `/bunny shell_new name:my-shell` | Create shell (name optional) |
| `/bunny shell_close shell:name` | Close shell |
| `/bunny run command:npm test` | Run command in shell (Editor+) |
| `/bunny run_stop` | Send Ctrl+C to foreground process |
| `/bunny file path:src/app.ts` | Download file from shell cwd (attachment) |

**`/bunny run`:** commands finishing within ~8s return full output in Discord. Long-running processes get a short excerpt; full logs in Web UI terminal.

## Screenshots

| Command | Description |
|---------|-------------|
| `/bunny snapshot` | Terminal PNG (`shell:` optional) |
| `/bunny full_snapshot` | Terminal + browser PNG (`url:` optional) |

## Browser watch stream

| Command | Description |
|---------|-------------|
| `/bunny stream_browser_start` | Start browser + watch URL |
| `/bunny stream_browser_start port:5173` | Target local dev port |
| `/bunny stream_browser_start url:http://127.0.0.1:3000` | Target URL |
| `/bunny stream_browser_start interactive:true` | Allow remote control (careful) |
| `/bunny stream_browser_stop` | Revoke watch link(s) for channel |
| `/bunny stream_browser_stop url:…` | Revoke one watch URL |

## Claude agents

| Command | Description |
|---------|-------------|
| `/bunny ask prompt:…` | Claude with channel context (`--resume`) |
| `/bunny plan prompt:…` | Same session as `ask` — planning style |
| `/bunny do prompt:…` | Claude with auto-approved file edits |
| `/bunny claude_reset` | Clear stored Claude session for channel |
| `/bunny stop` | Cancel task record (does not kill in-flight tmux Claude) |

## @mention (threads)

Not a slash command — **@mention the bot** in a linked channel with your task. Creates a thread + shell + Claude run. See [Workflows](./workflows#mention-thread-workflow).

## Permissions summary

| Action | Minimum role |
|--------|----------------|
| `/bunny link`, `status`, `snapshot` | Linked channel |
| `/bunny run`, `ask`, `plan`, `do`, git, shell | Editor + linked Discord user |
| Approve / Deny buttons | Admin |

## Examples

```text
/bunny link bunny-a1b2c3d4
/bunny run command:cargo test
/bunny ask prompt:Review the auth module and list risks
/bunny do prompt:Create a landing page at public/index.html
/bunny stream_browser_start port:5173
/bunny file path:README.md
```

Setup and OAuth: [Discord setup](./setup).
