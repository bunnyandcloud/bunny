import type { TerminalBlock } from './api';

type BlockMeta = {
  interactive?: boolean;
  tui_command?: string;
  tty_snapshot?: string;
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

export function interactiveTtySnapshot(block: TerminalBlock | undefined): string | undefined {
  const snap = blockMeta(block).tty_snapshot;
  return typeof snap === 'string' && snap.trim() ? snap : undefined;
}

export function commandExpectsInteractive(cmd: string): boolean {
  const lower = cmd.trim().toLowerCase();
  const parts = cmd.trim().split(/\s+/);
  const first = parts[0]?.replace(/^.*\//, '').toLowerCase() ?? '';

  if (
    lower.includes(' -y ') ||
    lower.endsWith(' -y') ||
    lower.includes(' --yes') ||
    lower.includes(' --defaults') ||
    lower.includes(' --default')
  ) {
    return false;
  }

  if (
    lower.includes('create-next-app') ||
    lower.includes('create-react-app') ||
    lower.includes('create-vite') ||
    lower.includes('create-remix') ||
    lower.includes('create-svelte') ||
    lower.includes('create-t3-app') ||
    lower.includes('sv create')
  ) {
    return true;
  }

  if (first === 'npx' || first === 'bunx') return true;
  if (['npm', 'yarn', 'pnpm', 'bun'].includes(first)) {
    if (
      lower.includes(' init') ||
      lower.includes(' create') ||
      lower.includes('create-') ||
      lower.includes(' exec')
    ) {
      return true;
    }
  }

  if (first === 'git') {
    if (
      lower.includes(' add -p') ||
      lower.includes(' add --patch') ||
      lower.includes(' add -i') ||
      lower.includes(' add --interactive') ||
      lower.includes(' stash -p') ||
      lower.includes(' stash --patch') ||
      lower.includes(' rebase -i') ||
      lower.includes(' rebase --interactive') ||
      lower.includes(' am -i') ||
      lower.includes(' am --interactive') ||
      (lower.includes(' commit') &&
        !lower.includes(' -m ') &&
        !lower.includes(' --message=') &&
        !lower.includes(' --message '))
    ) {
      return true;
    }
  }

  if (first === 'pip' || first === 'pip3') {
    if (
      (lower.includes(' uninstall') || lower.includes(' remove')) &&
      !lower.includes(' -y') &&
      !lower.includes(' --yes')
    ) {
      return true;
    }
  }

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

export function isRunningOutput(
  outputBlock: TerminalBlock | undefined,
): boolean {
  return (
    outputBlock?.status === 'running' && outputBlock.kind === 'process_run'
  );
}
