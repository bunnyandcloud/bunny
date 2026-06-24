import type { TerminalBlock } from './api';

const STORAGE_PREFIX = 'bunny-notebook-cmd-history:';
export const NOTEBOOK_COMMAND_HISTORY_MAX = 200;

export function notebookHistoryStorageKey(terminalId: string): string {
  return `${STORAGE_PREFIX}${terminalId}`;
}

export function readStoredNotebookHistory(terminalId: string): string[] {
  try {
    const raw = sessionStorage.getItem(notebookHistoryStorageKey(terminalId));
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((c): c is string => typeof c === 'string')
      .map((c) => c.trim())
      .filter(Boolean);
  } catch {
    return [];
  }
}

export function writeStoredNotebookHistory(terminalId: string, history: string[]): void {
  try {
    sessionStorage.setItem(
      notebookHistoryStorageKey(terminalId),
      JSON.stringify(history.slice(-NOTEBOOK_COMMAND_HISTORY_MAX)),
    );
  } catch {
    /* private mode / quota */
  }
}

/** Drop only consecutive duplicates (bash `ignoredups`-like). */
export function dedupeConsecutiveCommands(commands: string[]): string[] {
  const out: string[] = [];
  for (const cmd of commands) {
    const c = cmd.trim();
    if (!c) continue;
    if (out[out.length - 1] === c) continue;
    out.push(c);
  }
  return out;
}

export function commandsFromUserBlocks(blocks: TerminalBlock[]): string[] {
  return dedupeConsecutiveCommands(
    blocks
      .filter((b) => b.kind === 'user_command' && b.command?.trim())
      .sort((a, b) => a.seq - b.seq)
      .map((b) => b.command!.trim()),
  );
}

/** Notebook history: persisted user commands from blocks + this browser session. */
export function mergeNotebookCommandHistory(
  fromBlocks: string[],
  fromSession: string[],
): string[] {
  const merged = [...fromBlocks];
  for (const cmd of fromSession) {
    const c = cmd.trim();
    if (!c) continue;
    if (merged[merged.length - 1] === c) continue;
    merged.push(c);
  }
  return merged.slice(-NOTEBOOK_COMMAND_HISTORY_MAX);
}

export function appendSessionCommand(history: string[], command: string): string[] {
  const cmd = normalizeNotebookHistoryCommand(command);
  if (!cmd) return history;
  if (history[history.length - 1] === cmd) return history;
  return [...history, cmd].slice(-NOTEBOOK_COMMAND_HISTORY_MAX);
}

/** Prefer the user-facing command over notebook shell wrappers. */
export function normalizeNotebookHistoryCommand(command: string): string {
  let cmd = command.trim();
  if (!cmd) return '';

  const wrapped = cmd.match(
    /^\(PAGER=cat GIT_PAGER=cat (.+)\) 2>&1(?:;\s*echo\s+__BUNNY_EXIT__\$?\?)?$/,
  );
  if (wrapped?.[1]) {
    return wrapped[1].trim();
  }

  if (cmd.startsWith('PAGER=cat GIT_PAGER=cat ')) {
    cmd = cmd.slice('PAGER=cat GIT_PAGER=cat '.length).trim();
  }

  const marker = cmd.indexOf('; echo __BUNNY_EXIT__');
  if (marker > 0) {
    cmd = cmd.slice(0, marker).trim();
  }

  return cmd.trim();
}
