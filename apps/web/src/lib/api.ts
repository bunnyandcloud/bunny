const API = '/api/v1';
const REQUEST_TIMEOUT_MS = 30_000;

export function apiErrorMessage(err: unknown, fallback: string): string {
  if (err instanceof Error) {
    if (err.name === 'TimeoutError' || err.name === 'AbortError') {
      return 'Request timed out. Please try again.';
    }
    const msg = err.message.trim();
    if (msg) return msg;
  }
  return fallback;
}

export async function api<T>(
  path: string,
  options: RequestInit = {},
): Promise<T> {
  const res = await fetch(`${API}${path}`, {
    ...options,
    credentials: 'include',
    signal: options.signal ?? AbortSignal.timeout(REQUEST_TIMEOUT_MS),
    headers: {
      'Content-Type': 'application/json',
      ...options.headers,
    },
  });
  if (!res.ok) {
    const err = await res.json().catch(() => ({}));
    const msg =
      (typeof err?.error?.message === 'string' && err.error.message.trim()) ||
      res.statusText.trim() ||
      `Request failed (${res.status})`;
    throw new Error(msg);
  }
  if (res.status === 204) return undefined as T;
  return res.json();
}

export type LoginResponse =
  | {
      mfa_required: false;
      user_id: string;
      email: string;
      session_token: string;
      expires_at: string;
    }
  | {
      mfa_required: true;
      mfa_challenge_token: string;
      user_id: string;
      email: string;
    };

export function login(email: string, password: string) {
  return api<LoginResponse>('/auth/login', {
    method: 'POST',
    body: JSON.stringify({ email, password }),
  });
}

export function acceptInvitation(body: {
  token: string;
  email: string;
  password: string;
  device_id?: string;
}) {
  return api<{
    user_id: string;
    email: string;
    session_id: string | null;
    role: string;
    expires_at: string;
  }>('/invitations/accept', {
    method: 'POST',
    body: JSON.stringify(body),
  });
}

export function verifyMfa(
  code: string,
  mfaChallengeToken?: string,
) {
  return api<{
    user_id: string;
    email: string;
    session_token: string;
    expires_at: string;
  }>('/auth/mfa/verify', {
    method: 'POST',
    body: JSON.stringify({
      code,
      ...(mfaChallengeToken ? { mfa_challenge_token: mfaChallengeToken } : {}),
    }),
  });
}

export function me() {
  return api<{
    user_id: string;
    email: string;
    is_owner: boolean;
    mfa_enabled: boolean;
    can_install_claude: boolean;
    can_manage_vault: boolean;
    can_create_sessions: boolean;
    default_session_role: string;
  }>('/auth/me');
}

export type TeamUser = {
  id: string;
  email: string;
  disabled: boolean;
  is_system_owner: boolean;
  can_install_claude: boolean;
  can_manage_vault: boolean;
  can_create_sessions: boolean;
  default_session_role: string;
};

export function listUsersAdmin() {
  return api<TeamUser[]>('/users');
}

export function createTeamUser(body: {
  email: string;
  default_session_role: string;
  can_install_claude: boolean;
  can_manage_vault: boolean;
  can_create_sessions: boolean;
}) {
  return api<{ token: string }>('/users', {
    method: 'POST',
    body: JSON.stringify(body),
  });
}

export function updateUserAdmin(body: {
  user_id: string;
  can_install_claude: boolean;
  can_manage_vault: boolean;
  can_create_sessions: boolean;
  default_session_role: string;
}) {
  return api<{ ok: boolean }>('/users', {
    method: 'PATCH',
    body: JSON.stringify(body),
  });
}

export function revokeTeamUser(userId: string) {
  return api<{ ok: boolean }>(`/users/${userId}`, {
    method: 'DELETE',
  });
}

export function mfaStatus() {
  return api<{ enabled: boolean; recovery_remaining: number }>('/auth/mfa/status');
}

export function mfaSetup(password?: string) {
  return api<{ otpauth_uri: string; secret_base32: string }>('/auth/mfa/setup', {
    method: 'POST',
    body: JSON.stringify(password ? { password } : {}),
  });
}

