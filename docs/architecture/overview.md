# Architecture overview

Summary:

- **apps/server** — Rust agent (Axum + portable-pty + SQLite auth)
- **apps/web** — React + xterm.js client
- **apps/mobile** — Flutter client
- **apps/discord-bridge** — Discord bot (slash commands `/bunny …`)
- **crates/** — `bunny-core`, `bunny-auth`, `bunny-pty`, `bunny-browser`, `bunny-relay`, `bunny-secrets`, `bunny-push`, `bunny-discord`, `bunny-i18n`
- **packages/** — OpenAPI + WebSocket JSON Schema + i18n catalogs

All client traffic uses HTTPS/WSS with server-side RBAC. Internal Chromium CDP and VNC are never exposed publicly.

## Deeper dives

- [Web terminals (shells, tmux, WebSocket)](./terminals.md)
- [Discord integration](../integrations/discord.md)
- [Security](../security/README.md)
- [API reference](../api/README.md)
