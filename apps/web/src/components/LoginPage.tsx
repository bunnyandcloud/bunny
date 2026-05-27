import { FormEvent, useState } from 'react';
import { apiErrorMessage, type LoginResponse } from '../lib/api';
import { useAuth } from '../store/auth';

export default function LoginPage() {
  const login = useAuth((s) => s.login);
  const completeMfa = useAuth((s) => s.completeMfa);
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [mfaChallenge, setMfaChallenge] = useState<{
    token: string;
    email: string;
  } | null>(null);
  const [mfaCode, setMfaCode] = useState('');
  const [useRecovery, setUseRecovery] = useState(false);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  function redirectAfterAuth() {
    const next = new URLSearchParams(location.search).get('next');
    if (next) location.href = next;
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
      setError(apiErrorMessage(err, 'Login failed'));
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
      setError(
        apiErrorMessage(
          err,
          'Invalid code. Check your authenticator app or recovery code and try again.',
        ),
      );
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
          <h1 className="text-2xl font-bold text-bunny-accent">Two-factor authentication</h1>
          <p className="text-bunny-muted text-sm">
            Enter the code from your authenticator app for {mfaChallenge.email}.
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
            placeholder={useRecovery ? 'Recovery code' : '6-digit code'}
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
            {useRecovery ? 'Use authenticator code instead' : 'Use a recovery code'}
          </button>
          <button
            type="submit"
            disabled={loading}
            className="w-full py-2 bg-bunny-accent text-bunny-bg font-semibold rounded hover:opacity-90 disabled:opacity-50"
          >
            {loading ? 'Verifying…' : 'Verify'}
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
            Back to sign in
          </button>
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
        <h1 className="text-2xl font-bold text-bunny-accent">bunny</h1>
        <p className="text-bunny-muted text-sm">
          Authentication required. No anonymous access.
        </p>
        {error && <p className="text-red-400 text-sm">{error}</p>}
        <input
          type="email"
          placeholder="Email"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          className="w-full px-3 py-2 bg-bunny-bg border border-bunny-border rounded"
          required
        />
        <input
          type="password"
          placeholder="Password"
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
          {loading ? 'Signing in…' : 'Sign in'}
        </button>
      </form>
    </div>
  );
}
