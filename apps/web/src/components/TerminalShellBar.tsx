import InlineRename from './InlineRename';

export interface ShellTab {
  id: string;
  name: string;
  status: string;
  cwd?: string;
  git_project?: string;
  git_branch?: string;
}

interface Props {
  shells: ShellTab[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onRename: (id: string, name: string) => Promise<void>;
  onNew: () => void;
  busy?: boolean;
}

function shellTooltip(shell: ShellTab, ended: boolean): string {
  const lines = [`${shell.name} (${shell.id})`];
  if (shell.git_project && shell.git_branch) {
    lines.push(`${shell.git_project} · ${shell.git_branch}`);
  }
  if (shell.cwd) {
    lines.push(shell.cwd);
  }
  if (ended) {
    lines.push('ended');
  }
  return lines.join('\n');
}

export default function TerminalShellBar({
  shells,
  activeId,
  onSelect,
  onClose,
  onRename,
  onNew,
  busy,
}: Props) {
  return (
    <div className="flex items-center gap-1 px-2 py-1 border-b border-bunny-border bg-bunny-panel overflow-x-auto">
      {shells.map((shell) => {
        const active = shell.id === activeId;
        const ended =
          shell.status.toLowerCase().includes('exited') ||
          shell.status.toLowerCase().includes('crashed') ||
          shell.status.toLowerCase().includes('stopped');
        const gitLabel =
          shell.git_project && shell.git_branch
            ? `${shell.git_project} · ${shell.git_branch}`
            : null;
        return (
          <div
            key={shell.id}
            role="tab"
            aria-selected={active}
            className={`flex items-center gap-0.5 rounded text-xs shrink-0 cursor-pointer ${
              active
                ? 'bg-bunny-bg border border-bunny-accent text-gray-100'
                : 'border border-bunny-border text-bunny-muted hover:bg-bunny-bg'
            } ${ended ? 'opacity-60' : ''}`}
            title={shellTooltip(shell, ended)}
            onClick={() => onSelect(shell.id)}
          >
            <div className="px-2 py-0.5 truncate max-w-[180px] min-w-0">
              <div className="truncate">
                <InlineRename
                  value={shell.name}
                  className="text-xs"
                  inputClassName="text-xs max-w-[160px]"
                  title="Double-click to rename shell"
                  disabled={busy}
                  onSave={(name) => onRename(shell.id, name)}
                />
                {ended ? ' (ended)' : ''}
              </div>
              {gitLabel ? (
                <div className="truncate text-[10px] leading-tight text-bunny-muted/90 font-mono">
                  {gitLabel}
                </div>
              ) : null}
            </div>
            <button
              type="button"
              className="px-1.5 py-1 hover:text-red-400 self-start"
              title="Close shell"
              aria-label={`Close ${shell.name}`}
              onClick={(e) => {
                e.stopPropagation();
                onClose(shell.id);
              }}
            >
              ×
            </button>
          </div>
        );
      })}
      <button
        type="button"
        disabled={busy}
        className="text-xs px-2 py-1 border border-dashed border-bunny-border rounded text-bunny-muted hover:bg-bunny-bg disabled:opacity-50 shrink-0"
        onClick={onNew}
      >
        + New shell
      </button>
    </div>
  );
}
