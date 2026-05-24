# Mobile app (Flutter)

Store-ready flow: the user adds a server (host + SSH credentials), opens an **integrated SSH tunnel** to `127.0.0.1:bunnyPort` on the remote machine, then signs in to bunny (email/password on the agent — not the SSH password).

## Run

```bash
cd apps/mobile
flutter pub get
flutter run
```

No `BUNNY_SERVER` dart-define is required anymore: the app talks to `http://127.0.0.1:<localForwardPort>` through the tunnel.

## Server setup

On the VPS, bind bunny to loopback only:

```bash
bunny start --host 127.0.0.1 --port 7681
```

Ensure SSH access (password or PEM private key; stored encrypted on device).

## App flow

1. **Servers** — add profile (host, SSH user/port, bunny port, local forward port, SSH password).
2. **Connect** — SSH tunnel: phone `127.0.0.1:localForwardPort` → remote `127.0.0.1:bunnyPort`.
3. **Login** — bunny credentials; token stored per profile in secure storage.
4. **Session** — terminal, preview WebView, status tab (tunnel state).

Restore on launch: if a profile has a saved bunny token, the app reconnects SSH and resumes when possible.

## Features (MVP + Sprint B)

- Multi-server profiles (`ServerStore`)
- Integrated SSH local forward (`dartssh2` + `ServerSocket`)
- Agent discovery `GET /api/v1/agent/info` (no auth)
- Login, session bootstrap, interactive terminal
- Browser preview via WebView (port proxy)
- Voice push-to-talk sheet (Insert / Run / Cancel)

## Push & WebRTC

See [push-webrtc.md](./push-webrtc.md) for Firebase + FCM, WebRTC sidecar, and **browser video stream** (Sprint C).

## Planned
- Hardware-backed master key (Secure Enclave / StrongBox) for credential envelopes
- App Store / Play Store packaging

## Deep links

Configure `bunny://session/:id` and universal links to open the app; user must still authenticate.
