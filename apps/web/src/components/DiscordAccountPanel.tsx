import { useCallback, useEffect, useMemo, useState } from 'react';
import { apiErrorMessage, me, unlinkDiscordAccount, type DiscordAccountStatus } from '../lib/api';
import { useT } from '../i18n';

const DEFAULT_DISCORD: DiscordAccountStatus = {
  bridge_configured: false,
  oauth_configured: false,
  linked: false,
  discord_user_id: null,
  username: null,
};

interface Props {
  isOwner: boolean;
}

export default function DiscordAccountPanel({ isOwner }: Props) {
  const tr = useT();
  const [discord, setDiscord] = useState<DiscordAccountStatus>(DEFAULT_DISCORD);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [banner, setBanner] = useState<string | null>(null);

  const oauthReturn = useMemo(() => {
    const params = new URLSearchParams(location.search);
    return params.get('discord_link');
  }, []);

  const refresh = useCallback(() => {
    setLoading(true);
    setError(null);
    me()
      .then((profile) => setDiscord(profile.discord))
      .catch((e) => setError(apiErrorMessage(e, tr('web.discord.account.loadFailed'))))
      .finally(() => setLoading(false));
  }, [tr]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    if (!oauthReturn) return;
    if (oauthReturn === 'success') {
      setBanner(tr('web.discord.account.linkSuccess'));
    } else if (oauthReturn === 'conflict') {
      setBanner(tr('web.discord.account.linkConflict'));
    } else {
      setBanner(tr('web.discord.account.linkError'));
    }
    const url = new URL(location.href);
    url.searchParams.delete('discord_link');
    history.replaceState(null, '', url.pathname + url.search);
    refresh();
  }, [oauthReturn, refresh, tr]);

  async function handleDisconnect() {
    setBusy(true);
    setError(null);
    try {
      await unlinkDiscordAccount();
      setBanner(tr('web.discord.account.unlinkSuccess'));
      await refresh();
    } catch (e) {
      setError(apiErrorMessage(e, tr('web.discord.account.unlinkFailed')));
    } finally {
      setBusy(false);
    }
  }

  function handleConnect() {
    location.href = '/api/v1/auth/discord/start';
  }

  const serverConfigured = discord.bridge_configured || discord.oauth_configured;

  return (
    <div className="w-full max-w-lg border border-bunny-border rounded-lg p-4 bg-bunny-panel space-y-3">
      <div>
        <h2 className="text-sm font-medium text-gray-200">{tr('web.discord.account.title')}</h2>
        <p className="text-xs text-bunny-muted mt-1">{tr('web.discord.account.description')}</p>
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
      ) : !discord.oauth_configured ? (
        <div className="space-y-3">
          <p className="text-bunny-muted text-xs">
            {isOwner
              ? tr('web.discord.account.notConfiguredOwner')
              : tr('web.discord.account.notConfigured')}
          </p>
          {isOwner ? (
            <button
              type="button"
              onClick={() => { location.href = '/discord/setup'; }}
              className="px-4 py-2 rounded bg-[#5865F2] text-white text-sm font-medium hover:opacity-90"
            >
              {tr('web.discord.account.setupButton')}
            </button>
          ) : null}
        </div>
      ) : discord.linked ? (
        <div className="flex flex-wrap items-center justify-between gap-3">
          <p className="text-sm text-gray-200">
            {tr('web.discord.account.linkedAs', {
              name: discord.username ?? discord.discord_user_id ?? '?',
            })}
          </p>
          <button
            type="button"
            onClick={handleDisconnect}
            disabled={busy}
            className="px-3 py-1.5 rounded border border-bunny-border text-gray-200 text-xs hover:bg-bunny-bg disabled:opacity-50"
          >
            {busy ? tr('web.common.loading') : tr('web.discord.account.disconnect')}
          </button>
        </div>
      ) : (
        <button
          type="button"
          onClick={handleConnect}
          disabled={busy}
          className="px-4 py-2 rounded bg-[#5865F2] text-white text-sm font-medium hover:opacity-90 disabled:opacity-50"
        >
          {tr('web.discord.account.connect')}
        </button>
      )}

      {isOwner && serverConfigured ? (
        <button
          type="button"
          onClick={() => { location.href = '/discord/setup'; }}
          className="px-3 py-1.5 rounded border border-bunny-border text-gray-200 text-xs hover:bg-bunny-bg"
        >
          {tr('web.discord.account.manageButton')}
        </button>
      ) : null}
    </div>
  );
}
