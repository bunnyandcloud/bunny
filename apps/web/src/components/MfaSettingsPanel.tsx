import { FormEvent, useCallback, useEffect, useState } from 'react';
import {
  mfaDisable,
  mfaEnable,
  mfaRegenerateRecovery,
  mfaSetup,
  mfaStatus,
} from '../lib/api';

interface Props {
  email: string;
}

type Step = 'idle' | 'setup' | 'recovery' | 'disable';

export default function MfaSettingsPanel({ email }: Props) {
  const [enabled, setEnabled] = useState(false);
  const [recoveryRemaining, setRecoveryRemaining] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [step, setStep] = useState<Step>('idle');
  const [password, setPassword] = useState('');
  const [confirmCode, setConfirmCode] = useState('');
  const [otpauthUri, setOtpauthUri] = useState('');
  const [secretBase32, setSecretBase32] = useState('');
  const [recoveryCodes, setRecoveryCodes] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(() => {
    setLoading(true);
    mfaStatus()
      .then((s) => {
        setEnabled(s.enabled);
        setRecoveryRemaining(s.recovery_remaining);
      })
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  async function withPassword<T>(fn: (password?: string) => Promise<T>): Promise<T> {
    try {
      return await fn();
    } catch (e) {
      const msg = String(e);
      if (msg.includes('recent authentication') || msg.includes('FORBIDDEN')) {
        if (!password) {
          setError('Enter your password to continue.');
          throw e;
        }
        return fn(password);
      }
      throw e;
    }
  }

  async function startSetup(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const res = await withPassword((pw) => mfaSetup(pw));
      setOtpauthUri(res.otpauth_uri);
      setSecretBase32(res.secret_base32);
      setStep('setup');
      setConfirmCode('');
    } catch (err) {
      if (String(err) !== String(error)) setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function confirmEnable(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const res = await withPassword((pw) => mfaEnable(confirmCode, pw));
      setRecoveryCodes(res.recovery_codes);
      setStep('recovery');
      refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function handleDisable(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await withPassword((pw) => mfaDisable(confirmCode, pw));
      setStep('idle');
      setConfirmCode('');
      setPassword('');
      refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function handleRegenerate(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const res = await withPassword((pw) => mfaRegenerateRecovery(confirmCode, pw));
      setRecoveryCodes(res.recovery_codes);
      setStep('recovery');
      refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  if (loading) {
    return <p className="text-bunny-muted text-sm">Loading security settings…</p>;
  }

  const qrUrl = otpauthUri
    ? `https://api.qrserver.com/v1/create-qr-code/?size=200x200&data=${encodeURIComponent(otpauthUri)}`
    : '';

  return (
    <div className="w-full max-w-lg space-y-6">
      <div>
        <h2 className="text-lg font-semibold text-gray-100">Two-factor authentication</h2>
        <p className="text-bunny-muted text-sm mt-1">
          Compatible with Google Authenticator, Microsoft Authenticator, GitHub Mobile, and other
          TOTP apps.
        </p>
      </div>

      {error && <p className="text-red-400 text-sm">{error}</p>}

      <p className="text-sm">
        Status:{' '}
        <span className={enabled ? 'text-emerald-400' : 'text-bunny-muted'}>
          {enabled ? 'Enabled' : 'Disabled'}
        </span>
        {enabled && (
          <span className="text-bunny-muted"> · {recoveryRemaining} recovery codes left</span>
        )}
      </p>

      {!enabled && step !== 'setup' && (
        <form onSubmit={startSetup} className="space-y-3 border border-bunny-border rounded-lg p-4">
          <p className="text-sm text-bunny-muted">
            Re-enter your password if your session is older than 5 minutes.
          </p>
          <input
            type="password"
            placeholder="Password (if prompted)"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            className="w-full px-3 py-2 bg-bunny-bg border border-bunny-border rounded"
          />
          <button
            type="submit"
            disabled={busy}
            className="px-4 py-2 bg-bunny-accent text-bunny-bg rounded text-sm font-medium disabled:opacity-50"
          >
            Set up authenticator
          </button>
        </form>
      )}

      {step === 'setup' && (
        <form onSubmit={confirmEnable} className="space-y-4 border border-bunny-border rounded-lg p-4">
          {qrUrl && (
            <img src={qrUrl} alt="QR code for authenticator app" className="mx-auto rounded bg-white p-2" />
          )}
          <p className="text-xs text-amber-400">
            Never share this secret. It is shown only during setup.
          </p>
          <div className="text-xs font-mono break-all text-bunny-muted">
            Manual entry: {secretBase32}
          </div>
          <input
            type="text"
            inputMode="numeric"
            placeholder="6-digit code from app"
            value={confirmCode}
            onChange={(e) => setConfirmCode(e.target.value)}
            className="w-full px-3 py-2 bg-bunny-bg border border-bunny-border rounded font-mono"
            required
          />
          <input
            type="password"
            placeholder="Password (if prompted)"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            className="w-full px-3 py-2 bg-bunny-bg border border-bunny-border rounded"
          />
          <button
            type="submit"
            disabled={busy}
            className="w-full py-2 bg-bunny-accent text-bunny-bg rounded font-medium disabled:opacity-50"
          >
            Confirm and enable
          </button>
        </form>
      )}

      {step === 'recovery' && recoveryCodes.length > 0 && (
        <div className="border border-amber-600/50 rounded-lg p-4 space-y-3 bg-amber-950/20">
          <p className="text-sm text-amber-200 font-medium">
            Save these recovery codes now. They will not be shown again.
          </p>
          <ul className="font-mono text-sm space-y-1">
            {recoveryCodes.map((c) => (
              <li key={c}>{c}</li>
            ))}
          </ul>
          <button
            type="button"
            className="text-sm text-bunny-accent hover:underline"
            onClick={() => {
              setStep('idle');
              setRecoveryCodes([]);
            }}
          >
            I have saved them
          </button>
        </div>
      )}

      {enabled && step !== 'setup' && (
        <div className="space-y-4 border border-bunny-border rounded-lg p-4">
          <form onSubmit={handleRegenerate} className="space-y-2">
            <p className="text-sm font-medium text-gray-200">Regenerate recovery codes</p>
            <input
              type="text"
              placeholder="Current TOTP code"
              value={confirmCode}
              onChange={(e) => setConfirmCode(e.target.value)}
              className="w-full px-3 py-2 bg-bunny-bg border border-bunny-border rounded font-mono"
            />
            <input
              type="password"
              placeholder="Password (if prompted)"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              className="w-full px-3 py-2 bg-bunny-bg border border-bunny-border rounded"
            />
            <button
              type="submit"
              disabled={busy}
              className="text-sm text-bunny-accent hover:underline disabled:opacity-50"
            >
              Generate new recovery codes
            </button>
          </form>
          <hr className="border-bunny-border" />
          <form onSubmit={handleDisable} className="space-y-2">
            <p className="text-sm font-medium text-red-300">Disable MFA</p>
            <input
              type="text"
              placeholder="Current TOTP code"
              value={confirmCode}
              onChange={(e) => setConfirmCode(e.target.value)}
              className="w-full px-3 py-2 bg-bunny-bg border border-bunny-border rounded font-mono"
              required
            />
            <button
              type="submit"
              disabled={busy}
              className="px-4 py-2 border border-red-500/50 text-red-300 rounded text-sm disabled:opacity-50"
            >
              Disable two-factor authentication
            </button>
          </form>
        </div>
      )}

      <p className="text-xs text-bunny-muted">
        Account: {email}
      </p>
    </div>
  );
}
