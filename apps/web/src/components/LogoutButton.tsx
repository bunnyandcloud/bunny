import { useState } from 'react';
import { useT } from '../i18n';
import { useAuth } from '../store/auth';

interface Props {
  className?: string;
}

export default function LogoutButton({ className = '' }: Props) {
  const logout = useAuth((s) => s.logout);
  const tr = useT();
  const [busy, setBusy] = useState(false);

  async function handleLogout() {
    setBusy(true);
    try {
      await logout();
      location.href = '/';
    } catch {
      setBusy(false);
    }
  }

  return (
    <button
      type="button"
      onClick={handleLogout}
      disabled={busy}
      className={
        className ||
        'text-sm text-bunny-muted hover:text-gray-200 disabled:opacity-50'
      }
    >
      {busy ? tr('web.common.signingOut') : tr('web.common.signOut')}
    </button>
  );
}
