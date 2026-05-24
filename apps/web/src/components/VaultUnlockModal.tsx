import { useState, type FormEvent } from 'react';
import { unlockSecretsVault } from '../lib/api';

interface Props {
  open: boolean;
  onClose: () => void;
  onUnlocked: () => void;
  /** Reload shells in this session after unlock (workspace unlock). */
  sessionId?: string;
}

export default function VaultUnlockModal({
  open,
  onClose,
  onUnlocked,
  sessionId,
}: Props) {
  const [passphrase, setPassphrase] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (!open) return null;

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await unlockSecretsVault(passphrase, sessionId);
      setPassphrase('');
      onUnlocked();
      onClose();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  function handleClose() {
    if (busy) return;
    setPassphrase('');
    setError(null);
    onClose();
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60"
      role="dialog"
      aria-modal="true"
      aria-labelledby="vault-unlock-title"
    >
      <div className="w-full max-w-sm rounded border border-bunny-border bg-bunny-panel p-4 space-y-3 shadow-xl">
        <h2 id="vault-unlock-title" className="text-sm font-medium text-gray-200">
          Unlock secrets vault
        </h2>
        <p className="text-bunny-muted text-xs">
          {sessionId
            ? 'Les shells ouverts dans cette session seront rechargés (sans afficher les valeurs).'
            : 'Les shells en cours seront rechargés (sans afficher les valeurs).'}
        </p>
        <form onSubmit={handleSubmit} className="space-y-3">
          <label className="block text-sm">
            <span className="text-bunny-muted">Passphrase</span>
            <input
              type="password"
              required
              autoFocus
              value={passphrase}
              onChange={(e) => setPassphrase(e.target.value)}
              className="mt-1 w-full px-3 py-2 rounded bg-bunny-bg border border-bunny-border text-sm"
              autoComplete="current-password"
            />
          </label>
          {error && (
            <p className="text-red-400 text-xs" role="alert">
              {error}
            </p>
          )}
          <div className="flex justify-end gap-2">
            <button
              type="button"
              disabled={busy}
              onClick={handleClose}
              className="px-3 py-1.5 text-sm text-bunny-muted hover:text-gray-200 disabled:opacity-50"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={busy}
              className="px-3 py-1.5 rounded bg-bunny-accent text-bunny-bg text-sm font-medium hover:opacity-90 disabled:opacity-50"
            >
              {busy ? 'Unlocking…' : 'Unlock'}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
