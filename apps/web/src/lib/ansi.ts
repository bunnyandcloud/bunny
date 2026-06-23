/** Strip ANSI escape sequences for safe block rendering. */
export function stripAnsi(text: string): string {
  return text.replace(/\x1b\[[0-9;?]*[a-zA-Z]/g, '').replace(/\x1b\][^\x07]*(\x07|\x1b\\)/g, '');
}

/** Remove trailing shell prompt lines from captured terminal output. */
export function stripShellPromptLines(text: string): string {
  const lines = text.split('\n');
  const filtered = lines.filter((line) => !isShellPromptLine(line.trim()));
  while (filtered.length > 0 && isShellPromptLine(filtered[filtered.length - 1].trim())) {
    filtered.pop();
  }
  return filtered.join('\n');
}

function isShellPromptLine(line: string): boolean {
  if (!line.includes('@')) return false;
  const at = line.indexOf('@');
  if (!line.slice(at + 1).includes(':')) return false;
  return /[#$%]$/.test(line);
}