export function mfaEnable(code: string, password?: string) {
  return api<{ recovery_codes: string[] }>('/auth/mfa/enable', {
    method: 'POST',
    body: JSON.stringify({ code, ...(password ? { password } : {}) }),
  });
}

export function mfaDisable(code: string, password?: string) {
  return api<{ ok: boolean }>('/auth/mfa/disable', {
    method: 'POST',
    body: JSON.stringify({ code, ...(password ? { password } : {}) }),
  });
}

export function mfaRegenerateRecovery(code: string, password?: string) {
  return api<{ recovery_codes: string[] }>('/auth/mfa/recovery/regenerate', {
    method: 'POST',
    body: JSON.stringify({ code, ...(password ? { password } : {}) }),
  });
}

export function listSessions() {
  return api<Array<{ id: string; name: string; project_path: string; status: string }>>(
    '/sessions',
  );
}

export function getSession(sessionId: string) {
  return api<{ id: string; name: string; project_path: string; status: string }>(
    `/sessions/${sessionId}`,
  );
}

export function renameSession(sessionId: string, name: string) {
  return api<{ id: string; name: string; project_path: string; status: string }>(
    `/sessions/${sessionId}`,
    { method: 'PATCH', body: JSON.stringify({ name }) },
  );
}

export function deleteSession(sessionId: string) {
  return api<void>(`/sessions/${sessionId}`, { method: 'DELETE' });
}

export function createSession(projectPath?: string) {
  const body = projectPath ? { project_path: projectPath } : {};
  return api<{ id: string; login_url: string }>('/sessions', {
    method: 'POST',
    body: JSON.stringify(body),
  });
}

export function createInvitation(sessionId: string, email: string, role: string) {
  return api<{ token: string }>(`/sessions/${sessionId}/invitations`, {
    method: 'POST',
    body: JSON.stringify({ email, role }),
  });
}

export function listSessionMembers(sessionId: string) {
  return api<Array<{ user_id: string; email: string; role: string }>>(
    `/sessions/${sessionId}/members`,
  );
}

export function updateSessionMember(sessionId: string, userId: string, role: string) {
  return api<{ ok: boolean }>(`/sessions/${sessionId}/members/${userId}`, {
    method: 'PATCH',
    body: JSON.stringify({ role }),
  });
}

export function createDiscordLinkCode(sessionId: string, password?: string) {
  return api<{
    code: string;
    expires_in_minutes: number;
    instructions: string;
  }>(`/sessions/${sessionId}/discord/link-codes`, {
    method: 'POST',
    body: JSON.stringify({ password: password ?? null }),
  });
}

export function listDiscordLinks(sessionId: string) {
  return api<
    Array<{
      guild_id: string;
      channel_id: string;
      session_id: string;
      status: string;
      created_at: string;
    }>
  >(`/sessions/${sessionId}/discord/links`);
}

export function revokeDiscordLinks(sessionId: string) {
  return api<{ ok: boolean }>(`/sessions/${sessionId}/discord/links`, {
    method: 'DELETE',
  });
}

export function getWatchMeta(token: string) {
  return api<{
    session_id: string;
    layout: string;
    mode: string;
    visibility: string;
    browser_ids: string[];
    expires_at: string;
  }>(`/watch/${token}`);
}

export function grantWatchAccess(
  token: string,
  body: { discord_user_id?: string; bunny_session_token?: string },
) {
  return api<{
    access_token: string;
    session_id: string;
    expires_in_secs: number;
  }>(`/watch/${token}/access`, {
    method: 'POST',
    body: JSON.stringify(body),
  });
}

export function removeSessionMember(sessionId: string, userId: string) {
  return api<{ ok: boolean }>(`/sessions/${sessionId}/members/${userId}`, {
    method: 'DELETE',
  });
}

