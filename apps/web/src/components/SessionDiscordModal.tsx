import { useEffect, useState } from 'react';
import {
  apiErrorMessage,
  createDiscordLinkCode,
  listDiscordLinks,
  revokeDiscordLinks,
} from '../lib/api';
import { copyToClipboard } from '../lib/copyToClipboard';

export default function SessionDiscordModal(props: {
  open: boolean;
  sessionId: string;
  onClose: () => void;
}) {
  const { open, sessionId, onClose } = props;
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [discordCode, setDiscordCode] = useState<string | null>(null);
  const [codeCopied, setCodeCopied] = useState(false);
  const [copyFailed, setCopyFailed] = useState(false);
  const [discordPassword, setDiscordPassword] = useState('');
  const [discordLinks, setDiscordLinks] = useState<
    Array<{ guild_id: string; channel_id: string; status: string }>
  >([]);

  useEffect(() => {
    if (!open) return;
    setDiscordCode(null);
    setCodeCopied(false);
    setCopyFailed(false);
    setDiscordPassword('');
    setError('');
    setLoading(true);
    listDiscordLinks(sessionId)
      .then(setDiscordLinks)
      .catch((e) => setError(apiErrorMessage(e, 'Cannot load Discord links')))
      .finally(() => setLoading(false));
  }, [open, sessionId]);

  async function handleGenerateCode(e?: { preventDefault: () => void }) {
    e?.preventDefault();
    if (loading || !discordPassword) return;
    setLoading(true);
    setError('');
    setDiscordCode(null);
    setCodeCopied(false);
    setCopyFailed(false);
    try {
      const res = await createDiscordLinkCode(sessionId, discordPassword);
      setDiscordCode(res.code);
    } catch (err) {
      setError(apiErrorMessage(err, 'Discord link code failed'));
    } finally {
      setLoading(false);
    }
  }

  if (!open) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60">
      <div className="w-full max-w-lg bg-bunny-panel border border-bunny-border rounded-lg p-6 space-y-4">
        <div className="flex items-start justify-between gap-4">
          <div>
            <h2 className="text-lg font-semibold text-gray-200">Discord</h2>
            <p className="text-xs text-bunny-muted">
              Link a Discord channel to this session, then use{' '}
              <code className="text-bunny-accent">/bunny</code> commands there.
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="text-bunny-muted hover:text-gray-200"
          >
            ✕
          </button>
        </div>

        {error ? (
          <p className="text-red-400 text-sm" role="alert">
            {error}
          </p>
        ) : null}

        <div className="border border-bunny-border rounded p-3 bg-bunny-bg space-y-3">
          <h3 className="text-sm font-medium text-gray-200">Link channel</h3>
          <ol className="text-xs text-bunny-muted space-y-1 list-decimal list-inside">
            <li>Generate a one-time code below (password required).</li>
            <li>
              In Discord, run{' '}
              <code className="text-bunny-accent">/bunny link CODE</code> in the channel
              you want to link.
            </li>
          </ol>
          <form
            className="flex flex-wrap items-end gap-2"
            onSubmit={handleGenerateCode}
          >
            <label className="text-xs text-bunny-muted flex flex-col gap-1">
              Password (recent auth)
              <input
                type="password"
                value={discordPassword}
                onChange={(e) => setDiscordPassword(e.target.value)}
                className="px-3 py-2 bg-bunny-panel border border-bunny-border rounded"
                disabled={loading}
              />
            </label>
            <button
              type="submit"
              disabled={loading || !discordPassword}
              className="px-3 py-2 bg-bunny-accent text-bunny-on-accent rounded font-semibold disabled:opacity-50"
            >
              Generate code
            </button>
          </form>
          {discordCode ? (
            <div className="space-y-2">
              <button
                type="button"
                onClick={() => {
                  setCopyFailed(false);
                  void copyToClipboard(discordCode).then((ok) => {
                    if (ok) {
                      setCodeCopied(true);
                      setTimeout(() => setCodeCopied(false), 2000);
                    } else {
                      setCopyFailed(true);
                    }
                  });
                }}
                className="w-full rounded-lg border-2 border-bunny-accent/50 bg-bunny-accent/10 px-4 py-4 text-center transition hover:bg-bunny-accent/20 hover:border-bunny-accent cursor-pointer"
                title="Click to copy"
              >
                <span className="block select-all text-3xl font-bold font-mono tracking-[0.2em] text-bunny-accent">
                  {discordCode}
                </span>
                <span
                  className={
                    copyFailed
                      ? 'block text-xs mt-2 text-red-400'
                      : 'block text-xs mt-2 text-bunny-muted'
                  }
                >
                  {codeCopied
                    ? 'Copied!'
                    : copyFailed
                      ? 'Copy failed — select the code above manually (⌘C / Ctrl+C)'
                      : 'Click to copy'}
                </span>
              </button>
              <p className="text-xs text-bunny-muted text-center">
                Use within 15 minutes
              </p>
            </div>
          ) : null}
        </div>

        <div className="border border-bunny-border rounded p-3 bg-bunny-bg space-y-2">
          <div className="flex items-center justify-between gap-2">
            <h3 className="text-sm font-medium text-gray-200">Linked channels</h3>
            {discordLinks.length > 0 ? (
              <button
                type="button"
                disabled={loading}
                className="text-xs px-2 py-1 rounded border border-bunny-border hover:bg-bunny-panel disabled:opacity-50"
                onClick={async () => {
                  setLoading(true);
                  setError('');
                  try {
                    await revokeDiscordLinks(sessionId);
                    setDiscordLinks([]);
                  } catch (err) {
                    setError(apiErrorMessage(err, 'Revoke failed'));
                  } finally {
                    setLoading(false);
                  }
                }}
              >
                Revoke all
              </button>
            ) : null}
          </div>
          {loading && discordLinks.length === 0 ? (
            <p className="text-sm text-bunny-muted">Loading…</p>
          ) : discordLinks.length > 0 ? (
            <ul className="text-xs text-bunny-muted space-y-1">
              {discordLinks.map((l) => (
                <li key={`${l.guild_id}-${l.channel_id}`}>
                  Channel {l.channel_id} ({l.status})
                </li>
              ))}
            </ul>
          ) : (
            <p className="text-sm text-bunny-muted">No Discord channel linked yet.</p>
          )}
        </div>
      </div>
    </div>
  );
}
