import { useEffect, useState } from 'react';

interface NetEvent {
  requestId: string;
  method?: string;
  urlRedacted?: string;
  status?: number;
  phase: string;
}

interface Props {
  sessionId: string;
}

export default function NetworkPanel({ sessionId }: Props) {
  const [requests, setRequests] = useState<NetEvent[]>([]);

  useEffect(() => {
    const proto = location.protocol === 'https:' ? 'wss' : 'ws';
    const ws = new WebSocket(`${proto}://${location.host}/api/v1/sessions/${sessionId}/realtime`);
    ws.onmessage = (ev) => {
      try {
        const msg = JSON.parse(ev.data);
        if (msg.type === 'browser.network') {
          setRequests((prev) => {
            const id = msg.requestId as string;
            const existing = prev.findIndex((r) => r.requestId === id);
            const entry: NetEvent = {
              requestId: id,
              method: msg.method,
              urlRedacted: msg.urlRedacted,
              status: msg.status,
              phase: msg.phase,
            };
            if (existing >= 0) {
              const next = [...prev];
              next[existing] = { ...next[existing], ...entry };
              return next;
            }
            return [...prev.slice(-99), entry];
          });
        }
      } catch {
        /* ignore */
      }
    };
    return () => ws.close();
  }, [sessionId]);

  return (
    <div className="h-full overflow-auto text-xs font-mono">
      <table className="w-full">
        <thead className="text-bunny-muted sticky top-0 bg-bunny-panel">
          <tr>
            <th className="text-left p-1">Method</th>
            <th className="text-left p-1">URL</th>
            <th className="text-left p-1">Status</th>
            <th className="text-left p-1">Phase</th>
          </tr>
        </thead>
        <tbody>
          {requests.map((r) => (
            <tr key={r.requestId} className="border-t border-bunny-border/30">
              <td className="p-1">{r.method ?? '—'}</td>
              <td className="p-1 truncate max-w-[200px]">{r.urlRedacted ?? '—'}</td>
              <td className="p-1">{r.status ?? '—'}</td>
              <td className="p-1">{r.phase}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
