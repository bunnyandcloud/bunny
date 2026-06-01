import { useEffect, useState } from 'react';
import { apiErrorMessage, getWatchMeta, grantWatchAccess, watchNovncUrl } from '../lib/api';

export default function WatchPage({ token }: { token: string }) {
  const [meta, setMeta] = useState<Awaited<ReturnType<typeof getWatchMeta>> | null>(null);
  const [error, setError] = useState('');
  const [browserId, setBrowserId] = useState<string | null>(null);

  const interactive = meta?.mode === 'interactive';

  useEffect(() => {
    let cancelled = false;

    const load = async () => {
      try {
        const m = await getWatchMeta(token);
        if (cancelled) return;
        setMeta(m);
        setBrowserId(m.browser_ids[0] ?? null);
        setError('');
        await grantWatchAccess(token, {});
      } catch (e) {
        if (!cancelled) setError(apiErrorMessage(e, 'Watch stream ended or unavailable'));
      }
    };

    void load();
    const poll = window.setInterval(() => {
      void load();
    }, 2000);

    return () => {
      cancelled = true;
      window.clearInterval(poll);
    };
  }, [token]);

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
      <header className="border-b border-bunny-border px-4 py-2 flex items-center justify-between text-sm shrink-0">
        <span>
          Bunny watch · layout <strong>{meta.layout}</strong> ·{' '}
          {interactive ? (
            <strong className="text-amber-300">interactive</strong>
          ) : (
            'read-only'
          )}
        </span>
        <span className="text-bunny-muted text-xs">
          expires {new Date(meta.expires_at).toLocaleString()}
        </span>
      </header>
      <main className="flex-1 min-h-0 relative bg-black">
        {browserId ? (
          <iframe
            title="Bunny watch"
            src={watchNovncUrl(token, { interactive })}
            className="absolute inset-0 w-full h-full border-0"
          />
        ) : (
          <p className="absolute inset-0 flex items-center justify-center text-bunny-muted">
            No browser stream in this session.
          </p>
        )}
      </main>
    </div>
  );
}
