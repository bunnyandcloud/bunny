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
import TerminalThemeSelect from './TerminalThemeSelect';
import VaultInjectPanel from './VaultInjectPanel';
import VaultUnlockModal from './VaultUnlockModal';
import PreviewPanel from './PreviewPanel';
import BrowserPanel from './BrowserPanel';
import ClaudeSetupPanel from './ClaudeSetupPanel';
import { useT } from '../i18n';
import AppTopBar from './AppTopBar';
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

function displayStatus(tr: (key: string) => string, status: string): string {
  switch (status) {
    case 'connecting':
      return tr('web.session.statusConnecting');
    case 'ready':
      return tr('web.session.statusReady');
    case 'no shells':
      return tr('web.session.statusNoShells');
    case 'no_active_shell':
      return tr('web.session.noActiveShell');
    default:
      return status;
  }
}

export default function SessionWorkspace({ sessionId }: Props) {
  const tr = useT();
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

  const shellIdsRef = useRef<string[]>([]);

  const refreshShells = useCallback(async () => {
    const list = await listSessionTerminals(sessionId);
    const newDiscord = list.find(
      (s) => s.name.startsWith('discord-') && !shellIdsRef.current.includes(s.id),
    );
    shellIdsRef.current = list.map((s) => s.id);
    setShells(list);
    if (newDiscord) {
      setActiveId(newDiscord.id);
      setWorkspaceTab('terminal');
    }
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

  useEffect(() => {
    const proto = location.protocol === 'https:' ? 'wss' : 'ws';
    const ws = new WebSocket(`${proto}://${location.host}/api/v1/sessions/${sessionId}/realtime`);
    ws.onmessage = (ev) => {
      try {
        const msg = JSON.parse(ev.data as string) as {
          type?: string;
          terminalId?: string;
          name?: string;
        };
        if (msg.type === 'terminal.status.changed') {
          void refreshShells().then((list) => {
            if (msg.name?.startsWith('discord-') && msg.terminalId) {
              if (list.some((s) => s.id === msg.terminalId)) {
                setActiveId(msg.terminalId);
              }
            }
          });
        }
      } catch {
        /* ignore */
      }
    };
    return () => ws.close();
  }, [sessionId, refreshShells]);

  useEffect(() => {
    const id = window.setInterval(() => {
      void refreshShells();
    }, 3000);
    return () => window.clearInterval(id);
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
      setStatus('no_active_shell');
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
          {tr('web.session.home')}
        </button>
        <div className="flex-1 flex items-center justify-center gap-2 min-w-0 text-sm">
          {sessionName ? (
            <InlineRename
              value={sessionName}
              className="text-gray-200 font-medium truncate max-w-[min(100%,20rem)]"
              title={tr('web.session.renameHint')}
              onSave={async (name) => {
                const updated = await renameSession(sessionId, name);
                setSessionName(updated.name);
              }}
            />
          ) : (
            <span className="text-bunny-muted">{tr('web.common.loading')}</span>
          )}
          <span className="text-bunny-muted truncate">
            {tr('web.session.statusSeparator')} {displayStatus(tr, status)}
          </span>
        </div>
        <AppTopBar logoutClassName="text-xs text-bunny-muted hover:text-gray-200 disabled:opacity-50">
          <button
            type="button"
            onClick={() => setDiscordOpen(true)}
            className="text-xs px-2.5 py-1 rounded border border-bunny-border text-bunny-muted hover:text-gray-200 hover:bg-bunny-bg"
            title={tr('web.session.discordTitle')}
          >
            {tr('web.session.discord')}
          </button>
          <button
            type="button"
            onClick={() => setMembersOpen(true)}
            className="text-xs px-2.5 py-1 rounded border border-bunny-border text-bunny-muted hover:text-gray-200 hover:bg-bunny-bg"
            title={tr('web.session.membersTitle')}
          >
            {tr('web.session.members')}
          </button>
          {user?.isOwner && (
            <button
              type="button"
              onClick={handleVaultButtonClick}
              className={`text-xs px-2.5 py-1 rounded border font-medium ${
                showVaultSection
                  ? 'border-bunny-accent bg-bunny-accent/10 text-bunny-accent'
                  : vaultLocked
                    ? 'border-bunny-locked/40 text-bunny-locked hover:bg-bunny-locked/10'
                    : 'border-bunny-border text-bunny-muted hover:text-bunny-accent hover:bg-bunny-bg'
              }`}
              title={
                vaultLocked
                  ? tr('web.session.vaultLockedTitle')
                  : showVaultSection
                    ? tr('web.session.vaultHide')
                    : tr('web.session.vaultShow')
              }
            >
              {tr('web.session.vault')}
              {vaultLocked ? ' 🔒' : ''}
            </button>
          )}
        </AppTopBar>
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
              <div className="flex items-center gap-1">
              {(
                [
                  ['terminal', tr('web.session.tabTerminal')],
                  ['preview', tr('web.session.tabPreview')],
                  ['browser', tr('web.session.tabBrowser')],
                ] as const
              ).map(([id, label]) => (
                <button
                  key={id}
                  type="button"
                  onClick={() => setWorkspaceTab(id)}
                  className={`text-sm px-2.5 py-1 rounded border ${
                    workspaceTab === id
                      ? 'border-bunny-accent text-bunny-accent bg-bunny-accent/10 font-bold'
                      : 'border-transparent text-bunny-fg/75 hover:text-bunny-fg font-semibold'
                  }`}
                >
                  {label}
                </button>
              ))}
              </div>
              {workspaceTab === 'terminal' ? (
                <TerminalThemeSelect className="ml-auto shrink-0" />
              ) : null}
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
            <p className="px-2 py-1 text-xs font-medium text-bunny-fg/80 border-b border-bunny-border">
              {tr('web.session.tmuxHint')}
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
                  <p>{tr('web.session.noShellOpen')}</p>
                  {vaultLocked && hasStoredSecrets && (
                    <p className="text-xs text-bunny-locked font-medium">{tr('web.session.vaultLockedHint')}</p>
                  )}
                  {vaultStatus?.status === 'unlocked' && hasStoredSecrets && (
                    <p className="text-xs text-bunny-muted">{tr('web.session.vaultUnlockHint')}</p>
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
                    {tr('web.vault.sidebarHintPrefix', {
                      count: String(vaultStatus?.ref_count ?? 0),
                      plural: (vaultStatus?.ref_count ?? 0) > 1 ? 's' : '',
                    })}{' '}
                    <button
                      type="button"
                      className="text-bunny-accent hover:underline"
                      onClick={() => setUnlockOpen(true)}
                    >
                      {tr('web.vault.unlockButton')}
                    </button>{' '}
                    {tr('web.vault.sidebarHintSuffix')}
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
