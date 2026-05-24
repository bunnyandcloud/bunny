/**
 * CDP screencast → WebRTC video track (browser peer per browserId).
 */
import { chromium } from 'playwright';
import sharp from 'sharp';
import wrtc from '@roamhq/wrtc';

const { RTCVideoSource, rgbaToI420 } = wrtc.nonstandard;

/** @type {Map<string, { pc, source, track, cdpSession?, browser? }>} */
export const browserPeers = new Map();

function emit(obj) {
  process.stdout.write(`${JSON.stringify(obj)}\n`);
}

function feedRgbaFrame(source, rgba, width, height) {
  const i420 = new Uint8ClampedArray(Math.floor((width * height * 3) / 2));
  rgbaToI420(
    { width, height, data: rgba },
    { width, height, data: i420 },
  );
  source.onFrame({ width, height, data: i420 });
}

export function getBrowserPeer(browserId, iceServers) {
  if (!browserPeers.has(browserId)) {
    const source = new RTCVideoSource();
    const track = source.createTrack();
    const pc = new wrtc.RTCPeerConnection({ iceServers });
    pc.addTrack(track);
    pc.onicecandidate = (ev) => {
      if (ev.candidate) {
        emit({
          type: 'webrtc.browser.ice',
          browserId,
          candidate: ev.candidate.toJSON(),
        });
      }
    };
    browserPeers.set(browserId, {
      pc,
      source,
      track,
      cdpSession: null,
      browser: null,
    });
  }
  return browserPeers.get(browserId);
}

export async function startBrowserScreencast(browserId, cdpPort) {
  const entry = browserPeers.get(browserId);
  if (!entry || entry.cdpSession) return;

  const endpoint = `http://127.0.0.1:${cdpPort}`;
  const browser = await chromium.connectOverCDP(endpoint);
  const context = browser.contexts()[0] ?? (await browser.newContext());
  const page = context.pages()[0] ?? (await context.newPage());
  const cdp = await context.newCDPSession(page);

  await cdp.send('Page.startScreencast', {
    format: 'jpeg',
    quality: 75,
    maxWidth: 1280,
    maxHeight: 720,
    everyNthFrame: 1,
  });

  cdp.on('Page.screencastFrame', async (frame) => {
    try {
      await cdp.send('Page.screencastFrameAck', { sessionId: frame.sessionId });
      const buf = Buffer.from(frame.data, 'base64');
      const { data, info } = await sharp(buf)
        .ensureAlpha()
        .raw()
        .toBuffer({ resolveWithObject: true });
      const peer = browserPeers.get(browserId);
      if (peer?.source) {
        feedRgbaFrame(
          peer.source,
          new Uint8ClampedArray(data),
          info.width,
          info.height,
        );
      }
    } catch (err) {
      emit({ type: 'webrtc.browser.error', browserId, error: String(err) });
    }
  });

  entry.browser = browser;
  entry.cdpSession = cdp;
  emit({ type: 'webrtc.browser.screencast', browserId, status: 'started' });
}

export async function stopBrowserPeer(browserId) {
  const entry = browserPeers.get(browserId);
  if (!entry) return;
  try {
    if (entry.cdpSession) {
      await entry.cdpSession.send('Page.stopScreencast').catch(() => {});
    }
    if (entry.browser) {
      await entry.browser.close().catch(() => {});
    }
    entry.pc.close();
  } catch (_) {}
  browserPeers.delete(browserId);
  emit({ type: 'webrtc.browser.stopped', browserId });
}
