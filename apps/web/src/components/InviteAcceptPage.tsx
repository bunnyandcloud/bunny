import { useState } from 'react';

/** Shown when an invite link is opened while already signed in as someone else. */
export default function InviteAcceptPage(props: {
  currentEmail: string;
  inviteEmail: string | null;
  onSignOut: () => Promise<void>;
}) {
  const { currentEmail, inviteEmail, onSignOut } = props;
  const [busy, setBusy] = useState(false);

  return (
    <div className="min-h-screen flex items-center justify-center p-4">
      <div className="w-full max-w-md bg-bunny-panel border border-bunny-border rounded-lg p-8 space-y-4">
        <h1 className="text-2xl font-bold text-bunny-accent">Invitation pending</h1>
        <p className="text-bunny-muted text-sm">
          You are signed in as <strong className="text-gray-200">{currentEmail}</strong>.
          {inviteEmail ? (
            <>
              {' '}
              This invitation is for <strong className="text-gray-200">{inviteEmail}</strong>.
            </>
          ) : null}
        </p>
        <p className="text-bunny-muted text-sm">
          Sign out to create or use the invited account.
        </p>
        <button
          type="button"
          disabled={busy}
          onClick={async () => {
            setBusy(true);
            await onSignOut();
            location.reload();
          }}
          className="w-full py-2 bg-bunny-accent text-bunny-bg font-semibold rounded hover:opacity-90 disabled:opacity-50"
        >
          {busy ? 'Signing out…' : 'Sign out & accept invitation'}
        </button>
      </div>
    </div>
  );
}
