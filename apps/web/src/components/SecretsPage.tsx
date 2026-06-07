import { useCallback, useEffect, useState, type FormEvent } from 'react';
import {
  deleteSecret,
  getSecretsStatus,
  initSecretsVault,
  listSecrets,
  listSessions,
  lockSecretsVault,
  revealSecret,
  unlockSecretsVault,
  upsertSecret,
  type SecretMeta,
  type VaultStatus,
} from '../lib/api';
import { copyToClipboard } from '../lib/copyToClipboard';
import { useT } from '../i18n';
import AppTopBar from './AppTopBar';

interface Props {
  email: string;
}

type Scope = 'system' | 'project' | 'session';

const SCOPES: Scope[] = ['system', 'project', 'session'];

export default function SecretsPage({ email }: Props) {
  const tr = useT();
  const [status, setStatus] = useState<VaultStatus | null>(null);
  const [secrets, setSecrets] = useState<SecretMeta[]>([]);
  const [sessions, setSessions] = useState<Array<{ id: string; name: string }>>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const [passphrase, setPassphrase] = useState('');
  const [confirmPassphrase, setConfirmPassphrase] = useState('');

  const [showForm, setShowForm] = useState(false);
  const [formName, setFormName] = useState('');
  const [formScope, setFormScope] = useState<Scope>('system');
  const [formSessionId, setFormSessionId] = useState('');
  const [formValue, setFormValue] = useState('');

  const [revealed, setRevealed] = useState<Record<string, string>>({});
  const [savedNotice, setSavedNotice] = useState<{ name: string; env_var: string } | null>(null);
  const [copiedEnvVar, setCopiedEnvVar] = useState<string | null>(null);

  const refreshStatus = useCallback(async () => {
    const s = await getSecretsStatus();
    setStatus(s);
    return s;
  }, []);

  const refreshSecrets = useCallback(async () => {
    const list = await listSecrets();
    setSecrets(list);
  }, []);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const s = await refreshStatus();
      if (s.status === 'unlocked') {
        await refreshSecrets();
      } else {
        setSecrets([]);
      }
      const sess = await listSessions();
      setSessions(sess.map((x) => ({ id: x.id, name: x.name })));
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [refreshSecrets, refreshStatus]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  async function handleInit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await initSecretsVault(passphrase, confirmPassphrase);
      setPassphrase('');
      setConfirmPassphrase('');
      await refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function handleUnlock(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await unlockSecretsVault(passphrase);
      setPassphrase('');
      await refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function handleLock() {
    if (
      !window.confirm(
        'Verrouiller le vault ?\n\nLes shells existants gardent leurs variables, mais les nouveaux n\'auront plus de secrets jusqu\'au prochain unlock.',
      )
    ) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await lockSecretsVault();
      setRevealed({});
      setSavedNotice(null);
      await refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function handleSaveSecret(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const saved = await upsertSecret({
        name: formName,
        scope: formScope,
        session_id: formScope === 'session' ? formSessionId : undefined,
        value: formValue,
      });
      setSavedNotice({ name: saved.name, env_var: saved.env_var });
      setFormName('');
      setFormValue('');
      setFormScope('system');
      setFormSessionId('');
      setShowForm(false);
      await refreshSecrets();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function handleDelete(secret: SecretMeta) {
    if (
      !window.confirm(
        `Delete secret "${secret.name}" (${secret.scope})? This cannot be undone.`,
      )
    ) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await deleteSecret(secret.name, secret.scope, secret.session_id ?? undefined);
      setRevealed((prev) => {
        const next = { ...prev };
        delete next[secretKey(secret)];
        return next;
      });
      await refreshSecrets();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function handleReveal(secret: SecretMeta) {
    const key = secretKey(secret);
    if (revealed[key]) {
      setRevealed((prev) => {
        const next = { ...prev };
        delete next[key];
        return next;
      });
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const { value } = await revealSecret(
        secret.name,
        secret.scope,
        secret.session_id ?? undefined,
      );
      setRevealed((prev) => ({ ...prev, [key]: value }));
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  function secretKey(s: SecretMeta) {
    return `${s.name}:${s.scope}:${s.session_id ?? '-'}`;
  }

  function sessionLabel(id: string | null) {
    if (!id) return '—';
    const s = sessions.find((x) => x.id === id);
    return s ? `${s.name} (${id.slice(0, 8)}…)` : id.slice(0, 8) + '…';
  }

  async function handleCopyEnvVar(envVar: string) {
    const ok = await copyToClipboard(envVar);
    if (ok) {
      setCopiedEnvVar(envVar);
      window.setTimeout(() => setCopiedEnvVar((v) => (v === envVar ? null : v)), 2000);
    }
  }

  return (
    <div className="min-h-screen flex flex-col p-6 max-w-3xl mx-auto gap-6">
      <header className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-xl text-bunny-accent">{tr('web.secrets.title')}</h1>
          <p className="text-bunny-muted text-sm mt-1">{tr('web.secrets.subtitle')}</p>
        </div>
        <AppTopBar />
      </header>

      {error && (
        <p className="text-red-400 text-sm" role="alert">
          {error}
        </p>
      )}

      {loading ? (
        <p className="text-bunny-muted text-sm">{tr('web.common.loading')}</p>
      ) : status ? (
        <>
          <section className="border border-bunny-border rounded p-4 bg-bunny-panel space-y-2">
            <div className="flex items-center justify-between gap-4">
              <h2 className="text-xs uppercase tracking-wide text-bunny-muted">Vault status</h2>
              {status.status === 'unlocked' && (
                <button
                  type="button"
                  disabled={busy}
                  onClick={handleLock}
                  className="text-xs px-3 py-1 rounded border border-bunny-border text-bunny-muted hover:text-red-400 disabled:opacity-50"
                >
                  Lock vault
                </button>
              )}
            </div>
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
              <dt className="text-bunny-muted">State</dt>
              <dd>
                <StatusBadge state={status.status} />
              </dd>
              <dt className="text-bunny-muted">Path</dt>
              <dd className="font-mono text-xs text-gray-300 break-all">{status.path}</dd>
              <dt className="text-bunny-muted">DB refs</dt>
              <dd className="text-gray-300">{status.ref_count}</dd>
            </dl>
          </section>

          {status.status === 'missing' && (
            <section className="border border-bunny-border rounded p-4 space-y-4">
              <h2 className="text-sm font-medium text-gray-200">Create vault</h2>
              <p className="text-bunny-muted text-sm">
                No secrets vault exists yet. Choose a strong passphrase — it encrypts all secret values
                on disk.
              </p>
              <form onSubmit={handleInit} className="space-y-3 max-w-sm">
                <label className="block text-sm">
                  <span className="text-bunny-muted">Passphrase</span>
                  <input
                    type="password"
                    required
                    minLength={8}
                    value={passphrase}
                    onChange={(e) => setPassphrase(e.target.value)}
                    className="mt-1 w-full px-3 py-2 rounded bg-bunny-bg border border-bunny-border text-sm"
                    autoComplete="new-password"
                  />
                </label>
                <label className="block text-sm">
                  <span className="text-bunny-muted">Confirm passphrase</span>
                  <input
                    type="password"
                    required
                    minLength={8}
                    value={confirmPassphrase}
                    onChange={(e) => setConfirmPassphrase(e.target.value)}
                    className="mt-1 w-full px-3 py-2 rounded bg-bunny-bg border border-bunny-border text-sm"
                    autoComplete="new-password"
                  />
                </label>
                <button
                  type="submit"
                  disabled={busy}
                  className="px-4 py-2 rounded bg-bunny-accent text-bunny-on-accent text-sm font-medium hover:opacity-90 disabled:opacity-50"
                >
                  {busy ? 'Creating…' : 'Create vault'}
                </button>
              </form>
            </section>
          )}

          {status.status === 'locked' && (
            <section className="border border-bunny-border rounded p-4 space-y-4">
              <h2 className="text-sm font-medium text-gray-200">Unlock vault</h2>
              <p className="text-bunny-muted text-sm">
                {status.ref_count > 0 ? (
                  <>
                    {status.ref_count} secret{status.ref_count > 1 ? 's' : ''} enregistré
                    {status.ref_count > 1 ? 's' : ''} — déverrouille pour les injecter dans les
                    terminaux (<code className="text-gray-300">BUNNY_SECRET_*</code>).
                  </>
                ) : (
                  <>
                    Enter your vault passphrase to view and manage secrets. The passphrase stays in
                    server memory until you lock the vault or restart the agent.
                  </>
                )}
              </p>
              <form onSubmit={handleUnlock} className="space-y-3 max-w-sm">
                <label className="block text-sm">
                  <span className="text-bunny-muted">Passphrase</span>
                  <input
                    type="password"
                    required
                    value={passphrase}
                    onChange={(e) => setPassphrase(e.target.value)}
                    className="mt-1 w-full px-3 py-2 rounded bg-bunny-bg border border-bunny-border text-sm"
                    autoComplete="current-password"
                  />
                </label>
                <button
                  type="submit"
                  disabled={busy}
                  className="px-4 py-2 rounded bg-bunny-accent text-bunny-on-accent text-sm font-medium hover:opacity-90 disabled:opacity-50"
                >
                  {busy ? 'Unlocking…' : 'Unlock vault'}
                </button>
              </form>
            </section>
          )}

          {status.status === 'unlocked' && (
            <section className="space-y-4">
              {savedNotice && (
                <div className="border border-green-400/40 bg-green-400/10 rounded p-4 space-y-2 text-sm">
                  <p className="text-green-300 font-medium">
                    Secret « {savedNotice.name} » enregistré
                  </p>
                  <p className="text-bunny-muted text-xs leading-relaxed">
                    Le vault est déverrouillé — ouvre un <strong className="text-gray-300">nouveau
                    shell</strong> dans ta session pour utiliser{' '}
                    <code className="text-bunny-accent">{savedNotice.env_var}</code>. Garde le vault
                    unlocked pendant que tu travailles dans les terminaux.
                  </p>
                  <button
                    type="button"
                    onClick={() => setSavedNotice(null)}
                    className="text-xs text-bunny-muted hover:text-gray-200"
                  >
                    Dismiss
                  </button>
                </div>
              )}

              <div className="flex items-center justify-between gap-4">
                <h2 className="text-xs uppercase tracking-wide text-bunny-muted">Secrets</h2>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => setShowForm((v) => !v)}
                  className="text-sm px-3 py-1.5 rounded bg-bunny-accent text-bunny-on-accent font-medium hover:opacity-90 disabled:opacity-50"
                >
                  {showForm ? 'Cancel' : 'Add secret'}
                </button>
              </div>

              {showForm && (
                <form
                  onSubmit={handleSaveSecret}
                  className="border border-bunny-border rounded p-4 bg-bunny-panel space-y-3"
                >
                  <h3 className="text-sm font-medium text-gray-200">New or update secret</h3>
                  <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                    <label className="block text-sm sm:col-span-2">
                      <span className="text-bunny-muted">Name</span>
                      <input
                        type="text"
                        required
                        pattern="[A-Za-z0-9_-]+"
                        value={formName}
                        onChange={(e) => setFormName(e.target.value)}
                        placeholder="API_KEY"
                        className="mt-1 w-full px-3 py-2 rounded bg-bunny-bg border border-bunny-border text-sm font-mono"
                      />
                    </label>
                    <label className="block text-sm">
                      <span className="text-bunny-muted">Scope</span>
                      <select
                        value={formScope}
                        onChange={(e) => setFormScope(e.target.value as Scope)}
                        className="mt-1 w-full px-3 py-2 rounded bg-bunny-bg border border-bunny-border text-sm"
                      >
                        {SCOPES.map((s) => (
                          <option key={s} value={s}>
                            {s}
                          </option>
                        ))}
                      </select>
                    </label>
                    {formScope === 'session' && (
                      <label className="block text-sm">
                        <span className="text-bunny-muted">Session</span>
                        <select
                          required
                          value={formSessionId}
                          onChange={(e) => setFormSessionId(e.target.value)}
                          className="mt-1 w-full px-3 py-2 rounded bg-bunny-bg border border-bunny-border text-sm"
                        >
                          <option value="">Select session…</option>
                          {sessions.map((s) => (
                            <option key={s.id} value={s.id}>
                              {s.name}
                            </option>
                          ))}
                        </select>
                      </label>
                    )}
                    <label className="block text-sm sm:col-span-2">
                      <span className="text-bunny-muted">Value</span>
                      <input
                        type="password"
                        required
                        value={formValue}
                        onChange={(e) => setFormValue(e.target.value)}
                        className="mt-1 w-full px-3 py-2 rounded bg-bunny-bg border border-bunny-border text-sm font-mono"
                        autoComplete="off"
                      />
                    </label>
                  </div>
                  <button
                    type="submit"
                    disabled={busy}
                    className="px-4 py-2 rounded bg-bunny-accent text-bunny-on-accent text-sm font-medium hover:opacity-90 disabled:opacity-50"
                  >
                    {busy ? 'Saving…' : 'Save secret'}
                  </button>
                </form>
              )}

              {secrets.length === 0 ? (
                <p className="text-bunny-muted text-sm">No secrets yet.</p>
              ) : (
                <div className="border border-bunny-border rounded overflow-hidden">
                  <table className="w-full text-sm">
                    <thead>
                      <tr className="border-b border-bunny-border bg-bunny-panel text-left text-xs uppercase tracking-wide text-bunny-muted">
                        <th className="px-4 py-2 font-normal">Name</th>
                        <th className="px-4 py-2 font-normal">Scope</th>
                        <th className="px-4 py-2 font-normal hidden sm:table-cell">Session</th>
                        <th
                          className="px-4 py-2 font-normal hidden md:table-cell"
                          title="Disponible dans les nouveaux shells si le vault est unlocked"
                        >
                          Env var
                        </th>
                        <th className="px-4 py-2 font-normal w-28">Actions</th>
                      </tr>
                    </thead>
                    <tbody className="divide-y divide-bunny-border">
                      {secrets.map((secret) => {
                        const key = secretKey(secret);
                        const value = revealed[key];
                        return (
                          <tr key={key} className="hover:bg-bunny-panel/50">
                            <td className="px-4 py-3 font-mono text-gray-200">{secret.name}</td>
                            <td className="px-4 py-3 text-bunny-muted">{secret.scope}</td>
                            <td className="px-4 py-3 text-bunny-muted text-xs hidden sm:table-cell">
                              {sessionLabel(secret.session_id)}
                            </td>
                            <td className="px-4 py-3 hidden md:table-cell">
                              <button
                                type="button"
                                title="Disponible dans les nouveaux shells si le vault est unlocked — cliquer pour copier"
                                onClick={() => handleCopyEnvVar(secret.env_var)}
                                className="font-mono text-xs text-bunny-accent hover:underline text-left"
                              >
                                {copiedEnvVar === secret.env_var
                                  ? 'Copied!'
                                  : secret.env_var}
                              </button>
                            </td>
                            <td className="px-4 py-3">
                              <div className="flex flex-col gap-1">
                                <button
                                  type="button"
                                  disabled={busy}
                                  onClick={() => handleReveal(secret)}
                                  className="text-xs text-bunny-accent hover:underline disabled:opacity-50 text-left"
                                >
                                  {value ? 'Hide' : 'Reveal'}
                                </button>
                                {value && (
                                  <span className="font-mono text-xs text-gray-400 break-all max-w-[12rem]">
                                    {value}
                                  </span>
                                )}
                                <button
                                  type="button"
                                  disabled={busy}
                                  onClick={() => handleDelete(secret)}
                                  className="text-xs text-red-400 hover:underline disabled:opacity-50 text-left"
                                >
                                  Delete
                                </button>
                              </div>
                            </td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                </div>
              )}
            </section>
          )}
        </>
      ) : null}

      <p className="text-bunny-muted text-xs text-center">
        Signed in as {email} · Owner only
      </p>
    </div>
  );
}

function StatusBadge({ state }: { state: VaultStatus['status'] }) {
  const colors = {
    missing: 'text-yellow-400 border-yellow-400/40 bg-yellow-400/10',
    locked: 'text-orange-400 border-orange-400/40 bg-orange-400/10',
    unlocked: 'text-green-400 border-green-400/40 bg-green-400/10',
  };
  return (
    <span
      className={`inline-block px-2 py-0.5 rounded text-xs border capitalize ${colors[state]}`}
    >
      {state}
    </span>
  );
}
