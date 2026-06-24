import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  apiErrorMessage,
  fetchTerminalShellPrompt,
  listTerminalBlocks,
  stopTerminalRun,
  submitTerminalCommand,
  terminalWsUrl,
  type TerminalBlock,
} from '../lib/api';
import AttachTtyDrawer from './AttachTtyDrawer';
import BlockCard from './blocks/BlockCard';
import { useT } from '../i18n';
import { isInteractiveRunning, commandExpectsInteractive } from '../lib/blockMeta';
import {
  appendSessionCommand,
  commandsFromUserBlocks,
  mergeNotebookCommandHistory,
  readStoredNotebookHistory,
  writeStoredNotebookHistory,
} from '../lib/notebookCommandHistory';

interface Props {
  terminalId: string;
  active?: boolean;
  /** Open Attach TTY overlay on mount (e.g. Claude Code login shell). */
  defaultAttachOpen?: boolean;
}

type BlockPatchMsg = {
  type: 'block_patch';
  id: string;
  status?: TerminalBlock['status'];
  content_delta?: string;
  content_replace?: string;
  meta?: TerminalBlock['meta'];
  exit_code?: number;
  finished_at?: string;
};

function isRunningProcessBlock(block: TerminalBlock): boolean {
  return block.status === 'running' && block.kind === 'process_run';
}
function visibleBlocks(blocks: TerminalBlock[]): TerminalBlock[] {
  return blocks.filter(
    (b) =>
      !b.parent_block_id || (b.kind !== 'output' && b.kind !== 'process_run'),
  );
}

function outputForCommand(blocks: TerminalBlock[], commandId: string): TerminalBlock | undefined {
  return blocks.find(
    (b) =>
      b.parent_block_id === commandId &&
      (b.kind === 'output' || b.kind === 'process_run'),
  );
}

