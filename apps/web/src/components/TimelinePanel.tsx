import { useEffect, useState } from 'react';
import { getTimeline } from '../lib/api';

interface Props {
  sessionId: string;
}

export default function TimelinePanel({ sessionId }: Props) {
  const [events, setEvents] = useState<
    Array<{ id: string; source: string; event_type: string; ts: string; payload: unknown }>
  >([]);

  useEffect(() => {
    const load = () =>
      getTimeline(sessionId).then(setEvents).catch(() => {});
    load();
    const id = setInterval(load, 3000);
    return () => clearInterval(id);
  }, [sessionId]);

  return (
    <div className="h-full overflow-auto text-xs font-mono p-2 space-y-1">
      {events.length === 0 && (
        <p className="text-bunny-muted">No timeline events yet.</p>
      )}
      {events.map((e) => (
        <div
          key={e.id}
          className="border-b border-bunny-border/50 py-1 flex gap-2"
        >
          <span className="text-bunny-muted shrink-0">{e.ts.slice(11, 19)}</span>
          <span className="text-bunny-accent shrink-0">{e.source}</span>
          <span className="text-yellow-500 shrink-0">{e.event_type}</span>
          <span className="truncate text-gray-400">
            {JSON.stringify(e.payload).slice(0, 120)}
          </span>
        </div>
      ))}
    </div>
  );
}
