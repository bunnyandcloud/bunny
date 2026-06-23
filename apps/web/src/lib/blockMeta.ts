import type { TerminalBlock } from './api';

type BlockMeta = {
  interactive?: boolean;
  tui_command?: string;
};

export function blockMeta(block: TerminalBlock | undefined): BlockMeta {
  if (!block?.meta || typeof block.meta !== 'object' || Array.isArray(block.meta)) {
    return {};
  }
  return block.meta as BlockMeta;
}

export function isInteractiveBlock(block: TerminalBlock | undefined): boolean {
  return blockMeta(block).interactive === true;
}

export function interactiveTuiCommand(block: TerminalBlock | undefined): string | undefined {
  return blockMeta(block).tui_command;
}

export function commandExpectsInteractive(cmd: string): boolean {
  const lower = cmd.trim().toLowerCase();
  const parts = cmd.trim().split(/\s+/);
  const first = parts[0]?.replace(/^.*\//, '').toLowerCase() ?? '';

  if (first === 'apt' || first === 'apt-get') {
    if (
      lower.includes(' install') ||
      lower.includes(' update') ||
      lower.includes(' upgrade') ||
      lower.includes(' remove') ||
      lower.includes(' purge') ||
      lower.includes(' autoremove') ||
      lower.includes(' -y') ||
      lower.includes(' --yes')
    ) {
      return false;
    }
    return true;
  }

  const tui = new Set([
    'nvim', 'vim', 'vi', 'view', 'nano', 'micro', 'emacs', 'emacsclient',
    'htop', 'top', 'btop', 'less', 'more', 'man', 'apt', 'apt-get', 'dpkg',
    'dialog', 'whiptail', 'mysql', 'psql', 'sqlite3', 'mc', 'ranger', 'tig',
    'lazygit', 'claude', 'aider', 'ipython', 'bpython',
  ]);
  if (tui.has(first)) return true;
  if (['python', 'python3', 'node', 'ruby', 'irb'].includes(first)) {
    return parts.length === 1;
  }
  return false;
}

export function isInteractiveRunning(
  outputBlock: TerminalBlock | undefined,
): boolean {
  return (
    outputBlock?.status === 'running' &&
    outputBlock.kind === 'process_run' &&
    isInteractiveBlock(outputBlock)
  );
}
