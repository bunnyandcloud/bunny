import { forwardRef, useEffect, useImperativeHandle, useRef } from 'react';
import { FitAddon } from '@xterm/addon-fit';
import { WebLinksAddon } from '@xterm/addon-web-links';
import { Terminal } from '@xterm/xterm';
import { terminalWsUrl } from '../lib/api';
import { filterClientInput, filterServerOutput } from '../lib/terminalSanitize';
import '@xterm/xterm/css/xterm.css';

export interface TerminalPanelHandle {
  /** Insert text at the cursor. Returns true when sent over the live WebSocket. */
  inject: (text: string) => boolean;
  focus: () => void;
}

interface Props {
  terminalId: string;
  /** When false, panel stays connected but hidden (multi-tab shells). */
  active?: boolean;
  /** When false, do not steal focus on activate (avoids host clipboard paste into xterm). */
  autoFocus?: boolean;
  readonly?: boolean;
}

type ReplayMode = 'none' | 'catch_up' | 'recovery';

function offsetStorageKey(terminalId: string): string {
  return `bunny:term:${terminalId}:offset`;
}

function readStoredOffset(terminalId: string): number {
  try {
    const raw = localStorage.getItem(offsetStorageKey(terminalId));
    if (!raw) return 0;
    const n = Number.parseInt(raw, 10);
    return Number.isFinite(n) && n >= 0 ? n : 0;
  } catch {
    return 0;
  }
}

function writeStoredOffset(terminalId: string, offset: number) {
  try {
    localStorage.setItem(offsetStorageKey(terminalId), String(offset));
  } catch {
    /* private mode / quota */
  }
}

