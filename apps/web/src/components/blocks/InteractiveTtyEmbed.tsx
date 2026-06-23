import TerminalPanel from '../TerminalPanel';

interface Props {
  terminalId: string;
  active: boolean;
}

/** Compact live xterm for interactive TUI sessions (nvim, installers, etc.). */
export default function InteractiveTtyEmbed({ terminalId, active }: Props) {
  return (
    <div
      className="overflow-hidden rounded border border-bunny-border bg-[#0d0d0d]"
      onWheel={(e) => e.stopPropagation()}
    >
      <div className="h-[min(320px,40vh)] min-h-[200px] w-full">
        <TerminalPanel
          terminalId={terminalId}
          active={active}
          autoFocus={active}
          embedded
          liveAttach
        />
      </div>
    </div>
  );
}
