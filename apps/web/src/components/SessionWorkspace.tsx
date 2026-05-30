import { useCallback, useEffect, useRef, useState } from 'react';
import { Panel, PanelGroup, PanelResizeHandle } from 'react-resizable-panels';
import {
  createTerminal,
  deleteTerminal,
  getSecretsStatus,
  getSession,
  listSecrets,
  listSessionTerminals,
  renameSession,
  renameTerminal,
  sendTerminalInput,
  type SecretMeta,
  type VaultStatus,
} from '../lib/api';
import { useAuth } from '../store/auth';
import InlineRename from './InlineRename';
import SecretsVaultBanner from './SecretsVaultBanner';
import TerminalPanel, { type TerminalPanelHandle } from './TerminalPanel';
import TerminalShellBar, { type ShellTab } from './TerminalShellBar';
import VaultInjectPanel from './VaultInjectPanel';
import VaultUnlockModal from './VaultUnlockModal';
import PreviewPanel from './PreviewPanel';
import BrowserPanel from './BrowserPanel';
import ClaudeSetupPanel from './ClaudeSetupPanel';
import LogoutButton from './LogoutButton';
import SessionMembersModal from './SessionMembersModal';
import SessionDiscordModal from './SessionDiscordModal';

function browserStorageKey(sessionId: string) {
  return `bunny-browser-id:${sessionId}`;
}

function isClaudeSetupMode() {
  return new URLSearchParams(location.search).get('claude') === 'setup';
}

interface Props {
  sessionId: string;
}

type WorkspaceTab = 'terminal' | 'preview' | 'browser';

function nextShellName(existing: ShellTab[]): string {
  const used = new Set(existing.map((s) => s.name));
  let n = existing.length + 1;
  while (used.has(`shell ${n}`)) n += 1;
  return `shell ${n}`;
}

function secretsForSession(secrets: SecretMeta[], sessionId: string): SecretMeta[] {
  return secrets.filter(
    (s) =>
      s.scope === 'system' ||
      s.scope === 'project' ||
      (s.scope === 'session' && s.session_id === sessionId),
  );
}

