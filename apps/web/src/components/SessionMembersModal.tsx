import { FormEvent, useEffect, useMemo, useState } from 'react';
import {
  apiErrorMessage,
  createInvitation,
  listSessionMembers,
  removeSessionMember,
  updateSessionMember,
} from '../lib/api';

type Role = 'owner' | 'admin' | 'editor' | 'viewer';

const ROLE_OPTIONS: Role[] = ['owner', 'admin', 'editor', 'viewer'];

export default function SessionMembersModal(props: {
  open: boolean;
  sessionId: string;
  onClose: () => void;
}) {
  const { open, sessionId, onClose } = props;
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [inviteEmail, setInviteEmail] = useState('');
  const [inviteRole, setInviteRole] = useState<Role>('viewer');
  const [inviteToken, setInviteToken] = useState<string | null>(null);
  const [members, setMembers] = useState<
    Array<{ user_id: string; email: string; role: string }>
  >([]);

  const inviteLink = useMemo(() => {
    if (!inviteToken) return null;
    const next = `/s/${sessionId}`;
    const qs = new URLSearchParams({
      invite: inviteToken,
      email: inviteEmail.trim(),
      next,
    });
    return `/login?${qs}`;
  }, [inviteToken, inviteEmail, sessionId]);

  useEffect(() => {
    if (!open) return;
    setInviteToken(null);
    setError('');
    setLoading(true);
    listSessionMembers(sessionId)
      .then(setMembers)
      .catch((e) => setError(apiErrorMessage(e, 'Cannot load members')))
      .finally(() => setLoading(false));
  }, [open, sessionId]);

  if (!open) return null;

  async function onInviteSubmit(e: FormEvent) {
    e.preventDefault();
    setLoading(true);
    setError('');
    setInviteToken(null);
    try {
      const res = await createInvitation(sessionId, inviteEmail.trim(), inviteRole);
      setInviteToken(res.token);
      // Refresh member list (invite is not a member yet, but useful to detect existing users)
      setMembers(await listSessionMembers(sessionId));
    } catch (err) {
      setError(apiErrorMessage(err, 'Invite failed'));
    } finally {
      setLoading(false);
    }
  }

  async function onChangeRole(userId: string, role: string) {
    setLoading(true);
    setError('');
    try {
      await updateSessionMember(sessionId, userId, role);
      setMembers(await listSessionMembers(sessionId));
    } catch (err) {
      setError(apiErrorMessage(err, 'Update role failed'));
    } finally {
      setLoading(false);
    }
  }

  async function onRemove(userId: string) {
    setLoading(true);
    setError('');
    try {
      await removeSessionMember(sessionId, userId);
      setMembers(await listSessionMembers(sessionId));
    } catch (err) {
      setError(apiErrorMessage(err, 'Remove member failed'));
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60">
      <div className="w-full max-w-2xl bg-bunny-panel border border-bunny-border rounded-lg p-6 space-y-4">
        <div className="flex items-start justify-between gap-4">
          <div>
            <h2 className="text-lg font-semibold text-gray-200">Session members</h2>
            <p className="text-xs text-bunny-muted">
              Invite teammates and set their role (Owner/Admin/Editor/Viewer).
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

        <form onSubmit={onInviteSubmit} className="flex flex-wrap items-end gap-2">
          <label className="text-xs text-bunny-muted flex flex-col gap-1">
            Email
            <input
              type="email"
              value={inviteEmail}
              onChange={(e) => setInviteEmail(e.target.value)}
              className="px-3 py-2 bg-bunny-bg border border-bunny-border rounded"
              placeholder="teammate@company.com"
              disabled={loading}
              required
            />
          </label>
          <label className="text-xs text-bunny-muted flex flex-col gap-1">
            Role
            <select
              value={inviteRole}
              onChange={(e) => setInviteRole(e.target.value as Role)}
              className="px-3 py-2 bg-bunny-bg border border-bunny-border rounded"
              disabled={loading}
            >
              {ROLE_OPTIONS.map((r) => (
                <option key={r} value={r}>
                  {r}
                </option>
              ))}
            </select>
          </label>
          <button
            type="submit"
            disabled={loading}
            className="px-3 py-2 bg-bunny-accent text-bunny-bg rounded font-semibold disabled:opacity-50"
          >
            Invite
          </button>
        </form>

        {inviteToken && inviteLink ? (
          <div className="text-xs border border-bunny-border rounded p-3 bg-bunny-bg space-y-2">
            <p className="text-bunny-muted">
              Invitation token (send this link to the recipient):
            </p>
            <div className="flex items-center gap-2">
              <input
                readOnly
                value={location.origin + inviteLink}
                className="flex-1 px-2 py-1 bg-black/20 border border-bunny-border rounded font-mono text-[11px]"
              />
              <button
                type="button"
                className="text-bunny-accent hover:underline"
                onClick={async () => {
                  await navigator.clipboard.writeText(location.origin + inviteLink);
                }}
              >
                Copy
              </button>
            </div>
          </div>
        ) : null}

        <div className="border-t border-bunny-border pt-3">
          <h3 className="text-sm font-medium text-gray-200 mb-2">Members</h3>
          {loading && members.length === 0 ? (
            <p className="text-sm text-bunny-muted">Loading…</p>
          ) : (
            <div className="space-y-2">
              {members.map((m) => (
                <div
                  key={m.user_id}
                  className="flex items-center justify-between gap-2 p-2 rounded border border-bunny-border bg-bunny-bg"
                >
                  <div className="min-w-0">
                    <p className="text-sm text-gray-200 truncate">{m.email}</p>
                    <p className="text-[11px] text-bunny-muted font-mono truncate">
                      {m.user_id}
                    </p>
                  </div>
                  <div className="flex items-center gap-2 shrink-0">
                    <select
                      value={m.role}
                      onChange={(e) => void onChangeRole(m.user_id, e.target.value)}
                      className="text-xs px-2 py-1 bg-bunny-panel border border-bunny-border rounded"
                      disabled={loading}
                    >
                      {ROLE_OPTIONS.map((r) => (
                        <option key={r} value={r}>
                          {r}
                        </option>
                      ))}
                    </select>
                    <button
                      type="button"
                      onClick={() => void onRemove(m.user_id)}
                      className="text-xs px-2 py-1 rounded border border-red-500/40 text-red-300 hover:bg-red-500/10 disabled:opacity-50"
                      disabled={loading}
                    >
                      Remove
                    </button>
                  </div>
                </div>
              ))}
              {members.length === 0 ? (
                <p className="text-sm text-bunny-muted">No members found.</p>
              ) : null}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

