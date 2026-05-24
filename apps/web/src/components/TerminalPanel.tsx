import { FitAddon } from '@xterm/addon-fit';
import { WebLinksAddon } from '@xterm/addon-web-links';
import { Terminal } from '@xterm/xterm';
import { useEffect, useRef } from 'react';
import { terminalWsUrl } from '../lib/api';
import { filterClientInput, filterServerOutput } from '../lib/terminalSanitize';
import '@xterm/xterm/css/xterm.css';

interface Props {
  terminalId: string;
  /** When false, panel stays connected but hidden (multi-tab shells). */
  active?: boolean;
  readonly?: boolean;
}

export default function TerminalPanel({
  terminalId,
  active = true,
  readonly,
}: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const offsetRef = useRef(0);
  const fitRef = useRef<FitAddon | null>(null);
  const activeRef = useRef(active);
  activeRef.current = active;

  useEffect(() => {
    if (!active || !termRef.current || !fitRef.current || !containerRef.current) {
      return;
    }
    fitRef.current.fit();
    termRef.current.focus();
    // Resize only — `refresh` redraws tmux's live screen (~24 lines) and wipes replayed scrollback.
    if (wsRef.current?.readyState === WebSocket.OPEN && termRef.current) {
      wsRef.current.send(
        JSON.stringify({
          type: 'resize',
          cols: termRef.current.cols,
          rows: termRef.current.rows,
        }),
      );
    }
  }, [active]);

  useEffect(() => {
    if (!containerRef.current) return;

    const term = new Terminal({
      cursorBlink: true,
      fontSize: 14,
      fontFamily: 'Menlo, Monaco, Consolas, monospace',
      windowOptions: {
        setWinLines: false,
        setWinColumns: false,
        refreshWin: false,
        setWinSizePixels: false,
        setTitle: false,
        pushTitle: false,
      },
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
    if (active) {
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
      if (!activeRef.current) return;
      fit.fit();
      if (wsRef.current?.readyState === WebSocket.OPEN) {
        wsRef.current.send(
          JSON.stringify({ type: 'resize', cols: term.cols, rows: term.rows }),
        );
      }
    }

    function sendResizeDeferred() {
      requestAnimationFrame(() => {
        requestAnimationFrame(() => sendResize());
      });
    }

    function connect() {
      if (gaveUp) return;
      const ws = new WebSocket(terminalWsUrl(terminalId, offsetRef.current));
      wsRef.current = ws;
      let replaySeen = false;
      let opened = false;

      ws.onopen = () => {
        opened = true;
        reconnectAttempts = 0;
        ws.send(JSON.stringify({ type: 'subscribe', from_offset: offsetRef.current }));
        resizeAfterReplayTimer = setTimeout(() => {
          if (!replaySeen) sendResizeDeferred();
        }, 150);
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
          } else if (msg.type === 'output') {
            writeOutput(msg.data);
            offsetRef.current = msg.offset ?? offsetRef.current;
          } else if (msg.type === 'replay' && msg.chunks) {
            replaySeen = true;
            clearTimeout(resizeAfterReplayTimer);
            if (msg.has_history) {
              suppressNextClear = true;
            }
            for (const c of msg.chunks) {
              writeOutput(c.data, true);
              offsetRef.current = c.offset;
            }
            sendResizeDeferred();
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

    const onResize = () => sendResizeDeferred();
    window.addEventListener('resize', onResize);

    return () => {
      disposed = true;
      clearTimeout(reconnectTimer);
      clearTimeout(resizeAfterReplayTimer);
      window.removeEventListener('resize', onResize);
      wsRef.current?.close();
      fitRef.current = null;
      term.dispose();
    };
  }, [terminalId, readonly]);

  return (
    <div
      ref={containerRef}
      className="h-full w-full min-h-[200px] p-1 bg-bunny-bg rounded"
    />
  );
}
