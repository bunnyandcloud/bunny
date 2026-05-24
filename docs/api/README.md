# API

Base path: `/api/v1`

## Auth

| Method | Path | Auth |
|--------|------|------|
| POST | `/auth/bootstrap` | No (once) |
| POST | `/auth/login` | No |
| POST | `/auth/logout` | Yes |
| GET | `/auth/me` | Yes |

## Sessions

| Method | Path |
|--------|------|
| GET/POST | `/sessions` |
| GET | `/sessions/:id` |
| POST | `/sessions/:id/join` |
| POST | `/sessions/:id/stop` |
| POST | `/sessions/:id/reset` |
| WS | `/sessions/:id/realtime` |

## Terminals

| Method | Path |
|--------|------|
| GET/POST | `/terminals` |
| GET/DELETE | `/terminals/:id` |
| POST | `/terminals/:id/input` |
| POST | `/terminals/:id/resize` |
| WS | `/terminals/:id/ws` |

## Previews

Reverse proxy: `GET /s/:sessionId/ports/:port/*`

## Browser

| Method | Path |
|--------|------|
| POST | `/browser-sessions` |
| GET | `/browser-sessions/:id` |
| WS | `/browser-sessions/:id/events` |

## Other

- `GET /timeline?session_id=&since=&limit=`
- `POST /voice/intent`, `POST /voice/confirm`

OpenAPI spec: [packages/api-contracts/openapi.yaml](../../packages/api-contracts/openapi.yaml)

WebSocket events: [packages/realtime-protocol/events.schema.json](../../packages/realtime-protocol/events.schema.json)
