import { useCallback, useEffect, useState, type MouseEvent } from 'react';
import {
  createSession,
  deleteSession,
  getClaudeStatus,
  installClaude,
  listSessions,
  me,
  renameSession,
  startClaudeAuth,
  type ClaudeStatus,
} from '../lib/api';
import { useT } from '../i18n';
import AppTopBar from './AppTopBar';
import BunnyLogo from './BunnyLogo';
import DiscordAccountPanel from './DiscordAccountPanel';
import GitIdentityPanel from './GitIdentityPanel';
import InlineRename from './InlineRename';

interface SessionItem {
  id: string;
  name: string;
  project_path: string;
  status: string;
}

interface Props {
  email: string;
  isOwner: boolean;
  canCreateSessions: boolean;
}

export default function HomePage({ email, isOwner, canCreateSessions }: Props) {
  const tr = useT();
  const [sessions, setSessions] = useState<SessionItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [creating, setCreating] = useState(false);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [claude, setClaude] = useState<ClaudeStatus | null>(null);
  const [claudeBusy, setClaudeBusy] = useState(false);
  const [gitConfigured, setGitConfigured] = useState<boolean | null>(null);
  const [gitPanelOpen, setGitPanelOpen] = useState(false);

  const refreshClaude = useCallback(() => {
    getClaudeStatus()
      .then(setClaude)
      .catch(() => setClaude(null));
  }, []);

  const refresh = useCallback(() => {
    setLoading(true);
    setError(null);
    listSessions()
      .then(setSessions)
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    refresh();
    refreshClaude();
    me()
      .then((profile) => setGitConfigured(profile.git_configured))
      .catch(() => setGitConfigured(null));
  }, [refresh, refreshClaude]);

  useEffect(() => {
    if (!claude) return;
    const installing =
      claude.install.state === 'installing' || claude.install.state === 'downloading';
    if (!installing && claude.auth.phase !== 'waiting_url' && claude.auth.phase !== 'waiting_code') {
      return;
    }
    const timer = setInterval(refreshClaude, 1500);
    return () => clearInterval(timer);
  }, [claude, refreshClaude]);

  async function handleNewSession() {
    setCreating(true);
    setError(null);
    try {
      const { id } = await createSession();
      location.href = `/s/${id}`;
    } catch (e) {
      setError(String(e));
      setCreating(false);
    }
  }

  function openSession(id: string) {
    location.href = `/s/${id}`;
  }

  async function handleSetupClaude() {
    setClaudeBusy(true);
    setError(null);
    try {
      if (!claude?.installed) {
        await installClaude();
        let attempts = 0;
        while (attempts < 120) {
          await new Promise((r) => setTimeout(r, 2000));
          const s = await getClaudeStatus();
          setClaude(s);
          if (s.installed) break;
          if (s.install.state === 'failed') {
            throw new Error(s.install.error || tr('web.home.claudeInstallFailed'));
          }
          attempts += 1;
        }
        const final = await getClaudeStatus();
        if (!final.installed) {
          throw new Error(tr('web.home.claudeInstallTimeout'));
        }
      }
      const { session_id } = await startClaudeAuth();
      location.href = `/s/${session_id}?claude=setup`;
    } catch (e) {
      setError(String(e));
      setClaudeBusy(false);
    }
  }

  async function handleDeleteSession(id: string, e: MouseEvent) {
    e.stopPropagation();
    if (!window.confirm(tr('web.home.deleteSessionConfirm'))) {
      return;
    }
    setDeletingId(id);
    setError(null);
    try {
      await deleteSession(id);
      setSessions((prev) => prev.filter((s) => s.id !== id));
    } catch (err) {
      setError(String(err));
    } finally {
      setDeletingId(null);
    }
  }

  return (
    <div className="min-h-screen flex flex-col items-center justify-center gap-6 p-6">
      <div className="w-full max-w-lg flex justify-end">
        <AppTopBar />
      </div>
      <BunnyLogo />
      <div className="text-center space-y-1">
        <h1 className="text-xl text-bunny-accent">
          {tr('web.home.welcome', { email })}
        </h1>
        <p className="text-bunny-muted text-sm">{tr('web.home.subtitle')}</p>
      </div>

      <div className="flex flex-wrap items-center justify-center gap-3">
        {canCreateSessions ? (
          <button
            type="button"
            onClick={handleNewSession}
            disabled={creating}
            className="px-5 py-2.5 rounded bg-bunny-accent text-bunny-on-accent font-medium text-sm hover:opacity-90 disabled:opacity-50"
          >
            {creating ? tr('web.home.creating') : tr('web.home.newSession')}
          </button>
        ) : null}
        <button
          type="button"
          onClick={() => { location.href = '/security'; }}
          className="px-5 py-2.5 rounded border border-bunny-border text-gray-200 font-medium text-sm hover:bg-bunny-panel"
        >
          {tr('web.home.security')}
        </button>
        {isOwner && (
          <button
            type="button"
            onClick={() => { location.href = '/team'; }}
            className="px-5 py-2.5 rounded border border-bunny-border text-gray-200 font-medium text-sm hover:bg-bunny-panel"
          >
            {tr('web.home.team')}
          </button>
        )}
        {isOwner && (
          <button
            type="button"
            onClick={() => { location.href = '/secrets'; }}
            className="px-5 py-2.5 rounded border border-bunny-border text-gray-200 font-medium text-sm hover:bg-bunny-panel"
          >
            {tr('web.home.secretsVault')}
          </button>
        )}
        {claude && !claude.authenticated && (
          <button
            type="button"
            onClick={handleSetupClaude}
            disabled={
              claudeBusy ||
              claude.install.state === 'installing' ||
              claude.install.state === 'downloading'
            }
            className="px-5 py-2.5 rounded border border-bunny-accent text-bunny-accent font-medium text-sm hover:bg-bunny-panel disabled:opacity-50"
          >
            {claudeBusy ||
            claude.install.state === 'installing' ||
            claude.install.state === 'downloading'
              ? tr('web.home.settingUp')
              : tr('web.home.setupClaude')}
          </button>
        )}
      </div>

      {claude?.authenticated && (
        <p className="text-xs text-emerald-400 text-center">
          {tr('web.home.claudeReady')}
          {claude.version ? ` · ${claude.version}` : ''}
        </p>
      )}

      {claude && !claude.authenticated && claude.install.message && !claude.installed && (
        <p className="text-bunny-muted text-xs max-w-md text-center">{claude.install.message}</p>
      )}

      {error && (
        <p className="text-red-400 text-sm max-w-md text-center" role="alert">
          {error}
        </p>
      )}

      {gitConfigured === false ? (
        <GitIdentityPanel
          onConfiguredChange={(configured) => {
            setGitConfigured(configured);
            if (configured) setGitPanelOpen(false);
          }}
        />
      ) : gitConfigured === true && gitPanelOpen ? (
        <GitIdentityPanel
          onClose={() => setGitPanelOpen(false)}
          onConfiguredChange={setGitConfigured}
        />
      ) : gitConfigured === true ? (
        <div className="w-full max-w-lg flex justify-center">
          <button
            type="button"
            onClick={() => setGitPanelOpen(true)}
            className="px-4 py-2 rounded border border-bunny-border text-gray-200 text-sm hover:bg-bunny-panel"
          >
            {tr('web.home.gitSettings')}
          </button>
        </div>
      ) : null}

      <DiscordAccountPanel isOwner={isOwner} />

      <div className="w-full max-w-lg">
        <h2 className="text-xs uppercase tracking-wide text-bunny-muted mb-2">
          {tr('web.home.recentSessions')}
        </h2>
        {loading ? (
          <p className="text-bunny-muted text-sm">{tr('web.common.loading')}</p>
        ) : sessions.length === 0 ? (
          <p className="text-bunny-muted text-sm">{tr('web.home.noSessions')}</p>
        ) : (
          <ul className="divide-y divide-bunny-border border border-bunny-border rounded overflow-hidden">
            {sessions.map((s) => (
              <li key={s.id} className="flex items-stretch">
                <button
                  type="button"
                  onClick={() => openSession(s.id)}
                  className="flex-1 text-left px-4 py-3 hover:bg-bunny-panel transition-colors min-w-0"
                >
                  <span
                    className="block text-sm text-gray-200"
                    onClick={(e) => e.stopPropagation()}
                  >
                    <InlineRename
                      value={s.name}
                      className="font-medium max-w-full"
                      title={tr('web.home.renameHint')}
                      onSave={async (name) => {
                        const updated = await renameSession(s.id, name);
                        setSessions((prev) =>
                          prev.map((row) =>
                            row.id === s.id ? { ...row, name: updated.name } : row,
                          ),
                        );
                      }}
                    />
                  </span>
                  <span className="block text-xs text-bunny-muted truncate mt-0.5">
                    {s.project_path}
                  </span>
                  <span className="text-xs text-bunny-accent">{s.status}</span>
                </button>
                <button
                  type="button"
                  title={tr('web.home.deleteSession')}
                  aria-label={tr('web.home.deleteSession')}
                  disabled={deletingId === s.id}
                  onClick={(e) => handleDeleteSession(s.id, e)}
                  className="px-3 text-bunny-muted hover:text-red-400 hover:bg-bunny-panel disabled:opacity-50 shrink-0"
                >
                  {deletingId === s.id ? '…' : '×'}
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
