---
unlisted: true
---

# Improvement: server-side read-only noVNC enforcement

**Status:** future track (v1 accepted on client)  
**Context:** Discord watch links (`/watch/:token`), Web UI Stream tab (read-only)  
**Current implementation:** `apps/server/src/novnc_proxy.rs` (`NovncEmbedLock`, `bunny_lock` parameter)

---

## Current behavior (v1)

To distinguish **interactive** and **read-only**, the server serves a modified `vnc.html` page:

| Mode | Mechanism |
|------|-----------|
| **Interactive** | Injected script: `localStorage.view_only = false`, "View Only" checkbox unchecked on load |
| **Read-only** | noVNC Settings panel hidden (CSS), `localStorage.view_only = true`, checkbox checked + `disabled` + revert on `change` |

On **watch** links, mode is derived from `watch.mode` in the database (`interactive` vs `read_only`) — the client `bunny_lock` query param is **not** the source of truth for `/watch/:token/vnc/vnc.html`.

This fixes observed user issues:

1. **Interactive blocked** — noVNC persists `view_only` in `localStorage`; a previous read-only session left "View Only" checked.
2. **Read-only bypassable** — a viewer could open noVNC Settings and uncheck "View Only".

---

## Security limitation (why an improvement is needed)

v1 locking is **noVNC client-side only** (injected HTML/JS + hidden UI). It does not control what travels over the VNC WebSocket.

A determined user can bypass v1 by:

- modifying `localStorage` or the DOM via devtools;
- loading another noVNC page (unlocked static files) pointing at the same WebSocket;
- sending RFB frames (pointer / keyboard) with a custom VNC client, as long as they have the WebSocket URL and a valid watch token.

Today, the WebSocket proxy (`apps/server/src/ws.rs`, `handle_novnc_proxy`) relays **all** client → upstream messages without RFB protocol inspection:

```text
noVNC (browser)  ↔  bunny-server (proxy)  ↔  websockify  ↔  x11vnc  ↔  Chromium
```

x11vnc starts in **shared** mode (`-shared`, without `-viewonly`) so both Web UI control (Interactive tab) and read-only streaming work on the **same** browser stack per session.

**v1 trust model:** read-only = reasonable trust for a casual viewer; **not** a cryptographic or protocol barrier against an attacker with the watch link and technical skills.

---

## Improvement goal

Ensure **read-only** mode transmits **no** pointer/keyboard events to the desktop, regardless of noVNC client or `localStorage`, while keeping **interactive** mode for `interactive:true` links and the authenticated Interactive tab.

---

## Implementation options

### 1. RFB filtering in bunny-server WebSocket proxy (recommended)

Intercept the **client → upstream** stream in `handle_novnc_proxy` (or watch / browser variant with mode context).

- Parse binary RFB frames (type 5 PointerEvent, 4 KeyEvent, etc.).
- In **read-only** context: drop input messages; pass framebuffer / encodings / keepalive.
- In **interactive** context: relay without filter.

**Pros:** single x11vnc stack per session; enforcement independent of client.  
**Cons:** maintain a minimal RFB parser; test across noVNC encodings / versions.

Route parameters:

- Watch: mode from `watch.mode` (already resolved on HTTP).
- Authenticated browser: explicit flag on `/browser-sessions/:id/vnc/ws` (Stream = read-only, Interactive = full).

### 2. Two x11vnc instances or `-viewonly` toggle

- **Option A:** second read-only VNC port with `x11vnc -viewonly` for watch / Stream; interactive port without `-viewonly` for the editor UI.
- **Option B:** restart or reconfigure x11vnc on mode change (more fragile, latency).

**Pros:** enforcement at VNC server level, no RFB parser in bunny.  
**Cons:** stack complexity, extra ports, lifecycle coordination.

### 3. Restrict access to unlocked noVNC assets

Serve **only** locked `vnc.html` on public watch routes; deny or do not expose other noVNC static files without auth.

Reduces "alternate noVNC page" bypass, but **not sufficient** alone (custom VNC client + WS).

### 4. Separate watch token capabilities

JWT or claims on watch token: `capabilities: ["view"]` vs `["view", "input"]`. WS proxy refuses input frames when `input` is absent — same approach as (1), with explicit auth model.

---

## Acceptance criteria (future)

1. Watch link **without** `interactive:true`: mouse click, scroll, keyboard **have no effect** on Chromium, even after DOM / localStorage / alternate client manipulation.
2. Watch link **`interactive:true`**: full interaction without regression.
3. Web UI: **Stream** tab read-only locked; **Interactive** tab unchanged.
4. Automated or documented manual tests: at minimum an RFB checklist (pointer + key dropped in read-only).

---

## Files involved (future implementation)

| File | Role |
|------|------|
| `apps/server/src/ws.rs` | Bidirectional WebSocket proxy — RFB filter injection point |
| `apps/server/src/watch.rs` | Resolve `watch.mode` → read-only context on WS |
| `apps/server/src/novnc_proxy.rs` | HTML v1 lock (can remain as UI defense in depth) |
| `apps/server/src/api.rs` | `browser_novnc_ws` route — distinguish Stream vs Interactive |
| `crates/bunny-browser/src/stack.rs` | Optional: second VNC / `-viewonly` |
| Discord setup | Update [Discord setup](../team-chats/discord/setup) once server enforcement ships |

---

## References

- noVNC `view_only`: **client** option; does not secure the protocol.
- RFB 3.8: [RFC 6143](https://www.rfc-editor.org/rfc/rfc6143) — PointerEvent (5), KeyEvent (4) message types.
- Internal issue: noVNC Settings bypass reported during manual Discord watch testing (2025).
