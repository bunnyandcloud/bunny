import { useCallback, useEffect, useState, type MouseEvent } from 'react';
import { createSession, deleteSession, listSessions, renameSession } from '../lib/api';
import InlineRename from './InlineRename';

interface SessionItem {
  id: string;
  name: string;
  project_path: string;
  status: string;
}

interface Props {
  email: string;
}

export default function HomePage({ email }: Props) {
  const [sessions, setSessions] = useState<SessionItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [creating, setCreating] = useState(false);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

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
  }, [refresh]);

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
      <div className="text-center space-y-1">
        <h1 className="text-xl text-bunny-accent">Welcome, {email}</h1>
        <p className="text-bunny-muted text-sm">Start or resume a remote dev session.</p>
      </div>

      <button
        type="button"
        onClick={handleNewSession}
        disabled={creating}
        className="px-5 py-2.5 rounded bg-bunny-accent text-bunny-bg font-medium text-sm hover:opacity-90 disabled:opacity-50"
      >
        {creating ? 'Creating…' : 'New session'}
      </button>

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
