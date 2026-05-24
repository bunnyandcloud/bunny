import { useCallback, useEffect, useState } from 'react';
import {
  browserNavigate,
  createBrowser,
  getClaudeStatus,
  installClaude,
  detectClaudeAuthCode,
  submitClaudeAuthCode,
  type ClaudeStatus,
} from '../lib/api';

interface Props {
  sessionId: string;
  browserId: string | null;
  onBrowserId: (id: string) => void;
  onOpenBrowserTab: () => void;
  onOpenTerminalTab: (terminalId?: string) => void;
}

export default function ClaudeSetupPanel({
  sessionId,
  browserId,
  onBrowserId,
  onOpenBrowserTab,
  onOpenTerminalTab,
}: Props) {
  const [status, setStatus] = useState<ClaudeStatus | null>(null);
  const [installing, setInstalling] = useState(false);
  const [code, setCode] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [feedback, setFeedback] = useState<string | null>(null);
  const [browserOpening, setBrowserOpening] = useState(false);
  const [importing, setImporting] = useState(false);
  const [signInOpened, setSignInOpened] = useState(false);

  const refresh = useCallback(() => {
    getClaudeStatus()
      .then(setStatus)
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    refresh();
    const t = setInterval(refresh, 1500);
    return () => clearInterval(t);
  }, [refresh]);

  const tryImportFromBrowser = useCallback(
    async (opts?: { quiet?: boolean }) => {
      if (!browserId || status?.authenticated) return false;
      setImporting(true);
      if (!opts?.quiet) setError(null);
      try {
        const result = await detectClaudeAuthCode(browserId);
        if (result.found && result.submitted) {
          const hint = result.code_hint ? ` (${result.code_hint})` : '';
          setFeedback(
            `Code${hint} sent to the Claude login shell on the server — do not paste from your Mac clipboard.`,
          );
          onOpenTerminalTab(status?.auth.terminal_id ?? undefined);
          refresh();
          return true;
        }
        if (!result.found && !opts?.quiet) {
          setError(
            'Code not found — stay on the Authentication Code page until you see the full code (with a # in the middle), then click Import.',
          );
        }
        return false;
      } catch (e) {
        if (!opts?.quiet) setError(String(e));
        return false;
      } finally {
        setImporting(false);
      }
    },
    [browserId, status?.authenticated, status?.auth.terminal_id, refresh, onOpenTerminalTab],
  );

  async function handleInstall() {
    setInstalling(true);
    setError(null);
    try {
      await installClaude();
      refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setInstalling(false);
    }
  }

  async function openOAuthInBrowser(browserUrl: string) {
    setBrowserOpening(true);
    setError(null);
    try {
      let bid = browserId;
      if (!bid) {
        const created = await createBrowser(sessionId, browserUrl);
        bid = created.id;
        onBrowserId(bid);
      } else {
        await browserNavigate(bid, browserUrl);
      }
      onOpenBrowserTab();
      setSignInOpened(true);
    } catch (e) {
      setError(String(e));
    } finally {
      setBrowserOpening(false);
    }
  }

  async function handleSubmitCode(e: React.FormEvent) {
    e.preventDefault();
    if (!code.trim()) return;
    setBusy(true);
    setError(null);
    try {
      await submitClaudeAuthCode(code.trim());
      setCode('');
      setFeedback(
        'Code sent to the Claude login shell on the server — do not paste from your Mac clipboard.',
      );
      onOpenTerminalTab(status?.auth.terminal_id ?? undefined);
      refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  if (!status) {
    return (
      <div className="rounded border border-bunny-border bg-bunny-panel p-3 text-sm text-bunny-muted">
        Loading Claude setup…
      </div>
    );
  }

  if (status.authenticated) {
    return (
      <div className="rounded border border-emerald-900/50 bg-emerald-950/30 p-3 text-sm text-emerald-300">
        Claude Code is signed in
        {status.version ? ` (${status.version})` : ''}.
      </div>
    );
  }

  const oauthBrowserUrl = status.auth.oauth_browser_url;
  const oauthUrlReady = Boolean(oauthBrowserUrl);
  const awaitingCodeImport =
    Boolean(browserId) &&
    !status.auth.code_submitted &&
    (signInOpened ||
      status.auth.phase === 'waiting_code' ||
      status.auth.phase === 'code_submitted');
  const primaryBtn =
    'px-4 py-2 rounded bg-bunny-accent text-bunny-bg text-sm font-medium disabled:opacity-50';
  const secondaryBtn =
    'px-4 py-2 rounded border border-bunny-border text-bunny-muted text-sm disabled:opacity-40 cursor-not-allowed';
  const installBusy =
    installing || status.install.state === 'installing' || status.install.state === 'downloading';

  return (
    <div className="rounded border border-bunny-accent/40 bg-bunny-panel p-4 space-y-3 text-sm">
      <h3 className="font-medium text-bunny-accent">Claude Code setup</h3>

      {!status.installed && (
        <div className="space-y-2">
          <p className="text-bunny-muted">
            Install Claude on this server (Docker or VM). The installer is cached locally by
            Bunny.
          </p>
          {status.install.message && (
            <p className="text-gray-300">{status.install.message}</p>
          )}
          {status.install.error && (
            <p className="text-red-400" role="alert">
              {status.install.error}
            </p>
          )}
          <button
            type="button"
            disabled={installBusy}
            onClick={handleInstall}
            className="px-3 py-1.5 rounded bg-bunny-accent text-bunny-bg text-xs font-medium disabled:opacity-50"
          >
            {installBusy ? 'Installing…' : 'Install Claude'}
          </button>
        </div>
      )}

      {status.installed && (
        <div className="space-y-3">
          <p className="text-bunny-muted">
            Sign in using the remote browser (same machine as the agent — OAuth works without
            copying URLs to your laptop).
          </p>

          {oauthUrlReady ? (
            <div className="space-y-3">
              <div className="flex flex-wrap gap-2">
                <button
                  type="button"
                  disabled={browserOpening || awaitingCodeImport}
                  onClick={() => openOAuthInBrowser(oauthBrowserUrl!)}
                  className={awaitingCodeImport ? secondaryBtn : primaryBtn}
                >
                  {browserOpening ? 'Opening browser…' : 'Open sign-in in Browser tab'}
                </button>
                <button
                  type="button"
                  disabled={!browserId || importing || !awaitingCodeImport || status.auth.code_submitted}
                  onClick={() => void tryImportFromBrowser({ quiet: false })}
                  className={awaitingCodeImport ? primaryBtn : secondaryBtn}
                >
                  {importing ? 'Reading code…' : status.auth.code_submitted ? 'Code sent' : 'Import code from browser'}
                </button>
              </div>
              <p className="text-xs text-bunny-muted">
                {status.auth.code_submitted
                  ? 'Code sent once to the terminal — wait for Claude to finish sign-in.'
                  : awaitingCodeImport
                    ? 'Step 2 — open the Browser tab, wait for “Authentication Code”, then click Import.'
                    : 'Step 1 — open the Browser tab and complete Claude sign-in.'}
              </p>
            </div>
          ) : (
            <p className="text-xs text-bunny-muted">
              Waiting for the full sign-in link from Claude (must include OAuth scopes)…
            </p>
          )}

          <form onSubmit={handleSubmitCode} className="flex flex-col gap-2 sm:flex-row sm:items-end">
            <label className="flex-1 flex flex-col gap-1 text-xs text-bunny-muted">
              Authorization code
              <input
                type="text"
                value={code}
                onChange={(e) => setCode(e.target.value)}
                placeholder="Paste code from browser"
                className="px-2 py-1.5 rounded border border-bunny-border bg-bunny-bg text-gray-200 text-sm"
                autoComplete="off"
              />
            </label>
            <button
              type="submit"
              disabled={busy || !code.trim() || status.auth.code_submitted}
              className="px-3 py-1.5 rounded border border-bunny-border text-gray-200 text-xs hover:bg-bunny-bg disabled:opacity-50 shrink-0"
            >
              {busy ? 'Sending…' : 'Submit code'}
            </button>
          </form>

          {feedback && (
            <p className="text-emerald-300 text-xs" role="status">
              {feedback}
            </p>
          )}

          {status.auth.error && (
            <p className="text-red-400 text-xs" role="alert">
              {status.auth.error}
            </p>
          )}
        </div>
      )}

      {error && (
        <p className="text-red-400 text-xs" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
