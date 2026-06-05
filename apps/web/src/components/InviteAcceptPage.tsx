import { useState } from 'react';
import { useT } from '../i18n';

export default function InviteAcceptPage(props: {
  currentEmail: string;
  inviteEmail: string | null;
  onSignOut: () => Promise<void>;
}) {
  const { currentEmail, inviteEmail, onSignOut } = props;
  const tr = useT();
  const [busy, setBusy] = useState(false);

  return (
    <div className="min-h-screen flex items-center justify-center p-4">
      <div className="w-full max-w-md bg-bunny-panel border border-bunny-border rounded-lg p-8 space-y-4">
        <h1 className="text-2xl font-bold text-bunny-accent">
          {tr('web.invite.pendingTitle')}
        </h1>
        <p className="text-bunny-muted text-sm">
          {tr('web.invite.signedInAs', { email: currentEmail })}
          {inviteEmail ? (
            <> {tr('web.invite.forEmail', { email: inviteEmail })}</>
          ) : null}
        </p>
        <p className="text-bunny-muted text-sm">{tr('web.invite.signOutHint')}</p>
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
          {busy ? tr('web.common.signingOut') : tr('web.invite.signOutAccept')}
        </button>
      </div>
    </div>
  );
}
