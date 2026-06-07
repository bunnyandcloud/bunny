# Bunny documentation

## Architecture

- [Overview](./architecture/overview.md) — monorepo layout and security boundaries
- [Web terminals](./architecture/terminals.md) — tmux, WebSocket protocol, persistence, F5 recovery

## Setup & operations

- [Installation](./install/README.md) — prerequisites, Docker, systemd, secrets vault
- [Security](./security/README.md) — auth, MFA, secrets, redaction, browser/VNC
- [API reference](./api/README.md) — REST and WebSocket routes

## Integrations

- [Discord](./integrations/discord.md) — [application & server setup](./integrations/discord.md#discord-application-and-server), bridge, slash commands, threads, OAuth
- [Discord + Docker (Mac dev)](./integrations/discord-docker-dev.md) — `docker-dev.sh` quick start

## Mobile

- [Flutter app](./mobile/README.md) — SSH tunnel, sessions, terminal
- [Push & WebRTC](./mobile/push-webrtc.md) — FCM, sidecar, browser video stream

## Other

- [Internationalization (i18n)](./i18n.md) — shared `en`/`fr` catalogs
- [noVNC read-only enforcement](./improvements/novnc-readonly-server-enforcement.md) — future server-side RFB filtering
