import { useEffect, useState } from 'react';
import { apiErrorMessage, getWatchMeta, grantWatchAccess } from '../lib/api';
import { useBrowserWebRtc } from '../lib/useBrowserWebRtc';

export default function WatchPage({ token }: { token: string }) {
  const [meta, setMeta] = useState<Awaited<ReturnType<typeof getWatchMeta>> | null>(null);
  const [error, setError] = useState('');
  const [browserId, setBrowserId] = useState<string | null>(null);
  const [sessionId, setSessionId] = useState<string | null>(null);

  const webrtc = useBrowserWebRtc(sessionId ?? '', browserId);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const m = await getWatchMeta(token);
        if (cancelled) return;
        setMeta(m);
        setSessionId(m.session_id);
        setBrowserId(m.browser_ids[0] ?? null);
        await grantWatchAccess(token, {});
      } catch (e) {
        if (!cancelled) setError(apiErrorMessage(e, 'Cannot open watch session'));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [token]);

  useEffect(() => {
    if (!browserId || !sessionId) return;
    void webrtc.connect();
    return () => {
      void webrtc.disconnect();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- connect when ids set
  }, [browserId, sessionId]);

  if (error) {
    return (
      <div className="min-h-screen flex items-center justify-center text-red-400 p-6">
        {error}
      </div>
    );
  }

  if (!meta) {
    return (
      <div className="min-h-screen flex items-center justify-center text-bunny-muted">
        Loading watch…
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-bunny-bg text-gray-200 flex flex-col">
      <header className="border-b border-bunny-border px-4 py-2 flex items-center justify-between text-sm">
        <span>
          Bunny watch · layout <strong>{meta.layout}</strong> · read-only
        </span>
        <span className="text-bunny-muted text-xs">
          expires {new Date(meta.expires_at).toLocaleString()}
        </span>
      </header>
      <main className="flex-1 flex items-center justify-center p-4">
        {browserId ? (
          <video
            ref={webrtc.videoRef}
            autoPlay
            playsInline
            muted
            className="max-w-full max-h-[80vh] rounded border border-bunny-border bg-black"
          />
        ) : (
          <p className="text-bunny-muted">No browser stream in this session.</p>
        )}
      </main>
    </div>
  );
}
