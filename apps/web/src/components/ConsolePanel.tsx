import { useEffect, useState } from 'react';

interface ConsoleEvent {
  level: string;
  text: string;
  ts?: string;
}

interface Props {
  sessionId: string;
}

export default function ConsolePanel({ sessionId }: Props) {
  const [logs, setLogs] = useState<ConsoleEvent[]>([]);

  useEffect(() => {
    const proto = location.protocol === 'https:' ? 'wss' : 'ws';
    const ws = new WebSocket(`${proto}://${location.host}/api/v1/sessions/${sessionId}/realtime`);
    ws.onmessage = (ev) => {
      try {
        const msg = JSON.parse(ev.data);
        if (msg.type === 'browser.console') {
          setLogs((prev) => [
            ...prev.slice(-199),
            { level: msg.level, text: msg.text, ts: msg.ts },
          ]);
        }
      } catch {
        /* ignore */
      }
    };
    return () => ws.close();
  }, [sessionId]);

  return (
    <div className="h-full overflow-auto p-2 font-mono text-xs space-y-1">
      {logs.length === 0 && (
        <p className="text-bunny-muted">Console events appear when CDP collector is running.</p>
      )}
      {logs.map((l, i) => (
        <div key={i} className="flex gap-2">
          <span
            className={
              l.level === 'error'
                ? 'text-red-400'
                : l.level === 'warn'
                  ? 'text-yellow-400'
                  : 'text-gray-400'
            }
          >
            [{l.level}]
          </span>
          <span className="break-all">{l.text}</span>
        </div>
      ))}
    </div>
  );
}
