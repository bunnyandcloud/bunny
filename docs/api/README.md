# API

Base path: `/api/v1`

OpenAPI spec: [packages/api-contracts/openapi.yaml](../../packages/api-contracts/openapi.yaml)

WebSocket events: [packages/realtime-protocol/events.schema.json](../../packages/realtime-protocol/events.schema.json)

## Auth

| Method | Path | Auth |
|--------|------|------|
| POST | `/auth/bootstrap` | No (once) |
| POST | `/auth/login` | No |
| POST | `/auth/mfa/verify` | No (MFA challenge) |
| POST | `/auth/logout` | Yes |
| GET/PATCH | `/auth/me` | Yes |
| GET | `/auth/mfa/status` | Yes |
| POST | `/auth/mfa/setup` | Yes |
| POST | `/auth/mfa/enable` | Yes |
| POST | `/auth/mfa/disable` | Yes |
| POST | `/auth/mfa/recovery/regenerate` | Yes |

## Invitations & users

| Method | Path | Auth |
|--------|------|------|
| POST | `/invitations/accept` | No |
| GET/POST/PATCH | `/users` | Owner |
| DELETE | `/users/:user_id` | Owner |

## Sessions

| Method | Path |
|--------|------|
| GET/POST | `/sessions` |
| GET/PATCH/DELETE | `/sessions/:id` |
| POST | `/sessions/:id/join` |
| POST | `/sessions/:id/invitations` |
| GET | `/sessions/:id/members` |
| PATCH/DELETE | `/sessions/:id/members/:user_id` |
| POST | `/sessions/:id/stop` |
| POST | `/sessions/:id/reset` |
| WS | `/sessions/:id/realtime` |

## Terminals

| Method | Path |
|--------|------|
| GET/POST | `/terminals` |
| GET/PATCH/DELETE | `/terminals/:id` |
| POST | `/terminals/:id/input` |
| POST | `/terminals/:id/resize` |
| POST | `/terminals/:id/restart` |
| WS | `/terminals/:id/ws` |

## Previews

| Method | Path |
|--------|------|
| GET/POST | `/previews` |
| DELETE | `/previews/:id` |

Reverse proxy: `GET /s/:sessionId/ports/:port/*`

## Browser

| Method | Path |
|--------|------|
| POST | `/browser-sessions` |
| GET | `/browser-sessions/:id` |
| POST | `/browser-sessions/:id/control` |
| POST | `/browser-sessions/:id/restart` |
| POST | `/browser-sessions/:id/reset` |
| WS | `/browser-sessions/:id/events` |
| POST | `/browser-sessions/:id/webrtc/offer` |
| POST | `/browser-sessions/:id/webrtc/candidate` |
| POST | `/browser-sessions/:id/webrtc/stop` |
| WS | `/browser-sessions/:id/vnc/ws` |
| GET | `/browser-sessions/:id/vnc/*` |

## Discord (authenticated)

| Method | Path |
|--------|------|
| POST | `/sessions/:id/discord/link-codes` |
| GET/DELETE | `/sessions/:id/discord/links` |
| GET | `/auth/discord/start` |
| POST/DELETE | `/auth/discord/link` |

Public OAuth callback: `GET /auth/discord/callback`

## Discord (bridge, internal)

Bearer bridge token. Base: `/internal/discord`

| Method | Path |
|--------|------|
| POST | `/link`, `/unlink` |
| GET | `/status` |
| POST | `/locale` |
| GET | `/user-locale` |
| GET | `/shell/list` |
| POST | `/shell/run`, `/shell/run/stop`, `/shell/file`, `/shell/new`, `/shell/close` |
| POST | `/browser/open` |
| GET | `/browser/status` |
| POST | `/snapshot` |
| POST | `/stream/start`, `/stream/stop` |
| GET | `/stream/status` |
| POST | `/agent/ask`, `/agent/plan`, `/agent/do` |
| POST | `/task/stop` |
| POST | `/approval/resolve` |
| POST | `/claude/reset` |
| POST | `/thread/bind`, `/thread/input`, `/thread/answer`, `/thread/discussion`, `/thread/stop`, `/thread/finalize`, `/thread/status`, `/thread/attachment` |
| POST | `/project/set` |
| GET | `/project` |
| POST | `/git` |
| POST | `/follow/start`, `/follow/stop` |
| POST | `/audit` |

See [Discord integration](../integrations/discord.md).

## Watch (public, token-based)

| Method | Path |
|--------|------|
| GET | `/watch/:token` |
| POST | `/watch/:token/access` |
| WS | `/watch/:token/vnc/ws` |
| GET | `/watch/:token/vnc/*` |

## Timeline & audit

| Method | Path |
|--------|------|
| GET | `/timeline?session_id=&since=&limit=` |
| GET | `/audit-logs` |

## Voice

| Method | Path |
|--------|------|
| POST | `/voice/intent` |
| POST | `/voice/confirm` |

## Push & WebRTC

| Method | Path |
|--------|------|
| POST | `/push/register` |
| DELETE | `/push/register/:device_id` |
| GET | `/webrtc/config` |
| POST | `/sessions/:id/webrtc/offer` |
| POST | `/sessions/:id/webrtc/candidate` |

## Secrets vault

| Method | Path |
|--------|------|
| GET | `/secrets/status` |
| POST | `/secrets/init`, `/secrets/unlock`, `/secrets/lock` |
| GET/POST | `/secrets` |
| GET | `/secrets/:name/reveal` |
| DELETE | `/secrets/:name` |

## Claude Code

| Method | Path |
|--------|------|
| GET | `/claude/status` |
| POST | `/claude/install` |
| POST | `/claude/auth/start`, `/claude/auth/code`, `/claude/auth/detect-code` |

## Agent info

| Method | Path | Auth |
|--------|------|------|
| GET | `/agent/info` | No |
