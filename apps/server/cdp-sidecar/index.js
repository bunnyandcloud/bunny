#!/usr/bin/env node
/**
 * CDP collector sidecar — attaches to Chromium via CDP on 127.0.0.1 only.
 * Emits redacted console/network events as JSON lines on stdout.
 */
import { chromium } from 'playwright';
import readline from 'readline';

const cdpPort = process.env.BUNNY_CDP_PORT || '9222';
const cdpUrl = process.env.BUNNY_CDP_URL || `http://127.0.0.1:${cdpPort}`;
const sessionId = process.env.BUNNY_SESSION_ID || 'unknown';

function redactUrl(url) {
  try {
    const u = new URL(url);
    return `${u.origin}${u.pathname}`;
  } catch {
    return '[invalid-url]';
  }
}

function emit(event) {
  console.log(JSON.stringify({ sessionId, ts: new Date().toISOString(), ...event }));
}

async function main() {
  const browser = await chromium.connectOverCDP(cdpUrl);
  const context = browser.contexts()[0] || (await browser.newContext());
  const page = context.pages()[0] || (await context.newPage());

  page.on('console', (msg) => {
    emit({
      type: 'browser.console',
      level: msg.type(),
      text: msg.text().slice(0, 2000),
      url: page.url() ? redactUrl(page.url()) : null,
    });
  });

  page.on('pageerror', (err) => {
    emit({ type: 'browser.pageerror', message: String(err).slice(0, 2000) });
  });

  page.on('request', (req) => {
    emit({
      type: 'browser.network',
      phase: 'started',
      requestId: req.url().slice(0, 64),
      method: req.method(),
      urlRedacted: redactUrl(req.url()),
      resourceType: req.resourceType(),
    });
  });

  page.on('response', (res) => {
    emit({
      type: 'browser.network',
      phase: 'completed',
      requestId: res.url().slice(0, 64),
      status: res.status(),
      urlRedacted: redactUrl(res.url()),
    });
  });

  page.on('requestfailed', (req) => {
    emit({
      type: 'browser.network',
      phase: 'failed',
      requestId: req.url().slice(0, 64),
      urlRedacted: redactUrl(req.url()),
      error: req.failure()?.errorText || 'failed',
    });
  });

  emit({ type: 'collector.ready', cdpUrl });

  const rl = readline.createInterface({ input: process.stdin });
  rl.on('line', async (line) => {
    try {
      const cmd = JSON.parse(line);
      if (cmd.type === 'screenshot') {
        const buf = await page.screenshot({ type: 'png' });
        emit({ type: 'browser.screenshot', refId: cmd.refId, size: buf.length });
      }
    } catch (e) {
      emit({ type: 'collector.error', message: String(e) });
    }
  });
}

main().catch((e) => {
  emit({ type: 'collector.crashed', message: String(e) });
  process.exit(1);
});
