import type { Terminal } from '@xterm/xterm';
import { stripAnsi } from './ansi';

/** Command text after a bash/zsh prompt on one terminal row. */
export function extractCommandFromPromptLine(line: string): string {
  const clean = stripAnsi(line).trimEnd();
  if (!clean) return '';

  const withVenv = clean.match(/^(?:\([^)]+\)\s*)?[^\s]+@[^:]+:.*?[#$]\s*(.*)$/);
  if (withVenv?.[1] !== undefined) {
    return withVenv[1].trim();
  }

  const prompt = clean.match(/^[^\s]+@[^:]+:.*?[#$]\s*(.*)$/);
  if (prompt?.[1] !== undefined) {
    return prompt[1].trim();
  }

  const simple = clean.match(/^.*?[#$]\s+(.*)$/);
  if (simple?.[1] !== undefined) {
    return simple[1].trim();
  }

  return clean.trim();
}

/** Read the command line the user just submitted in an attached xterm. */
export function extractSubmittedCommandFromTerminal(term: Terminal): string {
  const buf = term.buffer.active;
  const row = buf.baseY + buf.cursorY;
  for (const r of [row, row - 1]) {
    if (r < 0) continue;
    const line = buf.getLine(r);
    if (!line) continue;
    const cmd = extractCommandFromPromptLine(line.translateToString(true));
    if (cmd) return cmd;
  }
  return '';
}
