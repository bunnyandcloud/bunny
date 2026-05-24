import InlineRename from './InlineRename';

export interface ShellTab {
  id: string;
  name: string;
  status: string;
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
            title={`${shell.name} (${shell.id})${ended ? ' — ended' : ''}`}
            onClick={() => onSelect(shell.id)}
          >
            <span className="px-2 py-1 truncate max-w-[160px]">
              <InlineRename
                value={shell.name}
                className="text-xs"
                inputClassName="text-xs max-w-[140px]"
                title="Double-click to rename shell"
                disabled={busy}
                onSave={(name) => onRename(shell.id, name)}
              />
              {ended ? ' (ended)' : ''}
            </span>
            <button
              type="button"
              className="px-1.5 py-1 hover:text-red-400"
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
