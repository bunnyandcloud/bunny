import { FormEvent, useCallback, useEffect, useMemo, useState } from 'react';
import {
  apiErrorMessage,
  getDiscordSetupStatus,
  reloadDiscordBridge,
  setupDiscordBot,
  setupDiscordOAuth,
  type DiscordSetupStatus,
} from '../lib/api';
import { copyToClipboard } from '../lib/copyToClipboard';
import { useT } from '../i18n';
import AppTopBar from './AppTopBar';

const PORTAL_URL = 'https://discord.com/developers/applications';

type Step = 'intro' | 'application' | 'bot' | 'invite' | 'oauth' | 'done' | 'manage';

function setupSectionFromUrl(): 'bot' | 'oauth' | null {
  const section = new URLSearchParams(location.search).get('section');
  if (section === 'bot' || section === 'oauth') return section;
  return null;
}

function clearSetupSectionFromUrl() {
  const url = new URL(location.href);
  if (!url.searchParams.has('section')) return;
  url.searchParams.delete('section');
  history.replaceState(null, '', url.pathname + url.search);
}

interface Props {
  email: string;
}

function StepList({ items }: { items: string[] }) {
  return (
    <ol className="list-decimal list-inside space-y-2 text-sm text-gray-200">
      {items.map((item) => (
        <li key={item} className="text-bunny-muted">
          {item}
        </li>
      ))}
    </ol>
  );
}

