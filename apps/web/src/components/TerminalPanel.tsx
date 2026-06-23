import { forwardRef, useEffect, useImperativeHandle, useRef } from 'react';
import { FitAddon } from '@xterm/addon-fit';
import { WebLinksAddon } from '@xterm/addon-web-links';
import { Terminal } from '@xterm/xterm';
import { terminalWsUrl } from '../lib/api';
import { filterClientInput, filterServerOutput } from '../lib/terminalSanitize';
import { getTerminalTheme } from '../lib/terminalThemes';
import { useTerminalTheme } from '../store/terminalTheme';
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
  /** Compact embed inside a notebook block (smaller font, no outer flex host). */
  embedded?: boolean;
  /** Attach to live pane only — skip scrollback replay, sync with refresh. */
  liveAttach?: boolean;
  /** Map mouse wheel to arrow keys for TUIs (htop, less, etc.). */
  wheelScrollTui?: boolean;
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
  { terminalId, active = true, autoFocus = true, readonly, embedded = false, liveAttach = false, wheelScrollTui = false },
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
  const terminalThemeId = useTerminalTheme((s) => s.themeId);

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
      fontSize: embedded ? 13 : 14,
      fontFamily: 'Menlo, Monaco, Consolas, monospace',
      convertEol: true,
      scrollback: embedded && liveAttach ? 0 : embedded ? 1000 : 10000,
      theme: getTerminalTheme(terminalThemeId),
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
      const clean = filterServerOutput(payload, {
        preserveAltScreen: embedded && liveAttach,
      });
      if (clean) {
        term.write(clean);
      }
    }

    let resizeTimer: ReturnType<typeof setTimeout>;
    let lastSizedCols = 0;
    let lastSizedRows = 0;

    function sendResize() {
      try {
        fit.fit();
      } catch {
        /* container may not be laid out yet */
      }
      const cols = embedded
        ? Math.max(term.cols, 20)
        : Math.max(term.cols, 80);
      const rows = embedded
        ? Math.max(term.rows, 6)
        : Math.max(term.rows, 24);
      const sizeChanged = cols !== lastSizedCols || rows !== lastSizedRows;
      if (embedded && !sizeChanged) {
        return;
      }
      lastSizedCols = cols;
      lastSizedRows = rows;
      if (wsRef.current?.readyState === WebSocket.OPEN) {
        wsRef.current.send(JSON.stringify({ type: 'resize', cols, rows }));
        if (embedded && liveAttach && sizeChanged) {
          setTimeout(() => {
            if (wsRef.current?.readyState === WebSocket.OPEN) {
              wsRef.current.send(JSON.stringify({ type: 'refresh' }));
            }
          }, 50);
        }
      }
    }

    function sendResizeDeferred() {
      if (embedded) {
        clearTimeout(resizeTimer);
        resizeTimer = setTimeout(() => sendResize(), 120);
        return;
      }
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
        if (liveAttach) {
          replayPhaseDone = true;
        }
        sendResize();
        const fromOffset = liveAttach
          ? Number.MAX_SAFE_INTEGER
          : isFirstConnect
            ? 0
            : offsetRef.current;
        isFirstConnect = false;
        ws.send(
          JSON.stringify({
            type: 'subscribe',
            from_offset: fromOffset,
          }),
        );
        if (liveAttach) {
          ws.send(JSON.stringify({ type: 'refresh' }));
          setTimeout(() => {
            if (wsRef.current?.readyState === WebSocket.OPEN) {
              wsRef.current.send(JSON.stringify({ type: 'refresh' }));
              sendResizeDeferred();
            }
          }, 300);
        }
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

    function sendTuiInput(data: string) {
      const filtered = filterClientInput(data);
      if (!filtered || readonly) return;
      if (wsRef.current?.readyState === WebSocket.OPEN) {
        wsRef.current.send(JSON.stringify({ type: 'input', data: filtered }));
      }
    }

    function wheelToTuiArrows(e: WheelEvent): boolean {
      e.preventDefault();
      e.stopPropagation();
      let delta = e.deltaY;
      if (e.deltaMode === WheelEvent.DOM_DELTA_LINE) {
        delta *= 16;
      } else if (e.deltaMode === WheelEvent.DOM_DELTA_PAGE) {
        delta *= 320;
      }
      const steps = Math.min(10, Math.max(1, Math.round(Math.abs(delta) / 40)));
      const seq = delta > 0 ? '\x1b[B' : '\x1b[A';
      for (let i = 0; i < steps; i++) {
        sendTuiInput(seq);
      }
      return false;
    }

    if (embedded || wheelScrollTui) {
      term.attachCustomWheelEventHandler(wheelToTuiArrows);
    }

    const onResize = () => sendResizeDeferred();
    window.addEventListener('resize', onResize);

    const resizeObserver = new ResizeObserver(() => {
      if (wsRef.current?.readyState === WebSocket.OPEN) {
        sendResizeDeferred();
      }
    });
    resizeObserver.observe(containerRef.current);

    return () => {
      disposed = true;
      clearTimeout(reconnectTimer);
      clearTimeout(resizeAfterReplayTimer);
      clearTimeout(resizeTimer);
      window.removeEventListener('resize', onResize);
      resizeObserver.disconnect();
      wsRef.current?.close();
      fitRef.current = null;
      termRef.current = null;
      injectFnRef.current = () => false;
      term.dispose();
    };
  }, [terminalId, readonly, active, embedded, liveAttach, wheelScrollTui]);

  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.theme = getTerminalTheme(terminalThemeId);
  }, [terminalThemeId]);

  return (
    <div className={embedded ? 'h-full w-full min-h-0' : 'bunny-terminal-host flex h-full w-full min-h-0'}>
      <div
        ref={containerRef}
        className={
          embedded
            ? 'h-full w-full min-h-0 p-1'
            : 'flex-1 min-h-0 min-w-0 p-1 bg-bunny-bg rounded-l'
        }
      />
    </div>
  );
});

export default TerminalPanel;