export default function NotebookPanel({
  terminalId,
  active = true,
  defaultAttachOpen = false,
}: Props) {
  const tr = useT();
  const [blocks, setBlocks] = useState<TerminalBlock[]>([]);
  const latestSeqRef = useRef(0);
  const [inputLocked, setInputLocked] = useState(false);
  const [attachOpen, setAttachOpen] = useState(defaultAttachOpen);
  const [inputLine, setInputLine] = useState('');
  const [wsReady, setWsReady] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [shellPrefix, setShellPrefix] = useState('');
  const scrollRef = useRef<HTMLDivElement>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  /** True when attach was opened automatically for an interactive command. */
  const attachAutoOpenedRef = useRef(false);
  const wasInteractiveRef = useRef(false);
  const draftBeforeHistoryRef = useRef('');
  const [historyIndex, setHistoryIndex] = useState<number | null>(null);
  const [sessionCommands, setSessionCommands] = useState<string[]>(() =>
    readStoredNotebookHistory(terminalId),
  );

  const blockCommands = useMemo(() => commandsFromUserBlocks(blocks), [blocks]);
  const commandHistory = useMemo(
    () => mergeNotebookCommandHistory(blockCommands, sessionCommands),
    [blockCommands, sessionCommands],
  );

  useEffect(() => {
    setSessionCommands(readStoredNotebookHistory(terminalId));
    setHistoryIndex(null);
    draftBeforeHistoryRef.current = '';
  }, [terminalId]);

  useEffect(() => {
    writeStoredNotebookHistory(terminalId, commandHistory);
  }, [terminalId, commandHistory]);

  useEffect(() => {
    if (defaultAttachOpen && active) {
      setAttachOpen(true);
    }
  }, [defaultAttachOpen, active, terminalId]);

  const mergeBlock = useCallback((block: TerminalBlock) => {
    setBlocks((prev) => {
      const idx = prev.findIndex((b) => b.id === block.id);
      if (idx >= 0) {
        const next = [...prev];
        next[idx] = block;
        return next;
      }
      return [...prev, block].sort((a, b) => a.seq - b.seq);
    });
    latestSeqRef.current = Math.max(latestSeqRef.current, block.seq);
  }, []);

  const mergeBlocks = useCallback((incoming: TerminalBlock[]) => {
    if (incoming.length === 0) return;
    setBlocks((prev) => {
      const byId = new Map(prev.map((b) => [b.id, b]));
      for (const b of incoming) {
        byId.set(b.id, b);
      }
      return [...byId.values()].sort((a, b) => a.seq - b.seq);
    });
    for (const b of incoming) {
      latestSeqRef.current = Math.max(latestSeqRef.current, b.seq);
    }
  }, []);

  const applyPatch = useCallback((msg: BlockPatchMsg) => {
    setBlocks((prev) =>
      prev.map((b) => {
        if (b.id !== msg.id) return b;
        return {
          ...b,
          content:
            msg.content_replace !== undefined
              ? msg.content_replace
              : msg.content_delta
                ? b.content + msg.content_delta
                : b.content,
          status: msg.status ?? b.status,
          meta: msg.meta ?? b.meta,
          exit_code: msg.exit_code ?? b.exit_code,
          finished_at: msg.finished_at ?? b.finished_at,
        };
      }),
    );
    if (msg.status && msg.status !== 'running') {
      setInputLocked(false);
    }
  }, []);

  const refreshShellPrefix = useCallback(() => {
    if (!active) return;
    fetchTerminalShellPrompt(terminalId)
      .then(({ prefix }) => setShellPrefix(prefix ?? ''))
      .catch(() => setShellPrefix(''));
  }, [active, terminalId]);

  useEffect(() => {
    let cancelled = false;
    listTerminalBlocks(terminalId, 0)
      .then(({ blocks: initial, latest_seq }) => {
        if (cancelled) return;
        setBlocks(initial.sort((a, b) => a.seq - b.seq));
        latestSeqRef.current = latest_seq;
        setInputLocked(initial.some(isRunningProcessBlock));
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [terminalId]);

  useEffect(() => {
    refreshShellPrefix();
  }, [refreshShellPrefix, blocks.length]);

  useEffect(() => {
    if (!active) return;
    const timer = window.setInterval(() => refreshShellPrefix(), 2000);
    return () => window.clearInterval(timer);
  }, [active, refreshShellPrefix]);

  useEffect(() => {
    if (!active) return;

    const ws = new WebSocket(terminalWsUrl(terminalId, { notebook: true }));
    wsRef.current = ws;
    setWsReady(false);

    ws.onopen = () => {
      setWsReady(true);
      ws.send(
        JSON.stringify({
          type: 'blocks_subscribe',
          from_seq: latestSeqRef.current > 0 ? latestSeqRef.current + 1 : 0,
        }),
      );
    };

    ws.onclose = () => setWsReady(false);
    ws.onerror = () => setWsReady(false);

    ws.onmessage = (ev) => {
      try {
        const msg = JSON.parse(ev.data);
        if (msg.type === 'blocks_snapshot') {
          const list = (msg.blocks as TerminalBlock[]) ?? [];
          mergeBlocks(list);
          if (typeof msg.latest_seq === 'number') {
            latestSeqRef.current = Math.max(latestSeqRef.current, msg.latest_seq);
          }
        } else if (msg.type === 'block_append') {
          mergeBlock(msg.block as TerminalBlock);
          const block = msg.block as TerminalBlock;
          if (isRunningProcessBlock(block)) {
            setInputLocked(true);
          }
        } else if (msg.type === 'block_patch') {
          applyPatch(msg as BlockPatchMsg);
          if (msg.status === 'running') setInputLocked(true);
          refreshShellPrefix();
        } else if (msg.type === 'error' && msg.code === 'input_locked') {
          setInputLocked(true);
        }
      } catch {
        /* ignore */
      }
    };

    return () => {
      ws.close();
      wsRef.current = null;
      setWsReady(false);
    };
  }, [terminalId, active, mergeBlock, mergeBlocks, applyPatch, refreshShellPrefix]);

  useEffect(() => {
    if (active && !inputLocked && !submitting) {
      inputRef.current?.focus();
    }
  }, [active, inputLocked, submitting, blocks.length]);

  const interactiveSessionActive = blocks.some((b) => isInteractiveRunning(b));

  useEffect(() => {
    if (interactiveSessionActive && !wasInteractiveRef.current) {
      attachAutoOpenedRef.current = true;
      setAttachOpen(true);
    } else if (
      !interactiveSessionActive &&
      wasInteractiveRef.current &&
      attachAutoOpenedRef.current
    ) {
      attachAutoOpenedRef.current = false;
      setAttachOpen(false);
      refreshShellPrefix();
    }
    wasInteractiveRef.current = interactiveSessionActive;
  }, [interactiveSessionActive, refreshShellPrefix]);

  useEffect(() => {
    if (interactiveSessionActive) return;
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: 'smooth' });
  }, [blocks, interactiveSessionActive]);

  const handleStop = useCallback(async () => {
    try {
      await stopTerminalRun(terminalId);
      setInputLocked(false);
    } catch {
      /* ignore */
    }
  }, [terminalId]);

  useEffect(() => {
    if (!active || !inputLocked || interactiveSessionActive) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'c' && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        void handleStop();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [active, inputLocked, interactiveSessionActive, handleStop]);

  const submitCommand = async (raw: string) => {
    const cmd = raw.trim();
    if (!cmd || inputLocked || submitting) return;

    setSubmitError(null);
    setSubmitting(true);
    setInputLine('');
    setHistoryIndex(null);
    draftBeforeHistoryRef.current = '';

    try {
      await submitTerminalCommand(terminalId, cmd);
      setSessionCommands((prev) => appendSessionCommand(prev, cmd));
      refreshShellPrefix();
      if (commandExpectsInteractive(cmd)) {
        attachAutoOpenedRef.current = true;
        setAttachOpen(true);
      }
      if (!wsReady) {
        const { blocks: refreshed } = await listTerminalBlocks(terminalId, 0);
        setBlocks(refreshed.sort((a, b) => a.seq - b.seq));
      }
    } catch (err) {
      setInputLine(cmd);
      setSubmitError(apiErrorMessage(err, tr('web.notebook.submitFailed')));
    } finally {
      setSubmitting(false);
      if (!commandExpectsInteractive(cmd)) {
        requestAnimationFrame(() => inputRef.current?.focus());
      }
    }
  };

  const handleInputKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (inputLocked || submitting) return;

    if (e.key === 'ArrowUp') {
      if (commandHistory.length === 0) return;
      e.preventDefault();
      if (historyIndex === null) {
        draftBeforeHistoryRef.current = inputLine;
        const idx = commandHistory.length - 1;
        setHistoryIndex(idx);
        setInputLine(commandHistory[idx]!);
        return;
      }
      if (historyIndex > 0) {
        const idx = historyIndex - 1;
        setHistoryIndex(idx);
        setInputLine(commandHistory[idx]!);
      }
      return;
    }

    if (e.key === 'ArrowDown') {
      if (historyIndex === null) return;
      e.preventDefault();
      if (historyIndex < commandHistory.length - 1) {
        const idx = historyIndex + 1;
        setHistoryIndex(idx);
        setInputLine(commandHistory[idx]!);
        return;
      }
      setHistoryIndex(null);
      setInputLine(draftBeforeHistoryRef.current);
    }
  };

  const displayBlocks = visibleBlocks(blocks);

  return (
    <div
      className={`relative flex h-full min-h-0 flex-col${active ? '' : ' invisible pointer-events-none'}`}
      aria-hidden={!active}
    >
      <div ref={scrollRef} className="min-h-0 flex-1 overflow-auto px-2 py-2">
        {displayBlocks.length === 0 ? (
          <p className="p-4 text-sm text-bunny-muted">{tr('web.notebook.empty')}</p>
        ) : (
          displayBlocks.map((block) => {
            const childOutput =
              block.kind === 'user_command' || block.kind === 'discord_command'
                ? outputForCommand(blocks, block.id)
                : undefined;
            return (
              <div key={block.id}>
                <BlockCard
                  block={block}
                  outputBlock={childOutput}
                  onStop={
                    childOutput?.status === 'running' ? () => void handleStop() : undefined
                  }
                />
              </div>
            );
          })
        )}
      </div>

      <div className="shrink-0 border-t border-bunny-border bg-bunny-panel/40">
        {inputLocked ? (
          <div className="flex flex-wrap items-center gap-2 px-3 py-2 text-xs font-medium text-yellow-600">
            <span>
              {interactiveSessionActive
                ? tr('web.notebook.interactiveLocked')
                : tr('web.notebook.inputLocked')}
            </span>
            {!interactiveSessionActive ? (
              <>
                <button
                  type="button"
                  className="rounded border border-yellow-600/40 px-2 py-0.5 hover:bg-yellow-500/10"
                  onClick={() => void handleStop()}
                >
                  {tr('web.notebook.stop')}
                </button>
                <span className="font-normal text-bunny-muted">
                  {tr('web.notebook.stopShortcut')}
                </span>
              </>
            ) : null}
          </div>
        ) : null}
        {!wsReady && !inputLocked ? (
          <p className="px-3 pt-2 text-xs text-bunny-muted">{tr('web.notebook.connecting')}</p>
        ) : null}
        {submitError ? (
          <p className="px-3 pt-2 text-xs text-red-600">{submitError}</p>
        ) : null}
        <form
          className="flex items-center gap-2 px-3 py-2"
          onSubmit={(e) => {
            e.preventDefault();
            void submitCommand(inputLine);
          }}
        >
          <span className="shrink-0 font-mono text-sm text-bunny-accent">
            {shellPrefix}$
          </span>
          <input
            ref={inputRef}
            type="text"
            value={inputLine}
            onChange={(e) => {
              if (historyIndex !== null) {
                setHistoryIndex(null);
              }
              setInputLine(e.target.value);
            }}
            onKeyDown={handleInputKeyDown}
            disabled={inputLocked}
            autoComplete="off"
            spellCheck={false}
            className="min-w-0 flex-1 bg-transparent font-mono text-sm text-bunny-fg outline-none placeholder:text-bunny-muted disabled:opacity-50"
            placeholder={tr('web.notebook.inputPlaceholder')}
            aria-label={tr('web.notebook.inputPlaceholder')}
          />
          <button
            type="button"
            className="shrink-0 rounded border border-bunny-border px-2 py-1 text-xs hover:bg-bunny-panel"
            onClick={() => setAttachOpen(true)}
          >
            {tr('web.notebook.attachTty')}
          </button>
        </form>
      </div>

      <AttachTtyDrawer
        terminalId={terminalId}
        open={attachOpen}
        interactive={interactiveSessionActive}
        onClose={() => {
          attachAutoOpenedRef.current = false;
          setAttachOpen(false);
          refreshShellPrefix();
        }}
        onStop={interactiveSessionActive ? () => void handleStop() : undefined}
        wheelScrollTui={interactiveSessionActive}
      />
    </div>
  );
}
