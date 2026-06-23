import TerminalPanel from './TerminalPanel';

interface Props {
  terminalId: string;
  open: boolean;
  onClose: () => void;
  wheelScrollTui?: boolean;
}

export default function AttachTtyDrawer({ terminalId, open, onClose, wheelScrollTui }: Props) {
  if (!open) return null;

  return (
    <div className="absolute inset-0 z-20 flex flex-col bg-bunny-bg/95 backdrop-blur-sm">
      <div className="flex items-center justify-between border-b border-bunny-border px-3 py-2">
        <p className="text-sm font-medium">Attach TTY — full interactive terminal</p>
        <button
          type="button"
          className="rounded border border-bunny-border px-2 py-1 text-xs hover:bg-bunny-panel"
          onClick={onClose}
        >
          Close
        </button>
      </div>
      <div className="min-h-0 flex-1">
        <TerminalPanel terminalId={terminalId} active autoFocus wheelScrollTui={wheelScrollTui} />
      </div>
    </div>
  );
}
