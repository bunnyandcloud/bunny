import { useEffect, useState } from 'react';
import { apiErrorMessage, getWatchMeta, grantWatchAccess, watchNovncUrl } from '../lib/api';
import LanguageSelect from './LanguageSelect';
import { effectiveLocale, guessBrowserLocale, readStoredLocale, t } from '../i18n';

export default function WatchPage({ token }: { token: string }) {
  const locale = effectiveLocale(readStoredLocale() ?? guessBrowserLocale());
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
        if (!cancelled) setError(apiErrorMessage(e, t(locale, 'web.watch.unavailable')));
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
  }, [token, locale]);

  if (error) {
    return (
      <div className="min-h-screen flex flex-col">
        <div className="absolute top-4 right-4 z-10">
          <LanguageSelect />
        </div>
        <div className="flex-1 flex items-center justify-center text-red-400 p-6">{error}</div>
      </div>
    );
  }

  if (!meta) {
    return (
      <div className="min-h-screen flex items-center justify-center text-bunny-muted">
        {t(locale, 'web.common.loadingWatch')}
      </div>
    );
  }

  const modeLabel = interactive
    ? t(locale, 'web.watch.interactive')
    : t(locale, 'web.watch.readOnly');

  return (
    <div className="min-h-screen bg-bunny-bg text-gray-200 flex flex-col">
      <header className="border-b border-bunny-border px-4 py-2 flex items-center justify-between text-sm shrink-0 gap-2">
        <span>
          {t(locale, 'web.watch.header', { layout: meta.layout, mode: modeLabel })}
        </span>
        <div className="flex items-center gap-2 shrink-0">
          <span className="text-bunny-muted text-xs">
            {t(locale, 'web.watch.expires', {
              date: new Date(meta.expires_at).toLocaleString(),
            })}
          </span>
          <LanguageSelect />
        </div>
      </header>
      <main className="flex-1 min-h-0 relative bg-black">
        {browserId ? (
          <iframe
            title={t(locale, 'web.watch.iframeTitle')}
            src={watchNovncUrl(token, { interactive })}
            className="absolute inset-0 w-full h-full border-0"
          />
        ) : (
          <p className="absolute inset-0 flex items-center justify-center text-bunny-muted">
            {t(locale, 'web.watch.noBrowser')}
          </p>
        )}
      </main>
    </div>
  );
}
