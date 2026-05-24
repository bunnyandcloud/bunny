import { useCallback, useEffect, useState } from 'react';
import { Panel, PanelGroup, PanelResizeHandle } from 'react-resizable-panels';
import {
  createTerminal,
  deleteTerminal,
  getSession,
  listSessionTerminals,
  renameSession,
  renameTerminal,
} from '../lib/api';
import InlineRename from './InlineRename';
import ConsolePanel from './ConsolePanel';
import NetworkPanel from './NetworkPanel';
import TerminalPanel from './TerminalPanel';
import TerminalShellBar, { type ShellTab } from './TerminalShellBar';
import TimelinePanel from './TimelinePanel';

interface Props {
  sessionId: string;
}

function nextShellName(existing: ShellTab[]): string {
  const used = new Set(existing.map((s) => s.name));
  let n = existing.length + 1;
  while (used.has(`shell ${n}`)) n += 1;
  return `shell ${n}`;
}

export default function SessionWorkspace({ sessionId }: Props) {
  const [sessionName, setSessionName] = useState('');
  const [shells, setShells] = useState<ShellTab[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [status, setStatus] = useState('connecting');
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getSession(sessionId)
      .then((s) => setSessionName(s.name))
      .catch(() => setSessionName(`Session ${sessionId.slice(0, 8)}`));
  }, [sessionId]);

  const refreshShells = useCallback(async () => {
    const list = await listSessionTerminals(sessionId);
    setShells(list);
    return list;
  }, [sessionId]);

  const openShell = useCallback(async (createIfEmpty: boolean) => {
    setBusy(true);
    setStatus('connecting');
    try {
      let list = await refreshShells();
      if (list.length === 0 && createIfEmpty) {
        const name = 'shell 1';
        const t = await createTerminal(sessionId, name, undefined);
        list = [{ id: t.id, name: t.name, status: 'Running' }];
        setShells(list);
      }
      if (list.length > 0) {
        setActiveId((prev) =>
          prev && list.some((s) => s.id === prev) ? prev : list[list.length - 1].id,
        );
        setStatus('ready');
      } else {
        setActiveId(null);
        setStatus('no shells');
      }
    } catch (e) {
      setStatus(String(e));
    } finally {
      setBusy(false);
    }
  }, [sessionId, refreshShells]);

  useEffect(() => {
    openShell(true);
  }, [openShell]);

  useEffect(() => {
    const onFocus = () => {
      refreshShells().then((list) => {
        if (list.length > 0) {
          setActiveId((prev) =>
            prev && list.some((s) => s.id === prev) ? prev : list[list.length - 1].id,
          );
        }
      });
    };
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, [refreshShells]);

  async function handleNewShell() {
    setBusy(true);
    try {
      const name = nextShellName(shells);
      const t = await createTerminal(sessionId, name, undefined);
      const list = await refreshShells();
      setActiveId(t.id);
      if (list.length === 0) {
        setShells([{ id: t.id, name: t.name, status: 'Running' }]);
      }
      setStatus('ready');
    } catch (e) {
      setStatus(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleRenameShell(id: string, name: string) {
    const updated = await renameTerminal(id, name);
    setShells((prev) =>
      prev.map((s) => (s.id === id ? { ...s, name: updated.name } : s)),
    );
  }

  async function handleCloseShell(id: string) {
    setBusy(true);
    try {
      await deleteTerminal(id);
      const list = await refreshShells();
      if (activeId === id) {
        setActiveId(list.length > 0 ? list[list.length - 1].id : null);
      }
      if (list.length === 0) {
        setStatus('no shells');
      }
    } catch (e) {
      setStatus(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="h-screen flex flex-col">
      <header className="flex items-center justify-between px-4 py-2 border-b border-bunny-border bg-bunny-panel gap-4">
        <button
          type="button"
          onClick={() => {
            location.href = '/';
          }}
          className="text-bunny-accent font-bold hover:opacity-80"
        >
          ← Sessions
        </button>
        <div className="flex-1 flex items-center justify-center gap-2 min-w-0 text-sm">
          {sessionName ? (
            <InlineRename
              value={sessionName}
              className="text-gray-200 font-medium truncate max-w-[min(100%,20rem)]"
              title="Double-click to rename session"
              onSave={async (name) => {
                const updated = await renameSession(sessionId, name);
                setSessionName(updated.name);
              }}
            />
          ) : (
            <span className="text-bunny-muted">Loading…</span>
          )}
          <span className="text-bunny-muted truncate">— {status}</span>
        </div>
        <div className="w-20" />
      </header>
      <PanelGroup direction="horizontal" className="flex-1">
        <Panel defaultSize={60} minSize={30}>
          <div className="h-full flex flex-col">
            <TerminalShellBar
              shells={shells}
              activeId={activeId}
              onSelect={setActiveId}
              onClose={handleCloseShell}
              onRename={handleRenameShell}
              onNew={handleNewShell}
              busy={busy}
            />
            <p className="px-2 py-1 text-xs text-bunny-muted border-b border-bunny-border">
              Shells run in tmux and survive agent restarts. Closing the browser only
              disconnects you — stop the agent, restart it, then re-open this session. Use ×
              to close a shell for good.
            </p>
            <div className="flex-1 min-h-0 relative">
              {shells.length > 0 ? (
                shells.map((shell) => {
                  const visible = shell.id === activeId;
                  return (
                    <div
                      key={shell.id}
                      className={
                        visible
                          ? 'absolute inset-0 flex flex-col'
                          : 'absolute inset-0 flex flex-col invisible pointer-events-none'
                      }
                      aria-hidden={!visible}
                    >
                      <TerminalPanel
                        terminalId={shell.id}
                        active={visible}
                      />
                    </div>
                  );
                })
              ) : (
                <div className="p-4 text-bunny-muted text-sm space-y-2">
                  <p>No shell open.</p>
                  <button
                    type="button"
                    className="text-xs px-3 py-1.5 border border-bunny-border rounded hover:bg-bunny-panel"
                    onClick={handleNewShell}
                    disabled={busy}
                  >
                    + New shell
                  </button>
                </div>
              )}
            </div>
          </div>
        </Panel>
        <PanelResizeHandle className="w-1 bg-bunny-border hover:bg-bunny-accent" />
        <Panel defaultSize={40} minSize={20}>
          <PanelGroup direction="vertical">
            <Panel defaultSize={40}>
              <div className="h-full flex flex-col border-b border-bunny-border">
                <div className="px-2 py-1 text-xs text-bunny-muted">Console</div>
                <ConsolePanel sessionId={sessionId} />
              </div>
            </Panel>
            <PanelResizeHandle className="h-1 bg-bunny-border" />
            <Panel defaultSize={30}>
              <div className="h-full flex flex-col border-b border-bunny-border">
                <div className="px-2 py-1 text-xs text-bunny-muted">Network (metadata only)</div>
                <NetworkPanel sessionId={sessionId} />
              </div>
            </Panel>
            <PanelResizeHandle className="h-1 bg-bunny-border" />
            <Panel defaultSize={30}>
              <div className="h-full flex flex-col">
                <div className="px-2 py-1 text-xs text-bunny-muted">Timeline</div>
                <TimelinePanel sessionId={sessionId} />
              </div>
            </Panel>
          </PanelGroup>
        </Panel>
      </PanelGroup>
    </div>
  );
}
