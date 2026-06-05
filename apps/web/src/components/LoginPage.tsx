import { FormEvent, useEffect, useMemo, useState } from 'react';
import { acceptInvitation, apiErrorMessage, type LoginResponse } from '../lib/api';
import { useT } from '../i18n';
import { useAuth } from '../store/auth';

function readInviteParams() {
  const params = new URLSearchParams(location.search);
  return {
    inviteToken: params.get('invite'),
    inviteEmail: params.get('email') ?? '',
    next: params.get('next'),
  };
}

export default function LoginPage() {
  const tr = useT();
  const login = useAuth((s) => s.login);
  const completeMfa = useAuth((s) => s.completeMfa);
  const check = useAuth((s) => s.check);

  const inviteParams = useMemo(readInviteParams, []);
  const inviteToken = inviteParams.inviteToken;

  const [email, setEmail] = useState(inviteParams.inviteEmail);
  const [password, setPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [mfaChallenge, setMfaChallenge] = useState<{
    token: string;
    email: string;
  } | null>(null);
  const [mfaCode, setMfaCode] = useState('');
  const [useRecovery, setUseRecovery] = useState(false);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    const { inviteEmail } = readInviteParams();
    if (inviteEmail) {
      setEmail(inviteEmail);
    }
  }, []);

  function redirectAfterAuth() {
    const next = readInviteParams().next;
    if (next) location.href = next;
  }

  async function onAcceptInviteSubmit(e: FormEvent) {
    e.preventDefault();
    if (!inviteToken || loading) return;
    if (password !== confirmPassword) {
      setError(tr('web.login.passwordMismatch'));
      return;
    }
    if (password.length < 8) {
      setError(tr('web.login.passwordMinLength'));
      return;
    }
    setLoading(true);
    setError('');
    try {
      const res = await acceptInvitation({
        token: inviteToken,
        email: email.trim(),
        password,
      });
      await check();
      const next = readInviteParams().next;
      location.href = next || (res.session_id ? `/s/${res.session_id}` : '/');
    } catch (err) {
      setError(apiErrorMessage(err, tr('web.login.inviteFailed')));
    } finally {
      setLoading(false);
    }
  }

  async function onPasswordSubmit(e: FormEvent) {
    e.preventDefault();
    setLoading(true);
    setError('');
    try {
      const result: LoginResponse = await login(email, password);
      if (result.mfa_required) {
        setMfaChallenge({
          token: result.mfa_challenge_token,
          email: result.email,
        });
      } else {
        redirectAfterAuth();
      }
    } catch (err) {
      setError(apiErrorMessage(err, tr('web.login.failed')));
    } finally {
      setLoading(false);
    }
  }

  async function onMfaSubmit(e: FormEvent) {
    e.preventDefault();
    if (!mfaChallenge || loading) return;
    setLoading(true);
    setError('');
    try {
      await completeMfa(mfaCode.trim(), mfaChallenge.token);
      redirectAfterAuth();
    } catch (err) {
      setError(apiErrorMessage(err, tr('web.login.mfaInvalid')));
    } finally {
      setLoading(false);
    }
  }

  if (mfaChallenge) {
    return (
      <div className="min-h-screen flex items-center justify-center p-4">
        <form
          onSubmit={onMfaSubmit}
          className="w-full max-w-md bg-bunny-panel border border-bunny-border rounded-lg p-8 space-y-4"
        >
          <h1 className="text-2xl font-bold text-bunny-accent">
            {tr('web.login.mfaTitle')}
          </h1>
          <p className="text-bunny-muted text-sm">
            {tr('web.login.mfaHint', { email: mfaChallenge.email })}
          </p>
          {error ? (
            <p className="text-red-400 text-sm" role="alert">
              {error}
            </p>
          ) : null}
          <input
            type="text"
            inputMode={useRecovery ? 'text' : 'numeric'}
            autoComplete="one-time-code"
            placeholder={
              useRecovery ? tr('web.login.recoveryCode') : tr('web.login.sixDigitCode')
            }
            value={mfaCode}
            onChange={(e) => setMfaCode(e.target.value)}
            disabled={loading}
            className="w-full px-3 py-2 bg-bunny-bg border border-bunny-border rounded font-mono disabled:opacity-50"
            required
          />
          <button
            type="button"
            className="text-sm text-bunny-accent hover:underline"
            onClick={() => {
              setUseRecovery(!useRecovery);
              setMfaCode('');
            }}
          >
            {useRecovery ? tr('web.login.useAuthenticator') : tr('web.login.useRecovery')}
          </button>
          <button
            type="submit"
            disabled={loading}
            className="w-full py-2 bg-bunny-accent text-bunny-bg font-semibold rounded hover:opacity-90 disabled:opacity-50"
          >
            {loading ? tr('web.login.verifying') : tr('web.login.verify')}
          </button>
          <button
            type="button"
            className="w-full py-2 text-bunny-muted text-sm hover:text-gray-200"
            onClick={() => {
              setMfaChallenge(null);
              setMfaCode('');
              setUseRecovery(false);
              setError('');
              setLoading(false);
            }}
          >
            {tr('web.login.backToSignIn')}
          </button>
        </form>
      </div>
    );
  }

  if (inviteToken) {
    return (
      <div className="min-h-screen flex items-center justify-center p-4">
        <form
          onSubmit={onAcceptInviteSubmit}
          className="w-full max-w-md bg-bunny-panel border border-bunny-border rounded-lg p-8 space-y-4"
        >
          <h1 className="text-2xl font-bold text-bunny-accent">
            {tr('web.login.inviteTitle')}
          </h1>
          <p className="text-bunny-muted text-sm">{tr('web.login.inviteSubtitle')}</p>
          {error && <p className="text-red-400 text-sm">{error}</p>}
          <label className="block text-xs text-bunny-muted">
            {tr('web.common.email')}
            <input
              type="email"
              placeholder={tr('web.common.email')}
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              readOnly={Boolean(inviteParams.inviteEmail)}
              className="mt-1 w-full px-3 py-2 bg-bunny-bg border border-bunny-border rounded disabled:opacity-70"
              required
            />
          </label>
          <label className="block text-xs text-bunny-muted">
            {tr('web.login.choosePassword')}
            <input
              type="password"
              placeholder={tr('web.login.newPasswordPlaceholder')}
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              autoComplete="new-password"
              className="mt-1 w-full px-3 py-2 bg-bunny-bg border border-bunny-border rounded"
              required
              minLength={8}
            />
          </label>
          <label className="block text-xs text-bunny-muted">
            {tr('web.login.confirmPassword')}
            <input
              type="password"
              placeholder={tr('web.login.confirmPassword')}
              value={confirmPassword}
              onChange={(e) => setConfirmPassword(e.target.value)}
              autoComplete="new-password"
              className="mt-1 w-full px-3 py-2 bg-bunny-bg border border-bunny-border rounded"
              required
              minLength={8}
            />
          </label>
          <button
            type="submit"
            disabled={loading}
            className="w-full py-2 bg-bunny-accent text-bunny-bg font-semibold rounded hover:opacity-90 disabled:opacity-50"
          >
            {loading ? tr('web.login.creatingAccount') : tr('web.login.createAndJoin')}
          </button>
          <p className="text-xs text-bunny-muted text-center">
            {tr('web.login.alreadyHaveAccount')}{' '}
            <button
              type="button"
              className="text-bunny-accent hover:underline"
              onClick={() => {
                location.href = '/';
              }}
            >
              {tr('web.login.signInInstead')}
            </button>
          </p>
        </form>
      </div>
    );
  }

  return (
    <div className="min-h-screen flex items-center justify-center p-4">
      <form
        onSubmit={onPasswordSubmit}
        className="w-full max-w-md bg-bunny-panel border border-bunny-border rounded-lg p-8 space-y-4"
      >
        <h1 className="text-2xl font-bold text-bunny-accent">{tr('web.login.title')}</h1>
        <p className="text-bunny-muted text-sm">{tr('web.login.subtitle')}</p>
        {error && <p className="text-red-400 text-sm">{error}</p>}
        <input
          type="email"
          placeholder={tr('web.common.email')}
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          className="w-full px-3 py-2 bg-bunny-bg border border-bunny-border rounded"
          required
        />
        <input
          type="password"
          placeholder={tr('web.common.password')}
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          className="w-full px-3 py-2 bg-bunny-bg border border-bunny-border rounded"
          required
        />
        <button
          type="submit"
          disabled={loading}
          className="w-full py-2 bg-bunny-accent text-bunny-bg font-semibold rounded hover:opacity-90 disabled:opacity-50"
        >
          {loading ? tr('web.login.signingIn') : tr('web.login.signIn')}
        </button>
      </form>
    </div>
  );
}