export default function DiscordSetupPage({ email: _email }: Props) {
  const tr = useT();
  const [status, setStatus] = useState<DiscordSetupStatus | null>(null);
  const [step, setStep] = useState<Step>('intro');
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const [bridgeReloadMsg, setBridgeReloadMsg] = useState<string | null>(null);

  const [applicationId, setApplicationId] = useState('');
  const [botToken, setBotToken] = useState('');
  const [guildId, setGuildId] = useState('');
  const [publicUrl, setPublicUrl] = useState('');
  const [oauthSecret, setOauthSecret] = useState('');
  const [oauthRedirectUri, setOauthRedirectUri] = useState('');

  const refresh = useCallback(() => {
    setLoading(true);
    setError(null);
    getDiscordSetupStatus()
      .then((s) => {
        setStatus(s);
        setPublicUrl(s.public_url);
        setOauthRedirectUri(s.oauth_redirect_uri);
        if (s.application_id) {
          setApplicationId(s.application_id);
        }
        if (s.guild_id) {
          setGuildId(s.guild_id);
        }
        const section = setupSectionFromUrl();
        if (section === 'bot') {
          setStep('bot');
          clearSetupSectionFromUrl();
        } else if (section === 'oauth') {
          setStep('oauth');
          clearSetupSectionFromUrl();
        } else if (s.bridge_configured || s.oauth_configured) {
          setStep('manage');
        }
      })
      .catch((e) => setError(apiErrorMessage(e, tr('web.discord.setup.loadFailed'))))
      .finally(() => setLoading(false));
  }, [tr]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const steps: Step[] = useMemo(
    () => ['intro', 'application', 'bot', 'invite', 'oauth', 'done'],
    [],
  );
  const stepIndex = steps.indexOf(step);

  async function copyValue(label: string, value: string) {
    await copyToClipboard(value);
    setCopied(label);
    setTimeout(() => setCopied(null), 2000);
  }

  async function handleBotSubmit(e: FormEvent) {
    e.preventDefault();
    const appId = Number(applicationId.trim());
    if (!Number.isFinite(appId) || appId <= 0) {
      setError(tr('web.discord.setup.invalidAppId'));
      return;
    }
    if (!botToken.trim()) {
      setError(tr('web.discord.setup.tokenRequired'));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const guild = guildId.trim() ? Number(guildId.trim()) : undefined;
      if (guildId.trim() && (!Number.isFinite(guild) || guild! <= 0)) {
        setError(tr('web.discord.setup.invalidGuildId'));
        return;
      }
      const res = await setupDiscordBot({
        application_id: appId,
        bot_token: botToken.trim(),
        guild_id: guild,
        public_url: publicUrl.trim() || undefined,
      });
      setOauthRedirectUri(res.oauth_redirect_uri);
      setPublicUrl(res.public_url);
      setStatus((prev) => (prev ? { ...prev, bridge_configured: true } : prev));
      if (res.bridge_starting) {
        setBridgeReloadMsg(tr('web.discord.setup.bridgeRestartStarting'));
      } else if (res.bridge_running) {
        setBridgeReloadMsg(tr('web.discord.setup.bridgeRestartOk'));
      } else {
        setBridgeReloadMsg(tr('web.discord.setup.bridgeRestartFailed'));
      }
      setStep('invite');
    } catch (err) {
      setError(apiErrorMessage(err, tr('web.discord.setup.botFailed')));
    } finally {
      setBusy(false);
    }
  }

  async function handleOAuthSubmit(e: FormEvent) {
    e.preventDefault();
    if (!oauthSecret.trim()) {
      setError(tr('web.discord.setup.secretRequired'));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const res = await setupDiscordOAuth({
        oauth_client_secret: oauthSecret.trim(),
        oauth_client_id: applicationId.trim() || undefined,
        oauth_redirect_uri: oauthRedirectUri.trim() || undefined,
      });
      setOauthRedirectUri(res.oauth_redirect_uri);
      setStatus((prev) =>
        prev
          ? { ...prev, bridge_configured: true, oauth_configured: true, public_url: prev.public_url }
          : prev,
      );
      setStep('manage');
    } catch (err) {
      setError(apiErrorMessage(err, tr('web.discord.setup.oauthFailed')));
    } finally {
      setBusy(false);
    }
  }

  function renderProgress() {
    const labels = [
      tr('web.discord.setup.progressIntro'),
      tr('web.discord.setup.progressApplication'),
      tr('web.discord.setup.progressBot'),
      tr('web.discord.setup.progressInvite'),
      tr('web.discord.setup.progressOauth'),
      tr('web.discord.setup.progressDone'),
    ];
    return (
      <div className="flex flex-wrap gap-2 text-xs">
        {labels.map((label, idx) => (
          <span
            key={label}
            className={
              idx === stepIndex
                ? 'text-[#5865F2] font-medium'
                : idx < stepIndex
                  ? 'text-emerald-400'
                  : 'text-bunny-muted'
            }
          >
            {idx + 1}. {label}
          </span>
        ))}
      </div>
    );
  }

  async function handleBridgeReload() {
    setBusy(true);
    setError(null);
    setBridgeReloadMsg(null);
    try {
      const res = await reloadDiscordBridge();
      if (res.bridge_starting) {
        setBridgeReloadMsg(tr('web.discord.setup.bridgeRestartStarting'));
      } else if (res.bridge_running) {
        setBridgeReloadMsg(tr('web.discord.setup.bridgeRestartOk'));
      } else {
        setBridgeReloadMsg(tr('web.discord.setup.bridgeRestartFailed'));
        setError(tr('web.discord.setup.bridgeRestartFailed'));
      }
    } catch (err) {
      setError(apiErrorMessage(err, tr('web.discord.setup.bridgeRestartFailed')));
    } finally {
      setBusy(false);
    }
  }

  function openBotEditor() {
    setBotToken('');
    setError(null);
    setStep('bot');
  }

  function openOAuthEditor() {
    setOauthSecret('');
    setError(null);
    setStep('oauth');
  }

  function statusBadge(configured: boolean) {
    return (
      <span className={configured ? 'text-emerald-400' : 'text-amber-400'}>
        {configured
          ? tr('web.discord.setup.statusConfigured')
          : tr('web.discord.setup.statusMissing')}
      </span>
    );
  }

  return (
    <div className="min-h-screen flex flex-col items-center p-6">
      <div className="w-full max-w-2xl flex items-center justify-between mb-8 gap-2">
        <button
          type="button"
          onClick={() => { location.href = '/'; }}
          className="text-sm text-bunny-muted hover:text-gray-200"
        >
          {tr('web.common.back')}
        </button>
        <h1 className="text-xl text-bunny-accent font-bold">{tr('web.discord.setup.title')}</h1>
        <AppTopBar />
      </div>

      <div className="w-full max-w-2xl border border-bunny-border rounded-lg p-6 bg-bunny-panel space-y-6">
        <p className="text-sm text-bunny-muted">{tr('web.discord.setup.subtitle')}</p>
        {renderProgress()}

        {loading ? (
          <p className="text-bunny-muted text-sm">{tr('web.common.loading')}</p>
        ) : null}

        {error ? (
          <p className="text-red-400 text-sm" role="alert">
            {error}
          </p>
        ) : null}

        {!loading && step === 'intro' ? (
          <div className="space-y-4">
            <p className="text-sm text-gray-200">{tr('web.discord.setup.introBody')}</p>
            <ul className="list-disc list-inside text-sm text-bunny-muted space-y-1">
              <li>{tr('web.discord.setup.introBulletBot')}</li>
              <li>{tr('web.discord.setup.introBulletOAuth')}</li>
              <li>{tr('web.discord.setup.introBulletLink')}</li>
            </ul>
            <button
              type="button"
              onClick={() => setStep('application')}
              className="px-4 py-2 rounded bg-[#5865F2] text-white text-sm font-medium hover:opacity-90"
            >
              {tr('web.discord.setup.start')}
            </button>
          </div>
        ) : null}

        {!loading && step === 'application' ? (
          <div className="space-y-4">
            <h2 className="text-sm font-medium text-gray-200">
              {tr('web.discord.setup.applicationTitle')}
            </h2>
            <p className="text-sm text-bunny-muted">
              {tr('web.discord.setup.portalHint')}{' '}
              <a
                href={PORTAL_URL}
                target="_blank"
                rel="noreferrer"
                className="text-[#5865F2] hover:underline"
              >
                {PORTAL_URL}
              </a>
            </p>
            <StepList
              items={[
                tr('web.discord.setup.applicationStep1'),
                tr('web.discord.setup.applicationStep2'),
                tr('web.discord.setup.applicationStep3'),
              ]}
            />
            <div className="flex flex-wrap gap-3">
              <a
                href={PORTAL_URL}
                target="_blank"
                rel="noreferrer"
                className="px-4 py-2 rounded border border-bunny-border text-gray-200 text-sm hover:bg-bunny-bg"
              >
                {tr('web.discord.setup.openPortal')}
              </a>
              <button
                type="button"
                onClick={() => setStep('bot')}
                className="px-4 py-2 rounded bg-[#5865F2] text-white text-sm font-medium hover:opacity-90"
              >
                {tr('web.discord.setup.continue')}
              </button>
            </div>
          </div>
        ) : null}

        {!loading && step === 'bot' ? (
          <form className="space-y-4" onSubmit={handleBotSubmit}>
            <h2 className="text-sm font-medium text-gray-200">{tr('web.discord.setup.botTitle')}</h2>
            {status?.bridge_configured ? (
              <p className="text-xs text-bunny-muted">{tr('web.discord.setup.botReconfigureHint')}</p>
            ) : null}
            <StepList
              items={[
                tr('web.discord.setup.botStep1'),
                tr('web.discord.setup.botStep2'),
                tr('web.discord.setup.botStep3'),
              ]}
            />
            <label className="block space-y-1">
              <span className="text-xs text-bunny-muted">{tr('web.discord.setup.appIdLabel')}</span>
              <input
                type="text"
                inputMode="numeric"
                value={applicationId}
                onChange={(e) => setApplicationId(e.target.value)}
                className="w-full px-3 py-2 rounded bg-bunny-bg border border-bunny-border text-sm text-gray-200"
                placeholder="123456789012345678"
                required
              />
            </label>
            <label className="block space-y-1">
              <span className="text-xs text-bunny-muted">{tr('web.discord.setup.botTokenLabel')}</span>
              <input
                type="password"
                value={botToken}
                onChange={(e) => setBotToken(e.target.value)}
                className="w-full px-3 py-2 rounded bg-bunny-bg border border-bunny-border text-sm text-gray-200"
                autoComplete="off"
                required
              />
            </label>
            <label className="block space-y-1">
              <span className="text-xs text-bunny-muted">{tr('web.discord.setup.guildIdLabel')}</span>
              <input
                type="text"
                inputMode="numeric"
                value={guildId}
                onChange={(e) => setGuildId(e.target.value)}
                className="w-full px-3 py-2 rounded bg-bunny-bg border border-bunny-border text-sm text-gray-200"
                placeholder={tr('web.discord.setup.guildIdPlaceholder')}
              />
            </label>
            <label className="block space-y-1">
              <span className="text-xs text-bunny-muted">{tr('web.discord.setup.publicUrlLabel')}</span>
              <input
                type="url"
                value={publicUrl}
                onChange={(e) => setPublicUrl(e.target.value)}
                className="w-full px-3 py-2 rounded bg-bunny-bg border border-bunny-border text-sm text-gray-200"
              />
              <span className="text-xs text-bunny-muted">{tr('web.discord.setup.publicUrlHint')}</span>
            </label>
            <div className="flex flex-wrap gap-3">
              <button
                type="button"
                onClick={() => setStep(status?.bridge_configured ? 'manage' : 'application')}
                className="px-4 py-2 rounded border border-bunny-border text-gray-200 text-sm hover:bg-bunny-bg"
              >
                {tr('web.common.back')}
              </button>
              <button
                type="submit"
                disabled={busy}
                className="px-4 py-2 rounded bg-[#5865F2] text-white text-sm font-medium hover:opacity-90 disabled:opacity-50"
              >
                {busy ? tr('web.common.loading') : tr('web.discord.setup.saveBot')}
              </button>
            </div>
          </form>
        ) : null}

        {!loading && step === 'invite' ? (
          <div className="space-y-4">
            <h2 className="text-sm font-medium text-gray-200">{tr('web.discord.setup.inviteTitle')}</h2>
            <p className="text-sm text-emerald-400">{tr('web.discord.setup.botSaved')}</p>
            <StepList
              items={[
                tr('web.discord.setup.inviteStep1'),
                tr('web.discord.setup.inviteStep2'),
                tr('web.discord.setup.inviteStep3'),
                tr('web.discord.setup.inviteStep4'),
              ]}
            />
            <a
              href={`${PORTAL_URL}?new_application=true`}
              target="_blank"
              rel="noreferrer"
              className="inline-block px-4 py-2 rounded border border-bunny-border text-gray-200 text-sm hover:bg-bunny-bg"
            >
              {tr('web.discord.setup.openUrlGenerator')}
            </a>
            <div className="flex flex-wrap gap-3">
              <button
                type="button"
                onClick={() => setStep('oauth')}
                className="px-4 py-2 rounded bg-[#5865F2] text-white text-sm font-medium hover:opacity-90"
              >
                {tr('web.discord.setup.continueOAuth')}
              </button>
            </div>
          </div>
        ) : null}

        {!loading && step === 'oauth' ? (
          <form className="space-y-4" onSubmit={handleOAuthSubmit}>
            <h2 className="text-sm font-medium text-gray-200">{tr('web.discord.setup.oauthTitle')}</h2>
            {status?.oauth_configured ? (
              <p className="text-xs text-bunny-muted">{tr('web.discord.setup.oauthReconfigureHint')}</p>
            ) : (
              <p className="text-sm text-bunny-muted">{tr('web.discord.setup.oauthIntro')}</p>
            )}
            <StepList
              items={[
                tr('web.discord.setup.oauthStep1'),
                tr('web.discord.setup.oauthStep2'),
                tr('web.discord.setup.oauthStep3'),
              ]}
            />
            <div className="rounded border border-bunny-border bg-bunny-bg p-3 space-y-2">
              <p className="text-xs text-bunny-muted">{tr('web.discord.setup.redirectLabel')}</p>
              <div className="flex flex-wrap items-center gap-2">
                <code className="text-xs text-gray-200 break-all">{oauthRedirectUri}</code>
                <button
                  type="button"
                  onClick={() => copyValue('redirect', oauthRedirectUri)}
                  className="px-2 py-1 rounded border border-bunny-border text-xs text-gray-200 hover:bg-bunny-panel"
                >
                  {copied === 'redirect'
                    ? tr('web.discord.setup.copied')
                    : tr('web.discord.setup.copy')}
                </button>
              </div>
            </div>
            <label className="block space-y-1">
              <span className="text-xs text-bunny-muted">{tr('web.discord.setup.oauthSecretLabel')}</span>
              <input
                type="password"
                value={oauthSecret}
                onChange={(e) => setOauthSecret(e.target.value)}
                className="w-full px-3 py-2 rounded bg-bunny-bg border border-bunny-border text-sm text-gray-200"
                autoComplete="off"
                required
              />
            </label>
            <div className="flex flex-wrap gap-3">
              <button
                type="button"
                onClick={() => setStep(status?.oauth_configured ? 'manage' : 'invite')}
                className="px-4 py-2 rounded border border-bunny-border text-gray-200 text-sm hover:bg-bunny-bg"
              >
                {tr('web.common.back')}
              </button>
              <button
                type="submit"
                disabled={busy}
                className="px-4 py-2 rounded bg-[#5865F2] text-white text-sm font-medium hover:opacity-90 disabled:opacity-50"
              >
                {busy ? tr('web.common.loading') : tr('web.discord.setup.saveOAuth')}
              </button>
            </div>
          </form>
        ) : null}

        {!loading && step === 'manage' && status ? (
          <div className="space-y-4">
            <h2 className="text-sm font-medium text-gray-200">{tr('web.discord.setup.manageTitle')}</h2>
            <p className="text-sm text-bunny-muted">{tr('web.discord.setup.manageIntro')}</p>
            <dl className="text-xs space-y-2 text-bunny-muted">
              <div className="flex flex-wrap justify-between gap-2">
                <dt>{tr('web.discord.setup.statusBridge')}</dt>
                <dd>{statusBadge(status.bridge_configured)}</dd>
              </div>
              <div className="flex flex-wrap justify-between gap-2">
                <dt>{tr('web.discord.setup.statusOAuth')}</dt>
                <dd>{statusBadge(status.oauth_configured)}</dd>
              </div>
              {status.application_id ? (
                <div className="flex flex-wrap justify-between gap-2">
                  <dt>{tr('web.discord.setup.appIdLabel')}</dt>
                  <dd className="text-gray-200">{status.application_id}</dd>
                </div>
              ) : null}
              {status.guild_id ? (
                <div className="flex flex-wrap justify-between gap-2">
                  <dt>{tr('web.discord.setup.guildIdLabel')}</dt>
                  <dd className="text-gray-200">{status.guild_id}</dd>
                </div>
              ) : null}
              <div className="flex flex-wrap justify-between gap-2">
                <dt>{tr('web.discord.setup.publicUrlLabel')}</dt>
                <dd className="text-gray-200 break-all">{status.public_url}</dd>
              </div>
            </dl>
            <p className="text-xs text-bunny-muted">{tr('web.discord.setup.restartReminder')}</p>
            {bridgeReloadMsg ? (
              <p className="text-xs text-emerald-400" role="status">
                {bridgeReloadMsg}
              </p>
            ) : null}
            <div className="flex flex-wrap gap-3">
              <button
                type="button"
                onClick={handleBridgeReload}
                disabled={busy || !status.bridge_configured}
                className="px-4 py-2 rounded bg-bunny-accent text-bunny-on-accent text-sm font-medium hover:opacity-90 disabled:opacity-50"
              >
                {busy ? tr('web.common.loading') : tr('web.discord.setup.restartBridge')}
              </button>
              <button
                type="button"
                onClick={openBotEditor}
                className="px-4 py-2 rounded bg-[#5865F2] text-white text-sm font-medium hover:opacity-90"
              >
                {tr('web.discord.setup.editBot')}
              </button>
              <button
                type="button"
                onClick={openOAuthEditor}
                className="px-4 py-2 rounded border border-[#5865F2] text-[#5865F2] text-sm font-medium hover:bg-bunny-bg"
              >
                {tr('web.discord.setup.editOAuth')}
              </button>
              <button
                type="button"
                onClick={() => setStep('intro')}
                className="px-4 py-2 rounded border border-bunny-border text-gray-200 text-sm hover:bg-bunny-bg"
              >
                {tr('web.discord.setup.startSetup')}
              </button>
              <button
                type="button"
                onClick={() => { location.href = '/'; }}
                className="px-4 py-2 rounded border border-bunny-border text-gray-200 text-sm hover:bg-bunny-bg"
              >
                {tr('web.discord.setup.backHome')}
              </button>
            </div>
          </div>
        ) : null}

        {!loading && step === 'done' ? (
          <div className="space-y-4">
            <p className="text-sm text-emerald-400">{tr('web.discord.setup.doneTitle')}</p>
            <p className="text-sm text-bunny-muted">{tr('web.discord.setup.doneBody')}</p>
            <StepList
              items={[
                tr('web.discord.setup.doneStep1'),
                tr('web.discord.setup.doneStep2'),
                tr('web.discord.setup.doneStep3'),
              ]}
            />
            <button
              type="button"
              onClick={() => { location.href = '/'; }}
              className="px-4 py-2 rounded bg-bunny-accent text-bunny-on-accent text-sm font-medium hover:opacity-90"
            >
              {tr('web.discord.setup.backHome')}
            </button>
            <button
              type="button"
              onClick={() => setStep('manage')}
              className="px-4 py-2 rounded border border-bunny-border text-gray-200 text-sm hover:bg-bunny-bg"
            >
              {tr('web.discord.account.manageButton')}
            </button>
          </div>
        ) : null}
      </div>
    </div>
  );
}