const TerminalPanel = forwardRef<TerminalPanelHandle, Props>(function TerminalPanel(
  { terminalId, active = true, autoFocus = true, readonly },
  ref,
) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const injectFnRef = useRef<(text: string) => boolean>(() => false);
  const offsetRef = useRef(readStoredOffset(terminalId));
  const fitRef = useRef<FitAddon | null>(null);
  const activeRef = useRef(active);
  activeRef.current = active;
  const connectedOnceRef = useRef(false);

  useImperativeHandle(
    ref,
    () => ({
      inject(text: string) {
        return injectFnRef.current(text);
      },
      focus() {
        termRef.current?.focus();
      },
    }),
    [],
  );

  useEffect(() => {
    if (!active || !termRef.current || !fitRef.current || !containerRef.current) {
      return;
    }
    fitRef.current.fit();
    if (autoFocus) {
      termRef.current.focus();
    }
    if (wsRef.current?.readyState === WebSocket.OPEN && termRef.current) {
      wsRef.current.send(
        JSON.stringify({
          type: 'resize',
          cols: termRef.current.cols,
          rows: termRef.current.rows,
        }),
      );
    }
  }, [active, autoFocus]);

  useEffect(() => {
    if (!containerRef.current) return;
    // Defer WS until this tab is shown at least once — avoids hidden shells disturbing live sessions.
    if (!active && !connectedOnceRef.current) return;

    const term = new Terminal({
      cursorBlink: true,
      fontSize: 14,
      fontFamily: 'Menlo, Monaco, Consolas, monospace',
      convertEol: true,
      scrollback: 10000,
      theme: {
        background: '#0d1117',
        foreground: '#c9d1d9',
        cursor: '#58a6ff',
      },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new WebLinksAddon());
    term.open(containerRef.current);
    fit.fit();
    fitRef.current = fit;
    if (active && autoFocus) {
      term.focus();
    }
    termRef.current = term;

    let reconnectTimer: ReturnType<typeof setTimeout>;
    let resizeAfterReplayTimer: ReturnType<typeof setTimeout>;
    let disposed = false;
    let reconnectAttempts = 0;
    const maxReconnectAttempts = 5;
    let gaveUp = false;

    let suppressNextClear = false;
    let replayPhaseDone = false;
    let liveFence = 0;
    /** Fresh xterm mount (F5): replay full buffer. WS reconnect: incremental catch-up only. */
    let isFirstConnect = true;

    function rememberOffset(offset: number) {
      if (offset > offsetRef.current) {
        offsetRef.current = offset;
        writeStoredOffset(terminalId, offset);
      }
    }

    function stripLeadingScreenClear(data: string): string {
      return data.replace(/^(\x1b\[[0-9;?0-9]*[a-zA-Z])+/u, '');
    }

    function writeOutput(data: string, fromReplay = false) {
      let payload = data;
      if (!fromReplay && suppressNextClear) {
        suppressNextClear = false;
        payload = stripLeadingScreenClear(payload);
      }
      const clean = filterServerOutput(payload);
      if (clean) {
        term.write(clean);
      }
    }

    function sendResize() {
      try {
        fit.fit();
      } catch {
        /* container may not be laid out yet */
      }
      const cols = Math.max(term.cols, 80);
      const rows = Math.max(term.rows, 24);
      if (wsRef.current?.readyState === WebSocket.OPEN) {
        wsRef.current.send(JSON.stringify({ type: 'resize', cols, rows }));
      }
    }

    function sendResizeDeferred() {
      sendResize();
      requestAnimationFrame(() => sendResize());
    }

    function replayMode(msg: Record<string, unknown>): ReplayMode {
      const mode = msg.replay_mode;
      if (mode === 'catch_up' || mode === 'recovery' || mode === 'none') {
        return mode;
      }
      return msg.has_history ? 'recovery' : 'none';
    }

    function handleReplay(msg: Record<string, unknown>) {
      clearTimeout(resizeAfterReplayTimer);
      replayPhaseDone = false;

      const mode = replayMode(msg);
      liveFence = typeof msg.snapshot_offset === 'number' ? msg.snapshot_offset : 0;
      const chunks = Array.isArray(msg.chunks) ? msg.chunks : [];

      if (mode === 'recovery') {
        term.reset();
        suppressNextClear = true;
        for (const c of chunks) {
          if (c && typeof c === 'object' && 'data' in c) {
            writeOutput(String((c as { data: string }).data), true);
            const off = (c as { offset?: number }).offset;
            if (typeof off === 'number') {
              rememberOffset(off);
            }
          }
        }
      } else if (mode === 'catch_up') {
        suppressNextClear = chunks.length > 0;
        for (const c of chunks) {
          if (c && typeof c === 'object' && 'data' in c) {
            writeOutput(String((c as { data: string }).data), true);
            const off = (c as { offset?: number }).offset;
            if (typeof off === 'number') {
              rememberOffset(off);
            }
          }
        }
      }

      replayPhaseDone = true;
      sendResizeDeferred();
    }

    function connect() {
      if (gaveUp) return;
      const ws = new WebSocket(terminalWsUrl(terminalId));
      wsRef.current = ws;
      let opened = false;
      replayPhaseDone = false;
      liveFence = 0;

      ws.onopen = () => {
        opened = true;
        connectedOnceRef.current = true;
        reconnectAttempts = 0;
        sendResize();
        // F5 recreates an empty xterm — must replay from 0, not last stored offset.
        const fromOffset = isFirstConnect ? 0 : offsetRef.current;
        isFirstConnect = false;
        ws.send(
          JSON.stringify({
            type: 'subscribe',
            from_offset: fromOffset,
          }),
        );
        sendResizeDeferred();
        resizeAfterReplayTimer = setTimeout(sendResizeDeferred, 100);
      };

      ws.onmessage = (ev) => {
        try {
          const msg = JSON.parse(ev.data);
          if (msg.type === 'error') {
            gaveUp = true;
            term.write(
              `\r\n\x1b[31m${msg.message ?? 'Shell unavailable'}\x1b[0m\r\n`,
            );
            ws.close();
          } else if (msg.type === 'replay') {
            handleReplay(msg);
          } else if (msg.type === 'output') {
            if (!replayPhaseDone) return;
            const offset = typeof msg.offset === 'number' ? msg.offset : 0;
            if (offset <= liveFence) return;
            writeOutput(msg.data);
            rememberOffset(offset);
          }
        } catch {
          /* ignore */
        }
      };

      ws.onclose = () => {
        if (disposed || gaveUp) return;
        if (!opened) {
          reconnectAttempts += 1;
        } else {
          reconnectAttempts = 0;
        }
        if (reconnectAttempts >= maxReconnectAttempts) {
          gaveUp = true;
          term.write(
            '\r\n\x1b[31mCould not connect to this shell. Close the tab with × and open a new shell.\x1b[0m\r\n',
          );
          return;
        }
        term.write('\r\n\x1b[33m[reconnecting…]\x1b[0m\r\n');
        reconnectTimer = setTimeout(connect, 1500);
      };
    }

    connect();

    if (!readonly) {
      term.onData((data) => {
        const filtered = filterClientInput(data);
        if (filtered && wsRef.current?.readyState === WebSocket.OPEN) {
          wsRef.current.send(JSON.stringify({ type: 'input', data: filtered }));
        }
      });
    }

    injectFnRef.current = (text: string): boolean => {
      const term = termRef.current;
      const ws = wsRef.current;
      if (!term || readonly) return false;
      const filtered = filterClientInput(text);
      if (!filtered) return false;

      term.write(filtered);

      if (ws?.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ type: 'input', data: filtered }));
        return true;
      }
      return false;
    };

    const onResize = () => sendResizeDeferred();
    window.addEventListener('resize', onResize);

    const resizeObserver = new ResizeObserver(() => {
      if (wsRef.current?.readyState === WebSocket.OPEN) {
        sendResize();
      }
    });
    resizeObserver.observe(containerRef.current);

    return () => {
      disposed = true;
      clearTimeout(reconnectTimer);
      clearTimeout(resizeAfterReplayTimer);
      window.removeEventListener('resize', onResize);
      resizeObserver.disconnect();
      wsRef.current?.close();
      fitRef.current = null;
      termRef.current = null;
      injectFnRef.current = () => false;
      term.dispose();
    };
  }, [terminalId, readonly, active]);

  return (
    <div className="bunny-terminal-host flex h-full w-full min-h-0">
      <div
        ref={containerRef}
        className="flex-1 min-h-0 min-w-0 p-1 bg-bunny-bg rounded-l"
      />
    </div>
  );
});

export default TerminalPanel;
