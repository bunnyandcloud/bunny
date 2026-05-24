#!/usr/bin/env node
/**
 * One-shot: read Claude OAuth code from all Chromium tabs (URL + page text).
 * Usage: node extract-oauth-code.js http://127.0.0.1:9222
 */
import { chromium } from 'playwright';

const cdpUrl = process.argv[2] || process.env.BUNNY_CDP_URL;
if (!cdpUrl) {
  console.log(JSON.stringify({ code: null, error: 'missing cdp url' }));
  process.exit(0);
}

/** Full paste format shown on Claude's "Authentication Code" page. */
const PAGE_CODE_RE = /[A-Za-z0-9]{20,}#[A-Za-z0-9_-]+/;

function normalize(raw) {
  const trimmed = String(raw || '').trim();
  if (!trimmed || trimmed === 'true' || trimmed === 'false') return null;
  if (trimmed.length < 20 || trimmed.length > 256) return null;
  if (!/^[A-Za-z0-9_#-]+$/.test(trimmed)) return null;
  return trimmed;
}

function codeFromUrl(url) {
  if (!url.includes('oauth/code/callback')) return null;
  try {
    const u = new URL(url);
    const code = u.searchParams.get('code');
    return normalize(code);
  } catch {
    return null;
  }
}

function codeFromText(text) {
  if (!text) return null;
  const m = text.match(PAGE_CODE_RE);
  return m ? normalize(m[0]) : null;
}

function pickBest(candidates) {
  const withHash = candidates.find((c) => c.includes('#'));
  if (withHash) return withHash;
  return candidates[0] ?? null;
}

async function main() {
  const browser = await chromium.connectOverCDP(cdpUrl);
  const candidates = [];
  for (const ctx of browser.contexts()) {
    for (const page of ctx.pages()) {
      try {
        const text = await page.evaluate(() => document.body?.innerText || '');
        const fromText = codeFromText(text);
        if (fromText) candidates.push(fromText);
      } catch {
        /* page may be navigating */
      }
      const fromUrl = codeFromUrl(page.url());
      if (fromUrl) candidates.push(fromUrl);
    }
  }
  await browser.close().catch(() => {});
  console.log(JSON.stringify({ code: pickBest(candidates) }));
}

main().catch((e) => {
  console.log(JSON.stringify({ code: null, error: String(e) }));
  process.exit(0);
});
