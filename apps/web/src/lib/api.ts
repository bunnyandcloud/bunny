const API = '/api/v1';

export async function api<T>(
  path: string,
  options: RequestInit = {},
): Promise<T> {
  const res = await fetch(`${API}${path}`, {
    ...options,
    credentials: 'include',
    headers: {
      'Content-Type': 'application/json',
      ...options.headers,
    },
  });
  if (!res.ok) {
    const err = await res.json().catch(() => ({}));
    throw new Error(err?.error?.message || res.statusText);
  }
  if (res.status === 204) return undefined as T;
  return res.json();
}

export function login(email: string, password: string) {
  return api<{ user_id: string; email: string }>('/auth/login', {
    method: 'POST',
    body: JSON.stringify({ email, password }),
  });
}

export function me() {
  return api<{ user_id: string; email: string; is_owner: boolean }>('/auth/me');
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
