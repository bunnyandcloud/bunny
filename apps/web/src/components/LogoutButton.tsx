import { useState } from 'react';
import { useAuth } from '../store/auth';

interface Props {
  className?: string;
}

export default function LogoutButton({ className = '' }: Props) {
  const logout = useAuth((s) => s.logout);
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
      {busy ? 'Signing out…' : 'Sign out'}
    </button>
  );
}
