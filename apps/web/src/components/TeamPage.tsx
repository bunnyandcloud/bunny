import { FormEvent, useCallback, useEffect, useMemo, useState } from 'react';
import {
  apiErrorMessage,
  createTeamUser,
  listUsersAdmin,
  revokeTeamUser,
  updateUserAdmin,
  type TeamUser,
} from '../lib/api';
import LogoutButton from './LogoutButton';

type SessionRole = 'admin' | 'editor' | 'viewer';
const SESSION_ROLES: SessionRole[] = ['admin', 'editor', 'viewer'];

export default function TeamPage(props: { email: string }) {
  const { email } = props;
  const [users, setUsers] = useState<TeamUser[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState<string | null>(null);
  const [inviteEmail, setInviteEmail] = useState('');
  const [inviteRole, setInviteRole] = useState<SessionRole>('viewer');
  const [inviteCreateSessions, setInviteCreateSessions] = useState(false);
  const [inviteClaude, setInviteClaude] = useState(false);
  const [inviteVault, setInviteVault] = useState(false);
  /** Set only after a successful API invite — not while typing. */
  const [createdInvite, setCreatedInvite] = useState<{
    token: string;
    email: string;
  } | null>(null);

  const inviteLink = useMemo(() => {
    if (!createdInvite) return null;
    const qs = new URLSearchParams({
      invite: createdInvite.token,
      email: createdInvite.email,
    });
    // Prefer the host the user actually uses in the browser (not 0.0.0.0 from origin).
    const base =
      location.hostname === '0.0.0.0' || location.hostname === '127.0.0.1'
        ? `http://127.0.0.1:${location.port || '7681'}`
        : location.origin;
    return `${base}/login?${qs}`;
  }, [createdInvite]);

  const refresh = useCallback(() => {
    setLoading(true);
    setError(null);
    listUsersAdmin()
      .then(setUsers)
      .catch((e) => setError(apiErrorMessage(e, 'Cannot load users')))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  async function onInviteSubmit(e: FormEvent) {
    e.preventDefault();
    const email = inviteEmail.trim();
    if (!email) return;
    setSaving('invite');
    setError(null);
    try {
      const res = await createTeamUser({
        email,
        default_session_role: inviteRole,
        can_create_sessions: inviteCreateSessions,
        can_install_claude: inviteClaude,
        can_manage_vault: inviteVault,
      });
      setCreatedInvite({ token: res.token, email });
      setInviteEmail('');
    } catch (err) {
      setError(apiErrorMessage(err, 'Invite failed'));
    } finally {
      setSaving(null);
    }
  }

  async function saveUser(
    user: TeamUser,
    patch: Partial<{
      can_install_claude: boolean;
      can_manage_vault: boolean;
      can_create_sessions: boolean;
      default_session_role: string;
    }>,
  ) {
    setSaving(user.id);
    setError(null);
    try {
      await updateUserAdmin({
        user_id: user.id,
        can_install_claude: patch.can_install_claude ?? user.can_install_claude,
        can_manage_vault: patch.can_manage_vault ?? user.can_manage_vault,
        can_create_sessions: patch.can_create_sessions ?? user.can_create_sessions,
        default_session_role: patch.default_session_role ?? user.default_session_role,
      });
      await refresh();
    } catch (err) {
      setError(apiErrorMessage(err, 'Update failed'));
    } finally {
      setSaving(null);
    }
  }

  async function removeUser(user: TeamUser) {
    if (
      !window.confirm(
        `Remove ${user.email}? They will be signed out and lose access to all sessions.`,
      )
    ) {
      return;
    }
    setSaving(user.id);
    setError(null);
    try {
      await revokeTeamUser(user.id);
      await refresh();
    } catch (err) {
      setError(apiErrorMessage(err, 'Remove failed'));
    } finally {
      setSaving(null);
    }
  }

  return (
    <div className="min-h-screen flex flex-col items-center gap-6 p-6">
      <div className="w-full max-w-5xl flex items-center justify-between">
        <button
          type="button"
          onClick={() => {
            location.href = '/';
          }}
          className="text-bunny-accent font-bold hover:opacity-80"
        >
          ← Home
        </button>
        <LogoutButton />
      </div>

      <div className="w-full max-w-5xl space-y-1">
        <h1 className="text-xl text-bunny-accent">Team</h1>
        <p className="text-bunny-muted text-sm">
          Manage users, global permissions, and default session roles. Signed in as {email}.
        </p>
      </div>

      {error ? (
        <p className="text-red-400 text-sm max-w-5xl w-full" role="alert">
          {error}
        </p>
      ) : null}

      <form
        onSubmit={onInviteSubmit}
        className="w-full max-w-5xl border border-bunny-border rounded p-4 space-y-3 bg-bunny-panel"
      >
        <h2 className="text-sm font-medium text-gray-200">Invite user</h2>
        <div className="flex flex-wrap gap-3 items-end">
          <label className="block text-xs text-bunny-muted flex-1 min-w-[12rem]">
            Email
            <input
              type="email"
              value={inviteEmail}
              onChange={(e) => setInviteEmail(e.target.value)}
              required
              className="mt-1 w-full px-3 py-2 bg-bunny-bg border border-bunny-border rounded text-sm"
              placeholder="user@example.com"
            />
          </label>
          <label className="block text-xs text-bunny-muted">
            Default session role
            <select
              value={inviteRole}
              onChange={(e) => setInviteRole(e.target.value as SessionRole)}
              className="mt-1 block px-3 py-2 bg-bunny-bg border border-bunny-border rounded text-sm"
            >
              {SESSION_ROLES.map((r) => (
                <option key={r} value={r}>
                  {r}
                </option>
              ))}
            </select>
          </label>
          <button
            type="submit"
            disabled={saving === 'invite'}
            className="px-4 py-2 rounded bg-bunny-accent text-bunny-bg text-sm font-medium disabled:opacity-50"
          >
            {saving === 'invite' ? 'Creating…' : 'Create invite link'}
          </button>
        </div>
        <div className="flex flex-wrap gap-4 text-xs text-bunny-muted">
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={inviteCreateSessions}
              onChange={(e) => setInviteCreateSessions(e.target.checked)}
            />
            Create sessions
          </label>
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={inviteClaude}
              onChange={(e) => setInviteClaude(e.target.checked)}
            />
            Install Claude
          </label>
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={inviteVault}
              onChange={(e) => setInviteVault(e.target.checked)}
            />
            Manage vault
          </label>
        </div>
        {inviteLink ? (
          <div className="text-xs space-y-1 border-t border-bunny-border pt-3">
            <p className="text-emerald-300">
              Invitation created for <strong>{createdInvite?.email}</strong>
            </p>
            <p className="text-bunny-muted">Share this link (valid 7 days):</p>
            <input
              readOnly
              value={inviteLink}
              className="w-full px-2 py-1.5 bg-bunny-bg border border-bunny-border rounded font-mono text-[11px]"
              onFocus={(e) => e.currentTarget.select()}
            />
            <button
              type="button"
              className="text-bunny-accent hover:underline"
              onClick={() => {
                void navigator.clipboard.writeText(inviteLink);
              }}
            >
              Copy link
            </button>
          </div>
        ) : null}
      </form>

      <div className="w-full max-w-5xl border border-bunny-border rounded overflow-x-auto">
        <div className="grid grid-cols-[minmax(10rem,1.4fr)_repeat(4,minmax(5rem,0.7fr))_minmax(6rem,0.8fr)_4rem] gap-2 px-3 py-2 bg-bunny-panel text-xs text-bunny-muted border-b border-bunny-border min-w-[44rem]">
          <div>User</div>
          <div>Status</div>
          <div>Sessions</div>
          <div>Claude</div>
          <div>Vault</div>
          <div>Default role</div>
          <div />
        </div>

        {loading ? (
          <div className="p-3 text-sm text-bunny-muted">Loading…</div>
        ) : users.length === 0 ? (
          <div className="p-3 text-sm text-bunny-muted">No users.</div>
        ) : (
          users.map((u) => {
            const locked = u.is_system_owner || u.disabled;
            return (
              <div
                key={u.id}
                className="grid grid-cols-[minmax(10rem,1.4fr)_repeat(4,minmax(5rem,0.7fr))_minmax(6rem,0.8fr)_4rem] gap-2 px-3 py-2 border-b border-bunny-border bg-bunny-bg items-center min-w-[44rem]"
              >
                <div className="min-w-0">
                  <div className="text-sm text-gray-200 truncate">{u.email}</div>
                  {u.is_system_owner ? (
                    <div className="text-[11px] text-bunny-accent">system owner</div>
                  ) : null}
                </div>
                <div className="text-xs">
                  {u.disabled ? (
                    <span className="text-red-300">disabled</span>
                  ) : (
                    <span className="text-emerald-300">active</span>
                  )}
                </div>
                <div>
                  <label className="text-xs text-bunny-muted flex items-center gap-2">
                    <input
                      type="checkbox"
                      checked={u.is_system_owner || u.can_create_sessions}
                      disabled={locked || saving === u.id}
                      onChange={(e) =>
                        void saveUser(u, { can_create_sessions: e.target.checked })
                      }
                    />
                    create
                  </label>
                </div>
                <div>
                  <label className="text-xs text-bunny-muted flex items-center gap-2">
                    <input
                      type="checkbox"
                      checked={u.is_system_owner || u.can_install_claude}
                      disabled={locked || saving === u.id}
                      onChange={(e) =>
                        void saveUser(u, { can_install_claude: e.target.checked })
                      }
                    />
                    install
                  </label>
                </div>
                <div>
                  <label className="text-xs text-bunny-muted flex items-center gap-2">
                    <input
                      type="checkbox"
                      checked={u.is_system_owner || u.can_manage_vault}
                      disabled={locked || saving === u.id}
                      onChange={(e) =>
                        void saveUser(u, { can_manage_vault: e.target.checked })
                      }
                    />
                    manage
                  </label>
                </div>
                <div>
                  <select
                    value={u.is_system_owner ? 'owner' : u.default_session_role}
                    disabled={locked || saving === u.id}
                    onChange={(e) =>
                      void saveUser(u, { default_session_role: e.target.value })
                    }
                    className="w-full px-2 py-1 bg-bunny-bg border border-bunny-border rounded text-xs"
                  >
                    {u.is_system_owner ? (
                      <option value="owner">owner</option>
                    ) : (
                      SESSION_ROLES.map((r) => (
                        <option key={r} value={r}>
                          {r}
                        </option>
                      ))
                    )}
                  </select>
                </div>
                <div className="text-right">
                  {!u.is_system_owner && !u.disabled ? (
                    <button
                      type="button"
                      disabled={saving === u.id}
                      onClick={() => void removeUser(u)}
                      className="text-xs text-red-300 hover:text-red-200 disabled:opacity-50"
                    >
                      {saving === u.id ? '…' : 'Remove'}
                    </button>
                  ) : null}
                </div>
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}
