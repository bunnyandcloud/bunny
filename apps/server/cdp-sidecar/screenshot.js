#!/usr/bin/env node
import http from 'http';
import WebSocket from 'ws';

const port = process.env.BUNNY_CDP_PORT || '9222';

function get(path) {
  return new Promise((resolve, reject) => {
    http
      .get(`http://127.0.0.1:${port}${path}`, (res) => {
        let data = '';
        res.on('data', (c) => (data += c));
        res.on('end', () => resolve(JSON.parse(data)));
      })
      .on('error', reject);
  });
}

function cdp(wsUrl, method, params = {}) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(wsUrl);
    const id = 1;
    ws.on('open', () => {
      ws.send(JSON.stringify({ id, method, params }));
    });
    ws.on('message', (raw) => {
      const msg = JSON.parse(raw.toString());
      if (msg.id === id) {
        ws.close();
        if (msg.error) reject(new Error(msg.error.message));
        else resolve(msg.result);
      }
    });
    ws.on('error', reject);
  });
}

const list = await get('/json/list');
const page = list.find((t) => t.type === 'page') || list[0];
if (!page?.webSocketDebuggerUrl) {
  process.exit(2);
}
await cdp(page.webSocketDebuggerUrl, 'Page.enable');
const result = await cdp(page.webSocketDebuggerUrl, 'Page.captureScreenshot', {
  format: 'png',
});
const buf = Buffer.from(result.data, 'base64');
process.stdout.write(buf);
