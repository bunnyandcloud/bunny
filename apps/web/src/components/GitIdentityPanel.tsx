import { useCallback, useEffect, useState } from 'react';
import { apiErrorMessage, me, updateGitProfile } from '../lib/api';
import { useT } from '../i18n';

interface Props {
  /** When set, show a close control (edit mode from home Git button). */
  onClose?: () => void;
  /** Notify parent when git profile becomes configured or is updated. */
  onConfiguredChange?: (configured: boolean) => void;
}

export default function GitIdentityPanel({ onClose, onConfiguredChange }: Props) {
  const tr = useT();
  const [gitName, setGitName] = useState('');
  const [gitEmail, setGitEmail] = useState('');
  const [accountEmail, setAccountEmail] = useState('');
  const [configured, setConfigured] = useState(false);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [banner, setBanner] = useState<string | null>(null);

  const refresh = useCallback(() => {
    setLoading(true);
    setError(null);
    me()
      .then((profile) => {
        setAccountEmail(profile.email);
        setGitName(profile.git_name ?? '');
        setGitEmail(profile.git_email ?? profile.email);
        setConfigured(profile.git_configured);
      })
      .catch((e) => setError(apiErrorMessage(e, tr('web.git.account.loadFailed'))))
      .finally(() => setLoading(false));
  }, [tr]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  async function handleSave(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    setBanner(null);
    try {
      const profile = await updateGitProfile({
        git_name: gitName.trim(),
        git_email: gitEmail.trim(),
      });
      setConfigured(profile.git_configured);
      onConfiguredChange?.(profile.git_configured);
      setBanner(tr('web.git.account.saveSuccess'));
      if (profile.git_configured && onClose) {
        onClose();
      }
    } catch (err) {
      setError(apiErrorMessage(err, tr('web.git.account.saveFailed')));
    } finally {
      setBusy(false);
    }
  }

  const required = !configured && !onClose;

  return (
    <div
      className={`w-full max-w-lg border rounded-lg p-4 bg-bunny-panel space-y-3 ${
        required ? 'border-amber-500/40' : 'border-bunny-border'
      }`}
    >
      <div className="flex items-start justify-between gap-3">
        <div>
          <h2 className="text-sm font-medium text-gray-200">{tr('web.git.account.title')}</h2>
          <p className="text-xs text-bunny-muted mt-1">{tr('web.git.account.description')}</p>
        </div>
        {onClose ? (
          <button
            type="button"
            onClick={onClose}
            className="shrink-0 px-2 py-1 rounded border border-bunny-border text-gray-200 text-xs hover:bg-bunny-bg"
          >
            {tr('web.git.account.close')}
          </button>
        ) : null}
      </div>

      {banner ? (
        <p className="text-xs text-emerald-400" role="status">
          {banner}
        </p>
      ) : null}

      {error ? (
        <p className="text-red-400 text-xs" role="alert">
          {error}
        </p>
      ) : null}

      {loading ? (
        <p className="text-bunny-muted text-xs">{tr('web.common.loading')}</p>
      ) : (
        <form onSubmit={handleSave} className="space-y-3">
          {!configured ? (
            <p className="text-xs text-amber-400/90">{tr('web.git.account.notConfigured')}</p>
          ) : (
            <p className="text-xs text-bunny-muted">{tr('web.git.account.configured')}</p>
          )}
          <label className="block space-y-1">
            <span className="text-xs text-bunny-muted">{tr('web.git.account.nameLabel')}</span>
            <input
              type="text"
              value={gitName}
              onChange={(e) => setGitName(e.target.value)}
              placeholder={tr('web.git.account.namePlaceholder')}
              className="w-full px-3 py-2 rounded bg-bunny-bg border border-bunny-border text-sm text-gray-200"
              autoComplete="name"
            />
          </label>
          <label className="block space-y-1">
            <span className="text-xs text-bunny-muted">{tr('web.git.account.emailLabel')}</span>
            <input
              type="email"
              value={gitEmail}
              onChange={(e) => setGitEmail(e.target.value)}
              placeholder={accountEmail}
              className="w-full px-3 py-2 rounded bg-bunny-bg border border-bunny-border text-sm text-gray-200"
              autoComplete="email"
            />
          </label>
          <button
            type="submit"
            disabled={busy || !gitName.trim() || !gitEmail.trim()}
            className="px-4 py-2 rounded bg-bunny-accent text-bunny-bg text-sm font-medium hover:opacity-90 disabled:opacity-50"
          >
            {busy ? tr('web.common.loading') : tr('web.git.account.save')}
          </button>
        </form>
      )}
    </div>
  );
}
