# Architecture overview

See the technical plan for full details. Summary:

- **apps/server**: Rust agent (axum + portable-pty + SQLite auth)
- **apps/web**: React + xterm.js client
- **apps/mobile**: Flutter client
- **crates/**: bunny-core, bunny-auth, bunny-pty, bunny-browser, bunny-relay
- **packages/**: OpenAPI + WebSocket JSON Schema

All client traffic uses HTTPS/WSS with server-side RBAC. Internal Chromium CDP and VNC are never exposed publicly.
