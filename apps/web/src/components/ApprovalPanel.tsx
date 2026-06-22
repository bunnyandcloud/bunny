import { useCallback, useEffect, useState } from 'react';
import { listSessionApprovals, resolveApproval, type PendingApproval } from '../lib/api';
import { useT } from '../i18n';

interface Props {
  sessionId: string;
}

export default function ApprovalPanel({ sessionId }: Props) {
  const tr = useT();
  const [items, setItems] = useState<PendingApproval[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const res = await listSessionApprovals(sessionId);
      setItems(res.approvals);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load approvals');
    }
  }, [sessionId]);

  useEffect(() => {
    void refresh();
    const id = window.setInterval(() => void refresh(), 15_000);
    return () => window.clearInterval(id);
  }, [refresh]);

  const handleResolve = async (approvalId: string, approve: boolean) => {
    setBusy(approvalId);
    try {
      await resolveApproval(approvalId, approve);
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Approval failed');
    } finally {
      setBusy(null);
    }
  };

  if (items.length === 0 && !error) {
    return null;
  }

  return (
    <div className="border-b border-amber-600/40 bg-amber-950/30 px-3 py-2 shrink-0">
      <div className="flex items-center justify-between gap-2 mb-1">
        <span className="text-xs font-medium text-amber-200">
          {tr('web.approvals.pendingTitle') || 'Pending approvals'}
        </span>
        <button
          type="button"
          className="text-[10px] text-bunny-muted hover:text-bunny-text"
          onClick={() => void refresh()}
        >
          {tr('web.common.refresh') || 'Refresh'}
        </button>
      </div>
      {error && <p className="text-xs text-red-400 mb-1">{error}</p>}
      <ul className="space-y-2">
        {items.map((a) => (
          <li
            key={a.id}
            className="flex flex-col sm:flex-row sm:items-center gap-2 text-xs border border-bunny-border rounded p-2 bg-bunny-panel/60"
          >
            <div className="flex-1 min-w-0">
              <p className="font-medium truncate">{a.actionSummary}</p>
              {a.reason && (
                <p className="text-bunny-muted truncate mt-0.5">{a.reason}</p>
              )}
            </div>
            <div className="flex gap-2 shrink-0">
              <button
                type="button"
                disabled={busy === a.id}
                className="px-2 py-1 rounded bg-emerald-700 hover:bg-emerald-600 disabled:opacity-50"
                onClick={() => void handleResolve(a.id, true)}
              >
                {tr('web.approvals.allow') || 'Allow'}
              </button>
              <button
                type="button"
                disabled={busy === a.id}
                className="px-2 py-1 rounded bg-red-900 hover:bg-red-800 disabled:opacity-50"
                onClick={() => void handleResolve(a.id, false)}
              >
                {tr('web.approvals.deny') || 'Deny'}
              </button>
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}
