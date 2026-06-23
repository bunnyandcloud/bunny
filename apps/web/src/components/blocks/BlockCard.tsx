import { stripAnsi, stripShellPromptLines } from '../../lib/ansi';
import {
  interactiveTuiCommand,
  isInteractiveRunning,
} from '../../lib/blockMeta';
import type { TerminalBlock } from '../../lib/api';
import { useT } from '../../i18n';
import BlockAuthorBadge from './BlockAuthorBadge';
import BlockTimelineRail from './BlockTimelineRail';
import InteractiveTtyEmbed from './InteractiveTtyEmbed';

interface Props {
  terminalId: string;
  block: TerminalBlock;
  outputBlock?: TerminalBlock;
  interactiveExpanded?: boolean;
  ttyActive?: boolean;
  onStop?: (blockId: string) => void;
  onFullscreen?: () => void;
}

function isCommandKind(kind: TerminalBlock['kind']) {
  return kind === 'user_command' || kind === 'discord_command';
}

export default function BlockCard({
  terminalId,
  block,
  outputBlock,
  interactiveExpanded = false,
  ttyActive = false,
  onStop,
  onFullscreen,
}: Props) {
  const tr = useT();
  const interactiveRunning = isInteractiveRunning(outputBlock);
  const running = outputBlock?.status === 'running';

  if (block.kind === 'system_event') {
    return (
      <div className="flex gap-2 py-2 text-xs text-bunny-muted italic">
        <BlockTimelineRail timestamp={block.created_at} />
        <p className="flex-1">{stripAnsi(block.content)}</p>
      </div>
    );
  }

  if (isCommandKind(block.kind)) {
    const outputText = outputBlock
      ? stripShellPromptLines(stripAnsi(outputBlock.content))
      : '';
    const tuiName = interactiveTuiCommand(outputBlock) ?? block.command ?? 'app';

    return (
      <div className="flex gap-2 py-2 border-b border-bunny-border/30">
        <BlockTimelineRail timestamp={block.created_at} />
        <div className="min-w-0 flex-1 space-y-1">
          <div className="flex items-start gap-2">
            <BlockAuthorBadge block={block} />
            <code className="flex-1 break-all font-mono text-sm text-bunny-fg">
              <span className="text-bunny-muted">$ </span>
              <span className="font-bold">{block.command ?? block.content}</span>
            </code>
          </div>
          {running ? (
            <div className="ml-8 flex flex-wrap items-center gap-2 text-xs">
              <span
                className={
                  interactiveRunning
                    ? 'rounded bg-blue-500/20 px-1.5 py-0.5 font-medium text-blue-400'
                    : 'rounded bg-yellow-500/20 px-1.5 py-0.5 font-medium text-yellow-600'
                }
              >
                {interactiveRunning
                  ? tr('web.notebook.interactiveBadge')
                  : tr('web.notebook.runningBadge')}
              </span>
              {interactiveRunning ? (
                <span className="text-bunny-muted">
                  {tr('web.notebook.interactiveHint', { app: tuiName })}
                </span>
              ) : null}
              {onStop && outputBlock ? (
                <button
                  type="button"
                  className="rounded border border-bunny-border px-2 py-0.5 hover:bg-bunny-panel"
                  onClick={() => onStop(outputBlock.id)}
                >
                  {tr('web.notebook.stop')}
                </button>
              ) : null}
              {interactiveRunning && onFullscreen ? (
                <button
                  type="button"
                  className="rounded border border-bunny-border px-2 py-0.5 hover:bg-bunny-panel"
                  onClick={onFullscreen}
                >
                  {tr('web.notebook.fullscreen')}
                </button>
              ) : null}
            </div>
          ) : null}
          {interactiveRunning && interactiveExpanded ? (
            <div className="ml-8 mt-2">
              <InteractiveTtyEmbed terminalId={terminalId} active={ttyActive} />
            </div>
          ) : null}
          {!interactiveRunning && outputText ? (
            <pre
              className={
                outputBlock?.status === 'failed'
                  ? 'ml-8 whitespace-pre-wrap break-words font-mono text-xs text-red-400/90'
                  : 'ml-8 whitespace-pre-wrap break-words font-mono text-xs text-bunny-fg/90'
              }
            >
              {outputText}
            </pre>
          ) : !interactiveRunning && running ? (
            <pre className="ml-8 font-mono text-xs text-bunny-muted">…</pre>
          ) : null}
        </div>
      </div>
    );
  }

  return (
    <div className="flex gap-2 py-1">
      <BlockTimelineRail timestamp={block.created_at} />
      <div className="min-w-0 flex-1">
        <pre className="whitespace-pre-wrap break-words font-mono text-xs text-bunny-fg/90">
          {stripAnsi(block.content)}
        </pre>
      </div>
    </div>
  );
}