export function createTerminal(
  sessionId: string,
  name: string,
  command?: string,
) {
  const body: Record<string, unknown> = {
    session_id: sessionId,
    name,
    cols: 80,
    rows: 24,
  };
  if (command) body.command = command;
  return api<{ id: string; name: string; ws_url: string }>('/terminals', {
    method: 'POST',
    body: JSON.stringify(body),
  });
}

export function listSessionTerminals(sessionId: string) {
  return api<Array<{ id: string; name: string; status: string }>>(
    `/terminals?session_id=${encodeURIComponent(sessionId)}`,
  );
}

export function deleteTerminal(terminalId: string) {
  return api<void>(`/terminals/${terminalId}`, { method: 'DELETE' });
}

export function sendTerminalInput(terminalId: string, data: string) {
  return api<void>(`/terminals/${terminalId}/input`, {
    method: 'POST',
    body: JSON.stringify({ data }),
  });
}

export function renameTerminal(terminalId: string, name: string) {
  return api<{ id: string; name: string; status: string }>(`/terminals/${terminalId}`, {
    method: 'PATCH',
    body: JSON.stringify({ name }),
  });
}

export function getTimeline(sessionId: string, since = 0) {
  return api<
    Array<{
      id: string;
      source: string;
      event_type: string;
      payload: unknown;
      sequence: number;
      ts: string;
    }>
  >(`/timeline?session_id=${sessionId}&since=${since}&limit=200`);
}

export function terminalWsUrl(terminalId: string, fromOffset?: number) {
  const proto = location.protocol === 'https:' ? 'wss' : 'ws';
  const q = fromOffset ? `?from_offset=${fromOffset}` : '';
  return `${proto}://${location.host}/api/v1/terminals/${terminalId}/ws${q}`;
}

export interface VaultStatus {
  status: 'missing' | 'locked' | 'unlocked';
  path: string;
  ref_count: number;
}

export interface SecretMeta {
  name: string;
  scope: string;
  session_id: string | null;
  env_var: string;
}

export function getSecretsStatus() {
  return api<VaultStatus>('/secrets/status');
}

export function initSecretsVault(passphrase: string, confirmPassphrase: string) {
  return api<{ ok: boolean }>('/secrets/init', {
    method: 'POST',
    body: JSON.stringify({ passphrase, confirm_passphrase: confirmPassphrase }),
  });
}

export function unlockSecretsVault(passphrase: string, sessionId?: string) {
  return api<{ ok: boolean }>('/secrets/unlock', {
    method: 'POST',
    body: JSON.stringify({
      passphrase,
      ...(sessionId ? { session_id: sessionId } : {}),
    }),
  });
}

export function lockSecretsVault() {
  return api<{ ok: boolean }>('/secrets/lock', { method: 'POST' });
}

export function listSecrets() {
  return api<SecretMeta[]>('/secrets');
}

export function upsertSecret(body: {
  name: string;
  scope: string;
  session_id?: string;
  value: string;
}) {
  return api<SecretMeta>('/secrets', {
    method: 'POST',
    body: JSON.stringify(body),
  });
}

export function deleteSecret(name: string, scope: string, sessionId?: string) {
  const params = new URLSearchParams({ scope });
  if (sessionId) params.set('session_id', sessionId);
  return api<void>(`/secrets/${encodeURIComponent(name)}?${params}`, { method: 'DELETE' });
}

export function revealSecret(name: string, scope: string, sessionId?: string) {
  const params = new URLSearchParams({ scope });
  if (sessionId) params.set('session_id', sessionId);
  return api<{ value: string }>(`/secrets/${encodeURIComponent(name)}/reveal?${params}`);
}

export function createPreview(sessionId: string, localPort: number) {
  return api<{ id: string; public_path: string }>('/previews', {
    method: 'POST',
    body: JSON.stringify({ session_id: sessionId, local_port: localPort }),
  });
}

export function listPreviews() {
  return api<Array<{ id: string; public_path: string }>>('/previews');
}

