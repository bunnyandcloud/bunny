/**
 * WebRTC sidecar: session data channels + browser video screencast (CDP).
 */
import http from 'http';
import wrtc from '@roamhq/wrtc';
import {
  browserPeers,
  getBrowserPeer,
  startBrowserScreencast,
  stopBrowserPeer,
} from './browser-stream.js';

const port = Number(process.env.BUNNY_WEBRTC_PORT || 18782);
const stun = process.env.BUNNY_STUN_URLS
  ? process.env.BUNNY_STUN_URLS.split(',').map((u) => ({ urls: u.trim() }))
  : [{ urls: 'stun:stun.l.google.com:19302' }];

if (process.env.BUNNY_TURN_URL) {
  stun.push({
    urls: process.env.BUNNY_TURN_URL,
    username: process.env.BUNNY_TURN_USERNAME || undefined,
    credential: process.env.BUNNY_TURN_CREDENTIAL || undefined,
  });
}

/** @type {Map<string, { pc: RTCPeerConnection, dc?: RTCDataChannel }>} */
const sessions = new Map();

function emit(obj) {
  process.stdout.write(`${JSON.stringify(obj)}\n`);
}

function wireDataChannel(sessionId, dc) {
  dc.onopen = () => emit({ type: 'webrtc.dc.open', sessionId });
  dc.onmessage = (ev) => {
    emit({
      type: 'webrtc.message',
      sessionId,
      data: typeof ev.data === 'string' ? ev.data : String(ev.data),
    });
  };
  dc.onclose = () => emit({ type: 'webrtc.dc.close', sessionId });
}

function getSession(sessionId) {
  if (!sessions.has(sessionId)) {
    const pc = new wrtc.RTCPeerConnection({ iceServers: stun });
    pc.onicecandidate = (ev) => {
      if (ev.candidate) {
        emit({
          type: 'webrtc.ice',
          sessionId,
          candidate: ev.candidate.toJSON(),
        });
      }
    };
    pc.ondatachannel = (ev) => {
      const entry = sessions.get(sessionId);
      if (entry) {
        entry.dc = ev.channel;
        wireDataChannel(sessionId, ev.channel);
      }
    };
    sessions.set(sessionId, { pc });
  }
  return sessions.get(sessionId);
}

async function readBody(req) {
  const chunks = [];
  for await (const c of req) chunks.push(c);
  return Buffer.concat(chunks).toString('utf8');
}

function json(res, code, body) {
  res.writeHead(code, { 'Content-Type': 'application/json' });
  res.end(JSON.stringify(body));
}

const server = http.createServer(async (req, res) => {
  try {
    const url = new URL(req.url || '/', `http://127.0.0.1:${port}`);
    const parts = url.pathname.split('/').filter(Boolean);

    if (req.method === 'GET' && parts[0] === 'health') {
      return json(res, 200, { ok: true });
    }

    if (parts[0] !== 'v1') {
      return json(res, 404, { error: 'not found' });
    }

    const body = req.method === 'POST' ? JSON.parse(await readBody(req)) : {};

    // Browser video: /v1/browser-sessions/:browserId/offer|candidate|stop
    if (parts[1] === 'browser-sessions' && parts[2]) {
      const browserId = parts[2];
      const action = parts[3];
      const entry = getBrowserPeer(browserId, stun);
      const { pc } = entry;

      if (action === 'offer' && req.method === 'POST') {
        const cdpPort = body.cdpPort;
        if (cdpPort) {
          startBrowserScreencast(browserId, cdpPort).catch((e) => {
            emit({ type: 'webrtc.browser.error', browserId, error: String(e) });
          });
        }
        await pc.setRemoteDescription(body);
        const answer = await pc.createAnswer();
        await pc.setLocalDescription(answer);
        return json(res, 200, { type: answer.type, sdp: answer.sdp });
      }

      if (action === 'candidate' && req.method === 'POST') {
        if (body.candidate) await pc.addIceCandidate(body.candidate);
        return json(res, 200, { ok: true });
      }

      if (action === 'stop' && req.method === 'POST') {
        await stopBrowserPeer(browserId);
        return json(res, 200, { ok: true });
      }

      return json(res, 404, { error: 'unknown browser action' });
    }

    // Session data channel: /v1/sessions/:sessionId/...
    if (parts[1] !== 'sessions' || !parts[2]) {
      return json(res, 404, { error: 'not found' });
    }

    const sessionId = parts[2];
    const action = parts[3];
    const entry = getSession(sessionId);
    const { pc } = entry;

    if (action === 'offer' && req.method === 'POST') {
      await pc.setRemoteDescription(body);
      const answer = await pc.createAnswer();
      await pc.setLocalDescription(answer);
      return json(res, 200, { type: answer.type, sdp: answer.sdp });
    }

    if (action === 'candidate' && req.method === 'POST') {
      if (body.candidate) await pc.addIceCandidate(body.candidate);
      return json(res, 200, { ok: true });
    }

    if (action === 'message' && req.method === 'POST') {
      const dc = entry.dc;
      if (!dc || dc.readyState !== 'open') {
        return json(res, 409, { error: 'data channel not open' });
      }
      dc.send(body.data ?? '');
      return json(res, 200, { ok: true });
    }

    if (action === 'close' && req.method === 'POST') {
      pc.close();
      sessions.delete(sessionId);
      return json(res, 200, { ok: true });
    }

    return json(res, 404, { error: 'unknown action' });
  } catch (e) {
    json(res, 500, { error: String(e) });
  }
});

server.listen(port, '127.0.0.1', () => {
  process.stderr.write(`webrtc-sidecar listening on 127.0.0.1:${port}\n`);
});
