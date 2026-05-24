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
  return api<{ user_id: string; email: string }>('/auth/me');
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
