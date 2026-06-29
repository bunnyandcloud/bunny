# Push & WebRTC (mobile v1)

## Push (FCM)

1. Create a Firebase project and add iOS/Android apps.
2. Run from `apps/mobile`:

   ```bash
   dart pub global activate flutterfire_cli
   flutterfire configure
   ```

   This replaces `lib/firebase_options.dart`.

3. On the **agent server**, set the FCM server key:

   ```bash
   export BUNNY_FCM_SERVER_KEY='AAAA...'
   bunny start --host 127.0.0.1 --port 7681
   ```

   Or in `~/.config/bunny/config.yaml`:

   ```yaml
   push:
     enabled: true
     fcm_server_key: "AAAA..."
   ```

4. After login, the app calls `POST /api/v1/push/register` with the FCM token.

Alerts are sent for console `error`/`warn` and session status changes.

## WebRTC

The agent starts a **Node sidecar** (`apps/server/webrtc-sidecar`, `@roamhq/wrtc`) on `127.0.0.1:18782` when you run `bunny start`.

Requirements on the server:

```bash
cd apps/server/webrtc-sidecar && npm install
```

Mobile flow (over the SSH tunnel):

1. `GET /api/v1/webrtc/config` — ICE servers (STUN + optional TURN)
2. Create `RTCPeerConnection` + data channel `bunny`
3. `POST /api/v1/sessions/:id/webrtc/offer` — SDP offer → answer
4. Trickle ICE via `POST .../webrtc/candidate` and `webrtc.ice` on session realtime WS

Optional TURN (UDP blocked / strict NAT) in `config.yaml`:

```yaml
webrtc:
  enabled: true
  sidecar_port: 18782
  turn_url: "turn:your-vps:3478?transport=tcp"
  turn_username: "bunny"
  turn_credential: "secret"
```

Status tab in the app shows WebRTC and push state.

## Sprint C — WebRTC browser video

The **Browser** tab streams the remote Chromium desktop via **CDP screencast → WebRTC video** (not only HTTP preview).

1. Agent starts browser stack (`bunny dev --browser`) and WebRTC sidecar.
2. Mobile creates a browser session via API, then **Stream via WebRTC** (auto when opening the Browser tab).
3. Fallback: HTTP preview WebView (`/s/:sessionId/ports/3000/`) if WebRTC fails.

Server routes:

- `POST /api/v1/browser-sessions` — create browser
- `POST /api/v1/browser-sessions/:id/webrtc/offer` — SDP
- `POST /api/v1/browser-sessions/:id/webrtc/candidate` — ICE

Sidecar uses Playwright `connectOverCDP` + `Page.startScreencast` (requires Linux browser stack on the agent).