export function createBrowser(sessionId: string, targetUrl?: string) {
  return api<{
    id: string;
    stream_path: string;
    events_path: string;
    webrtc_offer_path: string;
  }>('/browser-sessions', {
    method: 'POST',
    body: JSON.stringify({
      session_id: sessionId,
      ...(targetUrl ? { target_url: targetUrl } : {}),
    }),
  });
}

export function getBrowser(browserId: string) {
  return api<{
    id: string;
    novncPort: number | null;
    cdpPort: number | null;
    webrtcOfferPath: string;
  }>(`/browser-sessions/${browserId}`);
}

export function browserWebrtcStop(browserId: string) {
  return api<void>(`/browser-sessions/${browserId}/webrtc/stop`, { method: 'POST' });
}

export function restartBrowser(browserId: string, sessionId: string, targetUrl?: string) {
  return api<{ restarted: boolean }>(`/browser-sessions/${browserId}/restart`, {
    method: 'POST',
    body: JSON.stringify({
      session_id: sessionId,
      ...(targetUrl ? { target_url: targetUrl } : {}),
    }),
  });
}

export function getWebRtcConfig() {
  return api<{
    enabled: boolean;
    ice_servers: Array<{
      urls: string[];
      username?: string;
      credential?: string;
    }>;
    sidecar_port: number;
  }>('/webrtc/config');
}

export function browserWebrtcOffer(
  browserId: string,
  offer: { type: string; sdp: string },
) {
  return api<{ type: string; sdp: string }>(
    `/browser-sessions/${browserId}/webrtc/offer`,
    {
      method: 'POST',
      body: JSON.stringify(offer),
    },
  );
}

export function browserWebrtcCandidate(
  browserId: string,
  candidate: Record<string, unknown>,
) {
  return api<void>(`/browser-sessions/${browserId}/webrtc/candidate`, {
    method: 'POST',
    body: JSON.stringify({ candidate }),
  });
}

export interface ClaudeStatus {
  installed: boolean;
  authenticated: boolean;
  version: string | null;
  binary: string | null;
  install: {
    state: string;
    message: string;
    error: string | null;
  };
  auth: {
    active: boolean;
    phase: string;
    session_id: string | null;
    terminal_id: string | null;
    oauth_url: string | null;
    oauth_browser_url: string | null;
    code_submitted: boolean;
    error: string | null;
  };
}

export function getClaudeStatus() {
  return api<ClaudeStatus>('/claude/status');
}

export function installClaude() {
  return api<{ started: boolean; state?: string }>('/claude/install', {
    method: 'POST',
  });
}

export function startClaudeAuth(sessionId?: string) {
  return api<{ session_id: string; terminal_id: string }>('/claude/auth/start', {
    method: 'POST',
    body: JSON.stringify(sessionId ? { session_id: sessionId } : {}),
  });
}

export function submitClaudeAuthCode(code: string) {
  return api<{ ok: boolean }>('/claude/auth/code', {
    method: 'POST',
    body: JSON.stringify({ code }),
  });
}

export function detectClaudeAuthCode(browserId: string) {
  return api<{ found: boolean; submitted: boolean; code_hint?: string | null }>(
    '/claude/auth/detect-code',
    {
      method: 'POST',
      body: JSON.stringify({ browser_id: browserId }),
    },
  );
}

export function browserNavigate(browserId: string, url: string) {
  return api<{ ok: boolean }>(`/browser-sessions/${browserId}/control`, {
    method: 'POST',
    body: JSON.stringify({ navigate: url }),
  });
}

export function sessionRealtimeWsUrl(sessionId: string) {
  const proto = location.protocol === 'https:' ? 'wss' : 'ws';
  return `${proto}://${location.host}/api/v1/sessions/${sessionId}/realtime`;
}

export function previewUrl(sessionId: string, port: number) {
  return `/s/${sessionId}/ports/${port}/`;
}

export function browserNovncUrl(browserId: string) {
  const path = `api/v1/browser-sessions/${browserId}/vnc/ws`;
  return `/api/v1/browser-sessions/${browserId}/vnc/vnc.html?autoconnect=1&reconnect=1&reconnect_delay=2000&resize=scale&path=${path}`;
}
