use crate::session::PtySession;
use crate::tmux;
use crate::protocol::TerminalOutput;
use anyhow::{Context, Result};
use bunny_core::types::TerminalStatus;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub struct TerminalManager {
    terminals: RwLock<HashMap<Uuid, PtySession>>,
    default_shell: String,
    buffer_lines: usize,
    use_tmux: bool,
    scrollback_dir: Option<PathBuf>,
    data_dir: PathBuf,
}

impl TerminalManager {
    pub fn new(
        shell: String,
        buffer_lines: usize,
        use_tmux: bool,
        scrollback_dir: Option<PathBuf>,
        data_dir: PathBuf,
    ) -> Self {
        if let Some(dir) = &scrollback_dir {
            let _ = std::fs::create_dir_all(dir);
        }
        Self {
            terminals: RwLock::new(HashMap::new()),
            default_shell: shell,
            buffer_lines,
            use_tmux: use_tmux && tmux::available(),
            scrollback_dir,
            data_dir,
        }
    }

    pub fn uses_tmux(&self) -> bool {
        self.use_tmux
    }

    pub fn create(
        &self,
        stream_session_id: Uuid,
        name: &str,
        cwd: &Path,
        init_command: Option<&str>,
        cols: u16,
        rows: u16,
        extra_env: HashMap<String, String>,
    ) -> Result<(Uuid, Option<String>)> {
        self.create_with_id(
            Uuid::new_v4(),
            stream_session_id,
            name,
            cwd,
            init_command,
            cols,
            rows,
            extra_env,
            None,
            None,
        )
    }

    pub fn save_scrollback(&self, id: Uuid) {
        if let Some(dir) = &self.scrollback_dir {
            if let Some(session) = self.terminals.read().get(&id) {
                let content = session.buffer.all_content();
                let cwd = session
                    .tmux_target
                    .as_deref()
                    .and_then(tmux::pane_cwd);
                crate::scrollback::save_session(&dir, id, &content, cwd.as_deref());
            }
        }
    }

    pub fn flush_all_scrollbacks(&self) {
        if self.scrollback_dir.is_none() {
            return;
        }
        let dir = self.scrollback_dir.clone().unwrap();
        let sessions: Vec<(Uuid, String, Option<String>)> = self
            .terminals
            .read()
            .iter()
            .map(|(id, s)| {
                (
                    *id,
                    s.buffer.all_content(),
                    s.tmux_target.clone(),
                )
            })
            .collect();
        for (id, mut content, target) in sessions {
            if let Some(ref t) = target {
                if tmux::target_alive(t) {
                    if let Ok(cap) = tmux::capture_pane(t) {
                        content = crate::scrollback::merge(Some(content), cap);
                    }
                }
                let cwd = tmux::pane_cwd(t);
                crate::scrollback::save_session(&dir, id, &content, cwd.as_deref());
            } else {
                crate::scrollback::save_session(&dir, id, &content, None);
            }
        }
    }

    /// Load disk scrollback into the live buffer when the agent restarted without it.
    /// Deprecated for live display — disk is recovery-only; kept for explicit restore paths.
    #[allow(dead_code)]
    pub fn hydrate_scrollback_from_disk(&self, id: Uuid) {
        let _ = id;
    }

