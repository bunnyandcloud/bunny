import { useCallback, useEffect, useRef, useState } from 'react';
import {
  browserNovncUrl,
  createBrowser,
  getBrowser,
  restartBrowser,
} from '../lib/api';

interface Props {
  sessionId: string;
  targetPort?: number;
}

type ViewMode = 'novnc' | 'stream';

function browserStorageKey(sessionId: string) {
  return `bunny-browser-id:${sessionId}`;
}

/** Poll until websockify accepts TCP (server sets novncReady). */
async function waitForBrowserNovncReady(browserId: string, timeoutMs = 20_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const info = await getBrowser(browserId);
    if (info.novncPort != null && info.novncReady) {
      return;
    }
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error('Le navigateur distant met trop de temps à démarrer — clique Recharger.');
}

export default function BrowserPanel({ sessionId, targetPort = 3000 }: Props) {
  const [browserId, setBrowserId] = useState<string | null>(null);
  const [mode, setMode] = useState<ViewMode>('novnc');
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [reloadKey, setReloadKey] = useState(0);
  const targetUrl = `http://127.0.0.1:${targetPort}`;
  const initStarted = useRef(false);

  const showBrowser = useCallback((id: string) => {
    setBrowserId(id);
    sessionStorage.setItem(browserStorageKey(sessionId), id);
    setReloadKey((k) => k + 1);
  }, [sessionId]);

  const startBrowser = useCallback(async () => {
    setStarting(true);
    setError(null);
    try {
      const created = await createBrowser(sessionId, targetUrl);
      await waitForBrowserNovncReady(created.id);
      showBrowser(created.id);
    } catch (e) {
      setError(String(e));
    } finally {
      setStarting(false);
    }
  }, [sessionId, targetUrl, showBrowser]);

  const ensureBrowser = useCallback(async () => {
    setStarting(true);
    setError(null);
    const stored = sessionStorage.getItem(browserStorageKey(sessionId));
    if (stored) {
      try {
        const info = await getBrowser(stored);
        if (info.novncPort != null && info.novncReady) {
          showBrowser(stored);
          setStarting(false);
          return;
        }
        await waitForBrowserNovncReady(stored);
        showBrowser(stored);
        setStarting(false);
        return;
      } catch {
        sessionStorage.removeItem(browserStorageKey(sessionId));
      }
    }
    await startBrowser();
  }, [sessionId, startBrowser, showBrowser]);

  const reloadBrowser = useCallback(async () => {
    if (!browserId) {
      await ensureBrowser();
      return;
    }
    setStarting(true);
    setError(null);
    try {
      await restartBrowser(browserId, sessionId, targetUrl);
      await waitForBrowserNovncReady(browserId);
      setReloadKey((k) => k + 1);
    } catch (e) {
      setError(String(e));
    } finally {
      setStarting(false);
    }
  }, [browserId, sessionId, targetUrl, ensureBrowser]);

  useEffect(() => {
    if (initStarted.current) return;
    initStarted.current = true;
    void ensureBrowser();
  }, [ensureBrowser]);

  const prevPort = useRef(targetPort);
  useEffect(() => {
    if (prevPort.current === targetPort) return;
    prevPort.current = targetPort;
    if (browserId) void reloadBrowser();
  }, [targetPort, browserId, reloadBrowser]);

  return (
    <div className="h-full flex flex-col bg-bunny-bg">
      <div className="flex items-center gap-2 px-2 py-1.5 border-b border-bunny-border bg-bunny-panel text-xs shrink-0 flex-wrap">
        <span className="text-bunny-muted">URL</span>
        <code className="text-gray-300">{targetUrl}</code>
        <div className="ml-auto flex gap-1 flex-wrap">
          <button
            type="button"
            onClick={() => void reloadBrowser()}
            disabled={starting}
            className="px-2 py-0.5 rounded border border-bunny-border text-bunny-muted hover:text-gray-200 disabled:opacity-50"
          >
            Recharger
          </button>
          <button
            type="button"
            onClick={() => setMode('novnc')}
            className={`px-2 py-0.5 rounded border ${
              mode === 'novnc'
                ? 'border-bunny-accent text-bunny-accent bg-bunny-accent/10'
                : 'border-bunny-border text-bunny-muted hover:text-gray-200'
            }`}
          >
            Interactif
          </button>
          <button
            type="button"
            onClick={() => setMode('stream')}
            className={`px-2 py-0.5 rounded border ${
              mode === 'stream'
                ? 'border-bunny-accent text-bunny-accent bg-bunny-accent/10'
                : 'border-bunny-border text-bunny-muted hover:text-gray-200'
            }`}
          >
            Stream
          </button>
        </div>
      </div>

      <div className="flex-1 min-h-0 relative bg-black">
        {error && (
          <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 p-6 text-center">
            <p className="text-sm text-red-400 max-w-md">{error}</p>
            <p className="text-xs text-bunny-muted max-w-md">
              Vérifie que ton serveur écoute sur le port {targetPort}, puis clique Recharger.
            </p>
            <button
              type="button"
              onClick={() => void reloadBrowser()}
              disabled={starting}
              className="text-xs px-3 py-1.5 border border-bunny-border rounded hover:bg-bunny-panel disabled:opacity-50"
            >
              Réessayer
            </button>
          </div>
        )}

        {!error && (starting || !browserId) && (
          <div className="absolute inset-0 flex flex-col items-center justify-center gap-2 text-bunny-muted text-sm px-6 text-center">
            <p>Démarrage de Chromium sur le serveur…</p>
            <p className="text-xs text-bunny-muted/80">
              Premier lancement : quelques secondes (Xvfb + noVNC, puis Chromium). Les
              onglets suivants réutilisent le même navigateur.
            </p>
          </div>
        )}

        {!error && browserId && mode === 'novnc' && (
          <iframe
            key={reloadKey}
            title="Navigateur distant (noVNC)"
            src={browserNovncUrl(browserId)}
            className="absolute inset-0 w-full h-full border-0"
          />
        )}

        {!error && browserId && mode === 'stream' && (
          <iframe
            key={`stream-${reloadKey}`}
            title="Navigateur distant (stream read-only)"
            src={browserNovncUrl(browserId, { viewOnly: true })}
            className="absolute inset-0 w-full h-full border-0"
          />
        )}
      </div>

      <div className="px-2 py-1.5 text-[11px] text-bunny-muted border-t border-bunny-border shrink-0 leading-snug space-y-1">
        <p>
          <strong className="text-gray-300">Browser</strong> lance Chromium <em>sur le serveur</em>{' '}
          (dans Docker) et affiche son écran via noVNC — utile pour voir exactement ce que le
          serveur rend.
        </p>
        <p>
          « Échec de connexion » au premier essai ? Attends la fin du démarrage ou clique{' '}
          <strong className="text-gray-300">Recharger</strong> une fois. Pour le dev quotidien,
          l’onglet <strong className="text-gray-300">Preview</strong> est plus instantané (même
          port).
        </p>
      </div>
    </div>
  );
}
