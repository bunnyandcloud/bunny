import TerminalPanel from './TerminalPanel';
import { useT } from '../i18n';

interface Props {
  terminalId: string;
  open: boolean;
  onClose: () => void;
  /** Notebook interactive session (python REPL, npx, pip prompts, etc.). */
  interactive?: boolean;
  onStop?: () => void;
  wheelScrollTui?: boolean;
}

export default function AttachTtyDrawer({
  terminalId,
  open,
  onClose,
  interactive = false,
  onStop,
  wheelScrollTui,
}: Props) {
  const tr = useT();
  if (!open) return null;

  return (
    <div className="absolute inset-0 z-20 flex flex-col bg-bunny-bg/95 backdrop-blur-sm">
      <div className="flex items-center justify-between gap-2 border-b border-bunny-border px-3 py-2">
        <p className="text-sm font-medium">
          {interactive
            ? tr('web.notebook.interactiveDrawerTitle')
            : tr('web.notebook.attachDrawerTitle')}
        </p>
        <div className="flex shrink-0 items-center gap-2">
          {interactive && onStop ? (
            <button
              type="button"
              className="rounded border border-bunny-border px-2 py-1 text-xs hover:bg-bunny-panel"
              onClick={onStop}
            >
              {tr('web.notebook.stop')}
            </button>
          ) : null}
          <button
            type="button"
            className="rounded border border-bunny-border px-2 py-1 text-xs hover:bg-bunny-panel"
            onClick={onClose}
          >
            {tr('web.notebook.close')}
          </button>
        </div>
      </div>
      <div className="min-h-0 flex-1">
        <TerminalPanel
          terminalId={terminalId}
          active
          autoFocus
          liveAttach={interactive}
          wheelScrollTui={wheelScrollTui}
          notebookRecordCommands
        />
      </div>
    </div>
  );
}