    /// Returns `(terminal_id, tmux_target for persistence)`.
    pub fn create_with_id(
        &self,
        id: Uuid,
        _stream_session_id: Uuid,
        name: &str,
        cwd: &Path,
        init_command: Option<&str>,
        cols: u16,
        rows: u16,
        extra_env: HashMap<String, String>,
        existing_tmux_target: Option<&str>,
        initial_scrollback: Option<String>,
    ) -> Result<(Uuid, Option<String>)> {
        let recovery_scrollback = initial_scrollback.filter(|s| !s.trim().is_empty());
        let mut env = build_allowlisted_env(cwd, &self.data_dir, id);
        for (k, v) in extra_env {
            if k.starts_with("BUNNY_SECRET_")
                || k.as_str() == "BUNNY_TERMINAL_ID"
                || k.as_str() == "PATH"
            {
                env.insert(k, v);
            }
        }
        let session_env: HashMap<String, String> = env
            .iter()
            .filter(|(k, _)| {
                k.starts_with("BUNNY_SECRET_")
                    || **k == "BUNNY_TERMINAL_ID"
                    || **k == "PATH"
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let interactive_shell = tmux::interactive_shell_command(
            &self.data_dir,
            id,
            &self.default_shell,
            &session_env,
        )
        .unwrap_or_else(|_| self.default_shell.clone());

        let (pty, tmux_target) = if self.use_tmux {
            let tmux_target = if let Some(target) = existing_tmux_target {
                if tmux::target_alive(target) {
                    tmux::configure_session_for_web(tmux::session_name_from_target(target));
                    tmux::ensure_shell_running(
                        target,
                        cwd,
                        &interactive_shell,
                        &session_env,
                        false,
                    )
                        .context("respawn tmux shell")?;
                    target.to_string()
                } else {
                    tmux::ensure_terminal_session(
                        id,
                        cwd,
                        init_command,
                        &interactive_shell,
                        &session_env,
                    )
                    .context("recreate tmux session for shell")?
                }
            } else {
                tmux::ensure_terminal_session(
                    id,
                    cwd,
                    init_command,
                    &interactive_shell,
                    &session_env,
                )
                .context("create tmux session for shell")?
            };
            let pty = PtySession::spawn_tmux_attach(
                id,
                name.to_string(),
                &tmux_target,
                cols,
                rows,
                self.buffer_lines,
                self.scrollback_dir.clone(),
                recovery_scrollback,
            )?;
            (pty, Some(tmux_target))
        } else {
            let pty = PtySession::spawn_shell(
                id,
                name.to_string(),
                &self.default_shell,
                cwd.to_str().unwrap_or("/"),
                init_command,
                cols,
                rows,
                env,
                self.buffer_lines,
                self.scrollback_dir.clone(),
                recovery_scrollback,
            )?;
            (pty, None)
        };

        self.terminals.write().insert(id, pty);
        Ok((id, tmux_target))
    }

    pub fn get(&self, id: Uuid) -> Option<()> {
        self.terminals.read().get(&id).map(|_| ())
    }

    pub fn write(&self, id: Uuid, data: &str) -> Result<()> {
        let terminals = self.terminals.read();
        let session = terminals
            .get(&id)
            .ok_or_else(|| anyhow::anyhow!("terminal not found"))?;
        session.write_input(data)
    }

    pub fn resize(&self, id: Uuid, cols: u16, rows: u16) -> Result<()> {
        let tmux_target = {
            let terminals = self.terminals.read();
            let session = terminals
                .get(&id)
                .ok_or_else(|| anyhow::anyhow!("terminal not found"))?;
            session.resize(cols, rows)?;
            session.tmux_target.clone()
        };
        if let Some(target) = tmux_target {
            let _ = tmux::resize_pane(&target, cols, rows);
        }
        Ok(())
    }

    pub fn subscribe(&self, id: Uuid) -> Option<tokio::sync::broadcast::Receiver<TerminalOutput>> {
        let terminals = self.terminals.read();
        terminals.get(&id).map(|s| s.subscribe())
    }

    pub fn buffer_offset(&self, id: Uuid) -> Option<u64> {
        self.terminals
            .read()
            .get(&id)
            .map(|s| s.buffer.current_offset())
    }

    pub fn buffer_replay_range(
        &self,
        id: Uuid,
        from: u64,
        to: u64,
    ) -> Option<Vec<(u64, String)>> {
        let terminals = self.terminals.read();
        terminals
            .get(&id)
            .map(|s| s.buffer.replay_range(from, to))
    }

    /// Drop in-memory attach scrollback (e.g. before a notebook interactive session).
    pub fn clear_live_buffer(&self, id: Uuid) -> bool {
        if let Some(session) = self.terminals.write().get_mut(&id) {
            session.buffer.replace("");
            true
        } else {
            false
        }
    }

    pub fn take_recovery_replay(&self, id: Uuid) -> Option<String> {
        let terminals = self.terminals.read();
        terminals.get(&id).and_then(|s| s.take_recovery_replay())
    }

    /// Append Discord transcript to the attach buffer and notify live WebSocket clients.
    /// Disk persistence is handled separately; no disk read on reconnect.
    pub fn inject_transcript(&self, id: Uuid, text: &str) {
        if text.is_empty() {
            return;
        }
        let terminals = self.terminals.read();
        if let Some(session) = terminals.get(&id) {
            let offset = session.buffer.append(text);
            let _ = session.output_tx.send(TerminalOutput {
                offset,
                data: text.to_string(),
            });
        }
    }

    /// Recent scrollback (for parsing OAuth URLs during Claude setup).
    pub fn recent_output(&self, id: Uuid) -> Option<String> {
        self.terminals
            .read()
            .get(&id)
            .map(|s| s.buffer.all_content())
    }

    /// Visible terminal screen for Discord snapshots (tmux pane only).
    pub fn capture_snapshot_text(&self, id: Uuid) -> Option<String> {
        let terminals = self.terminals.read();
        let session = terminals.get(&id)?;
        if let Some(ref target) = session.tmux_target {
            if tmux::target_alive(target) {
                if let Ok(cap) = tmux::capture_pane_visible(target) {
                    if !cap.trim().is_empty() {
                        return Some(cap);
                    }
                }
            }
        }
        None
    }

    /// Stop attach client and kill tmux window if applicable.
    pub fn tmux_target(&self, id: Uuid) -> Option<String> {
        self.terminals
            .read()
            .get(&id)
            .and_then(|s| s.tmux_target.clone())
    }

    /// Force tmux to redraw the attach client. Safe only on an empty xterm (first connect,
    /// `replay_mode: none`). During live sessions this overwrites mid-screen lines in xterm.
    pub fn refresh_display(&self, id: Uuid) {
        let terminals = self.terminals.read();
        if let Some(session) = terminals.get(&id) {
            if let Some(target) = &session.tmux_target {
                tmux::refresh_client(target);
            }
        }
    }

    pub fn remove(&self, id: Uuid) -> bool {
        if let Some(session) = self.remove_attach_only(id) {
            if let Some(target) = &session.tmux_target {
                tmux::kill_target(target);
            } else {
                tmux::kill_terminal_session(id);
            }
            session.kill();
            true
        } else {
            false
        }
    }

    /// Remove in-memory attach client without killing tmux windows.
    pub fn remove_attach_only(&self, id: Uuid) -> Option<PtySession> {
        self.save_scrollback(id);
        self.terminals.write().remove(&id)
    }

    pub fn list_ids(&self) -> Vec<Uuid> {
        self.terminals.read().keys().copied().collect()
    }

    pub fn status(&self, id: Uuid) -> Option<TerminalStatus> {
        self.terminals.read().get(&id).map(|s| s.status())
    }

    pub fn name(&self, id: Uuid) -> Option<String> {
        self.terminals.read().get(&id).map(|s| s.name.clone())
    }

    pub fn set_name(&self, id: Uuid, name: String) -> bool {
        if let Some(session) = self.terminals.write().get_mut(&id) {
            session.name = name;
            true
        } else {
            false
        }
    }
}

fn build_allowlisted_env(cwd: &Path, data_dir: &Path, terminal_id: Uuid) -> HashMap<String, String> {
    let mut env = HashMap::new();
    for (key, value) in crate::locale::utf8_locale_vars() {
        env.insert(key.into(), value.into());
    }
    env.insert("TERM".into(), "xterm-256color".into());
    env.insert("COLORTERM".into(), "truecolor".into());
    env.insert("PWD".into(), cwd.display().to_string());
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    env.insert("HOME".into(), home.clone());
    let bunny_bin = data_dir.join("bin");
    env.insert(
        "PATH".into(),
        format!(
            "{}:{home}/.local/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
            bunny_bin.display()
        ),
    );
    env.insert("BUNNY_TERMINAL_ID".into(), terminal_id.to_string());
    env
}
