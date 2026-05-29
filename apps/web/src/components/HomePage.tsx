import { useCallback, useEffect, useState, type MouseEvent } from 'react';
import {
  createSession,
  deleteSession,
  getClaudeStatus,
  installClaude,
  listSessions,
  renameSession,
  startClaudeAuth,
  type ClaudeStatus,
} from '../lib/api';
import InlineRename from './InlineRename';
import LogoutButton from './LogoutButton';

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
  const [sessions, setSessions] = useState<SessionItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [creating, setCreating] = useState(false);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [claude, setClaude] = useState<ClaudeStatus | null>(null);
  const [claudeBusy, setClaudeBusy] = useState(false);

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
  }, [refresh, refreshClaude]);

  useEffect(() => {
    if (!claude) return;
    const installing =
      claude.install.state === 'installing' || claude.install.state === 'downloading';
    if (!installing && claude.auth.phase !== 'waiting_url' && claude.auth.phase !== 'waiting_code') {
      return;
    }
    const t = setInterval(refreshClaude, 1500);
    return () => clearInterval(t);
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
            throw new Error(s.install.error || 'Claude installation failed');
          }
          attempts += 1;
        }
        const final = await getClaudeStatus();
        if (!final.installed) {
          throw new Error('Claude installation timed out');
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
    if (
      !window.confirm(
        'Delete this session? All shells will be stopped and removed.',
      )
    ) {
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
        <LogoutButton />
      </div>
      <div className="text-center space-y-1">
        <h1 className="text-xl text-bunny-accent">Welcome, {email}</h1>
        <p className="text-bunny-muted text-sm">Start or resume a remote dev session.</p>
      </div>

      <div className="flex flex-wrap items-center justify-center gap-3">
        {canCreateSessions ? (
          <button
            type="button"
            onClick={handleNewSession}
            disabled={creating}
            className="px-5 py-2.5 rounded bg-bunny-accent text-bunny-bg font-medium text-sm hover:opacity-90 disabled:opacity-50"
          >
            {creating ? 'Creating…' : 'New session'}
          </button>
        ) : null}
        <button
          type="button"
          onClick={() => { location.href = '/security'; }}
          className="px-5 py-2.5 rounded border border-bunny-border text-gray-200 font-medium text-sm hover:bg-bunny-panel"
        >
          Security
        </button>
        {isOwner && (
          <button
            type="button"
            onClick={() => {
              location.href = '/team';
            }}
            className="px-5 py-2.5 rounded border border-bunny-border text-gray-200 font-medium text-sm hover:bg-bunny-panel"
          >
            Team
          </button>
        )}
        {isOwner && (
          <button
            type="button"
            onClick={() => { location.href = '/secrets'; }}
            className="px-5 py-2.5 rounded border border-bunny-border text-gray-200 font-medium text-sm hover:bg-bunny-panel"
          >
            Secrets vault
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
              ? 'Setting up…'
              : 'Setup Claude'}
          </button>
        )}
      </div>

      {claude?.authenticated && (
        <p className="text-xs text-emerald-400 text-center">
          Claude Code ready{claude.version ? ` · ${claude.version}` : ''}
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

      <div className="w-full max-w-lg">
        <h2 className="text-xs uppercase tracking-wide text-bunny-muted mb-2">
          Recent sessions
        </h2>
        {loading ? (
          <p className="text-bunny-muted text-sm">Loading…</p>
        ) : sessions.length === 0 ? (
          <p className="text-bunny-muted text-sm">No sessions yet.</p>
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
                      title="Double-click to rename session"
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
                  title="Delete session"
                  aria-label="Delete session"
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