export default function SessionWorkspace({ sessionId }: Props) {
  const { user } = useAuth();
  const [sessionName, setSessionName] = useState('');
  const [shells, setShells] = useState<ShellTab[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [status, setStatus] = useState('connecting');
  const [busy, setBusy] = useState(false);
  const [vaultStatus, setVaultStatus] = useState<VaultStatus | null>(null);
  const [unlockOpen, setUnlockOpen] = useState(false);
  const [secrets, setSecrets] = useState<SecretMeta[]>([]);
  const [vaultCollapsed, setVaultCollapsed] = useState(false);
  const [workspaceTab, setWorkspaceTab] = useState<WorkspaceTab>('terminal');
  const [suppressTerminalFocus, setSuppressTerminalFocus] = useState(false);
  const [previewPort, setPreviewPort] = useState(3000);
  const [claudeBrowserId, setClaudeBrowserId] = useState<string | null>(() =>
    sessionStorage.getItem(browserStorageKey(sessionId)),
  );
  const [membersOpen, setMembersOpen] = useState(false);
  const [discordOpen, setDiscordOpen] = useState(false);
  const claudeSetup = isClaudeSetupMode();
  const terminalRefs = useRef(new Map<string, TerminalPanelHandle>());

  const refreshVaultStatus = useCallback(async () => {
    if (!user?.isOwner) {
      setVaultStatus(null);
      return;
    }
    try {
      const s = await getSecretsStatus();
      setVaultStatus(s);
    } catch {
      setVaultStatus(null);
    }
  }, [user?.isOwner]);

  const refreshSecrets = useCallback(async () => {
    if (!user?.isOwner || vaultStatus?.status !== 'unlocked') {
      setSecrets([]);
      return;
    }
    try {
      const list = await listSecrets();
      setSecrets(list);
    } catch {
      setSecrets([]);
    }
  }, [user?.isOwner, vaultStatus?.status]);

  const handleVaultUnlocked = useCallback(async () => {
    if (!user?.isOwner) return;
    try {
      const s = await getSecretsStatus();
      setVaultStatus(s);
      if (s.status === 'unlocked') {
        setSecrets(await listSecrets());
      } else {
        setSecrets([]);
      }
    } catch {
      setVaultStatus(null);
      setSecrets([]);
    }
    setVaultCollapsed(false);
  }, [user?.isOwner]);

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
    openShell(!claudeSetup);
  }, [openShell, claudeSetup]);

  useEffect(() => {
    refreshVaultStatus();
  }, [refreshVaultStatus]);

  useEffect(() => {
    if (user?.isOwner && vaultStatus?.status === 'unlocked') {
      refreshSecrets();
    }
  }, [user?.isOwner, vaultStatus?.status, refreshSecrets]);

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

  const vaultLocked = vaultStatus?.status === 'locked';
  const vaultUnlocked = vaultStatus?.status === 'unlocked';
  const hasStoredSecrets = (vaultStatus?.ref_count ?? 0) > 0;
  const showVaultBanner = user?.isOwner && vaultLocked;
  const showSidebarSecretsHint =
    user?.isOwner && vaultLocked && hasStoredSecrets && shells.length === 0;
  const sessionSecrets = secretsForSession(secrets, sessionId);
  const showVaultSection = user?.isOwner && !vaultCollapsed;

  async function handleInjectSecret(envVar: string) {
    if (!activeId) {
      setStatus('No active shell');
      return;
    }
    const text = `$${envVar}`;
    const panel = terminalRefs.current.get(activeId);
    try {
      const sentViaWs = panel?.inject(text) ?? false;
      if (!sentViaWs) {
        await sendTerminalInput(activeId, text);
      }
      panel?.focus();
    } catch (e) {
      setStatus(String(e));
    }
  }

  function handleVaultButtonClick() {
    if (vaultLocked) {
      setUnlockOpen(true);
      return;
    }
    setVaultCollapsed((collapsed) => !collapsed);
  }

  return (
    <div className="h-screen flex flex-col">
      <SessionMembersModal
        open={membersOpen}
        sessionId={sessionId}
        onClose={() => setMembersOpen(false)}
      />
      <SessionDiscordModal
        open={discordOpen}
        sessionId={sessionId}
        onClose={() => setDiscordOpen(false)}
      />
      <VaultUnlockModal
        open={unlockOpen}
        onClose={() => setUnlockOpen(false)}
        onUnlocked={handleVaultUnlocked}
        sessionId={sessionId}
      />
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
        <div className="flex items-center justify-end gap-2 shrink-0 min-w-[5rem]">
          <button
            type="button"
            onClick={() => setDiscordOpen(true)}
            className="text-xs px-2.5 py-1 rounded border border-bunny-border text-bunny-muted hover:text-gray-200 hover:bg-bunny-bg"
            title="Link Discord channel to this session"
          >
            Discord
          </button>
          <button
            type="button"
            onClick={() => setMembersOpen(true)}
            className="text-xs px-2.5 py-1 rounded border border-bunny-border text-bunny-muted hover:text-gray-200 hover:bg-bunny-bg"
            title="Manage session members"
          >
            Members
          </button>
          {user?.isOwner && (
            <button
              type="button"
              onClick={handleVaultButtonClick}
              className={`text-xs px-2.5 py-1 rounded border font-medium ${
                showVaultSection
                  ? 'border-bunny-accent bg-bunny-accent/10 text-bunny-accent'
                  : vaultLocked
                    ? 'border-orange-400/40 text-orange-300 hover:bg-orange-400/10'
                    : 'border-bunny-border text-bunny-muted hover:text-bunny-accent hover:bg-bunny-bg'
              }`}
              title={
                vaultLocked
                  ? 'Vault verrouillé — cliquer pour déverrouiller'
                  : showVaultSection
                    ? 'Masquer le panneau vault'
                    : 'Afficher le panneau vault'
              }
            >
              Vault{vaultLocked ? ' 🔒' : ''}
            </button>
          )}
          <LogoutButton className="text-xs text-bunny-muted hover:text-gray-200 disabled:opacity-50" />
        </div>
      </header>
      {showVaultBanner && (
        <SecretsVaultBanner onUnlock={() => setUnlockOpen(true)} />
      )}
      <PanelGroup direction="horizontal" className="flex-1">
        <Panel
          defaultSize={user?.isOwner && showVaultSection ? 75 : 100}
          minSize={30}
        >
          <div className="h-full flex flex-col">
            {claudeSetup && (
              <div className="shrink-0 p-2 border-b border-bunny-border">
                <ClaudeSetupPanel
                  sessionId={sessionId}
                  browserId={claudeBrowserId}
                  onBrowserId={(id) => {
                    setClaudeBrowserId(id);
                    sessionStorage.setItem(browserStorageKey(sessionId), id);
                  }}
                  onOpenBrowserTab={() => setWorkspaceTab('browser')}
                  onOpenTerminalTab={(terminalId) => {
                    setWorkspaceTab('terminal');
                    setSuppressTerminalFocus(true);
                    void refreshShells().then((list) => {
                      if (terminalId && list.some((s) => s.id === terminalId)) {
                        setActiveId(terminalId);
                      }
                    });
                    window.setTimeout(() => setSuppressTerminalFocus(false), 2500);
                  }}
                />
              </div>
            )}
            <div className="flex items-center gap-1 px-2 py-1 border-b border-bunny-border bg-bunny-panel shrink-0">
              {(
                [
                  ['terminal', 'Terminal'],
                  ['preview', 'Preview'],
                  ['browser', 'Browser'],
                ] as const
              ).map(([id, label]) => (
                <button
                  key={id}
                  type="button"
                  onClick={() => setWorkspaceTab(id)}
                  className={`text-xs px-2.5 py-1 rounded border ${
                    workspaceTab === id
                      ? 'border-bunny-accent text-bunny-accent bg-bunny-accent/10'
                      : 'border-transparent text-bunny-muted hover:text-gray-200'
                  }`}
                >
                  {label}
                </button>
              ))}
            </div>
            {workspaceTab === 'preview' && (
              <PreviewPanel
                sessionId={sessionId}
                defaultPort={previewPort}
                onPortChange={setPreviewPort}
              />
            )}
            {workspaceTab === 'browser' && (
              <BrowserPanel sessionId={sessionId} targetPort={previewPort} />
            )}
            {workspaceTab === 'terminal' && (
              <>
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
                        ref={(handle) => {
                          if (handle) {
                            terminalRefs.current.set(shell.id, handle);
                          } else {
                            terminalRefs.current.delete(shell.id);
                          }
                        }}
                        terminalId={shell.id}
                        active={visible}
                        autoFocus={!suppressTerminalFocus}
                      />
                    </div>
                  );
                })
              ) : (
                <div className="p-4 text-bunny-muted text-sm space-y-2">
                  <p>No shell open.</p>
                  {vaultLocked && hasStoredSecrets && (
                    <p className="text-xs text-orange-300/80">
                      Le vault est verrouillé — déverrouille-le puis ouvre un shell pour injecter les
                      secrets (<code className="text-gray-400">BUNNY_SECRET_*</code>).
                    </p>
                  )}
                  {vaultStatus?.status === 'unlocked' && hasStoredSecrets && (
                    <p className="text-xs text-bunny-muted">
                      Déverrouille le vault pour injecter les variables{' '}
                      <code className="text-gray-400">BUNNY_SECRET_*</code> dans les shells ouverts.
                    </p>
                  )}
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
              </>
            )}
          </div>
        </Panel>
        {user?.isOwner && showVaultSection && (
          <>
            <PanelResizeHandle className="w-1 bg-bunny-border hover:bg-bunny-accent" />
            <Panel defaultSize={25} minSize={15}>
              <div className="h-full flex flex-col bg-bunny-bg">
                {showSidebarSecretsHint && (
                  <p className="px-2 py-1.5 text-[11px] leading-snug text-bunny-muted border-b border-bunny-border bg-bunny-panel/80 shrink-0">
                    Vault verrouillé — {vaultStatus?.ref_count} secret
                    {(vaultStatus?.ref_count ?? 0) > 1 ? 's' : ''} en attente.{' '}
                    <button
                      type="button"
                      className="text-bunny-accent hover:underline"
                      onClick={() => setUnlockOpen(true)}
                    >
                      Unlock
                    </button>{' '}
                    puis nouveau shell.
                  </p>
                )}
                <div className="flex-1 min-h-0">
                  <VaultInjectPanel
                    secrets={sessionSecrets}
                    locked={!vaultUnlocked}
                    onInject={handleInjectSecret}
                    onUnlock={() => setUnlockOpen(true)}
                    onManage={() => {
                      location.href = '/secrets';
                    }}
                  />
                </div>
              </div>
            </Panel>
          </>
        )}
      </PanelGroup>
    </div>
  );
}
