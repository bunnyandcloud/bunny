import { useEffect, useState } from 'react';
import { createPreview, listPreviews, previewUrl } from '../lib/api';

interface Props {
  sessionId: string;
  defaultPort?: number;
  onPortChange?: (port: number) => void;
}

export default function PreviewPanel({ sessionId, defaultPort = 3000, onPortChange }: Props) {
  const [port, setPort] = useState(defaultPort);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      setError(null);
      setReady(false);
      try {
        const existing = await listPreviews();
        const match = existing.find((p) => p.public_path.includes(`/ports/${port}/`));
        if (!match) {
          await createPreview(sessionId, port);
        }
        if (!cancelled) setReady(true);
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [sessionId, port]);

  useEffect(() => {
    onPortChange?.(port);
  }, [port, onPortChange]);

  useEffect(() => {
    setPort(defaultPort);
  }, [defaultPort]);

  const src = previewUrl(sessionId, port);

  return (
    <div className="h-full flex flex-col bg-bunny-bg">
      <div className="flex items-center gap-2 px-2 py-1.5 border-b border-bunny-border bg-bunny-panel text-xs shrink-0">
        <span className="text-bunny-muted">Port</span>
        <input
          type="number"
          min={1}
          max={65535}
          value={port}
          onChange={(e) => setPort(Number(e.target.value) || defaultPort)}
          className="w-20 px-1.5 py-0.5 rounded bg-bunny-bg border border-bunny-border text-gray-200"
        />
        <a
          href={src}
          target="_blank"
          rel="noreferrer"
          className="text-bunny-accent hover:underline ml-auto"
        >
          Ouvrir dans un nouvel onglet
        </a>
      </div>
      <div className="flex-1 min-h-0 relative">
        {error && (
          <div className="absolute inset-0 flex items-center justify-center p-4 text-sm text-red-400">
            {error}
          </div>
        )}
        {!error && !ready && (
          <div className="absolute inset-0 flex items-center justify-center text-bunny-muted text-sm">
            Préparation de la preview…
          </div>
        )}
        {ready && !error && (
          <iframe
            key={`${sessionId}-${port}`}
            title={`Preview port ${port}`}
            src={src}
            className="absolute inset-0 w-full h-full border-0 bg-white"
            sandbox="allow-scripts allow-same-origin allow-forms allow-popups allow-modals"
          />
        )}
      </div>
      <div className="px-2 py-1.5 text-[11px] text-bunny-muted border-t border-bunny-border shrink-0 leading-snug space-y-1">
        <p>
          <strong className="text-gray-300">Preview</strong> — ton app sur le port {port}, via le
          proxy Bunny (<code className="text-gray-400">{location.host}</code>).
        </p>
        <p className="text-amber-200/90">
          Dev server (Next, Vite, etc.) : autorise l’origine{' '}
          <code className="text-gray-400">{location.host}</code> si ton framework bloque le HMR
          cross-origin.
        </p>
      </div>
    </div>
  );
}
