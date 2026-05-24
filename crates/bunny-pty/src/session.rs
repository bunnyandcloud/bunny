use crate::buffer::CircularBuffer;
use crate::sanitize::strip_probe_noise;
use crate::scrollback;
use anyhow::Result;
use bunny_core::types::TerminalStatus;
use parking_lot::RwLock;
use portable_pty::{native_pty_system, Child, CommandBuilder, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;

enum PtyCommand {
    Input(String),
    Resize { cols: u16, rows: u16 },
}

pub struct PtySession {
    pub id: Uuid,
    pub name: String,
    pub buffer: Arc<CircularBuffer>,
    status: Arc<RwLock<TerminalStatus>>,
    pub output_tx: broadcast::Sender<String>,
    cmd_tx: mpsc::UnboundedSender<PtyCommand>,
    child: Arc<RwLock<Box<dyn Child + Send + Sync>>>,
    /// When set, only the attach client is killed on drop; tmux window keeps running.
    pub tmux_target: Option<String>,
    scrollback_dir: Option<PathBuf>,
}

impl PtySession {
    pub fn spawn_shell(
        id: Uuid,
        name: String,
        shell: &str,
        cwd: &str,
        init_command: Option<&str>,
        cols: u16,
        rows: u16,
        env: HashMap<String, String>,
        buffer_lines: usize,
        scrollback_dir: Option<PathBuf>,
        initial_scrollback: Option<String>,
    ) -> Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new(shell);
        cmd.cwd(cwd);
        for (k, v) in env {
            cmd.env(k, v);
        }
        if let Some(init) = init_command {
            cmd.arg("-c");
            cmd.arg(init);
        }

        let child = pair.slave.spawn_command(cmd)?;
        Self::from_pty_pair(
            id,
            name,
            pair,
            child,
            None,
            cols,
            rows,
            buffer_lines,
            scrollback_dir,
            initial_scrollback,
        )
    }

    /// Attach to an existing tmux window; survives agent restarts.
    pub fn spawn_tmux_attach(
        id: Uuid,
        name: String,
        tmux_target: &str,
        cols: u16,
        rows: u16,
        buffer_lines: usize,
        scrollback_dir: Option<PathBuf>,
        initial_scrollback: Option<String>,
    ) -> Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new("tmux");
        cmd.arg("attach");
        cmd.arg("-t");
        cmd.arg(tmux_target);
        cmd.env("TERM", "tmux-256color");
        cmd.env("COLORTERM", "truecolor");

        let child = pair.slave.spawn_command(cmd)?;
        Self::from_pty_pair(
            id,
            name,
            pair,
            child,
            Some(tmux_target.to_string()),
            cols,
            rows,
            buffer_lines,
            scrollback_dir,
            initial_scrollback,
        )
    }

    fn from_pty_pair(
        id: Uuid,
        name: String,
        pair: portable_pty::PtyPair,
        child: Box<dyn Child + Send + Sync>,
        tmux_target: Option<String>,
        _cols: u16,
        _rows: u16,
        buffer_lines: usize,
        scrollback_dir: Option<PathBuf>,
        initial_scrollback: Option<String>,
    ) -> Result<Self> {
        let mut reader = pair.master.try_clone_reader()?;
        let mut writer = pair.master.take_writer()?;
        let master = pair.master;

        let buffer = Arc::new(CircularBuffer::new(buffer_lines, 2 * 1024 * 1024));
        if let Some(init) = initial_scrollback {
            buffer.restore(&init);
        }
        let (output_tx, _) = broadcast::channel(256);
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
        let status = Arc::new(RwLock::new(TerminalStatus::Running));
        let child = Arc::new(RwLock::new(child));

        let buffer_clone = buffer.clone();
        let output_tx_clone = output_tx.clone();
        let status_reader = status.clone();
        let scrollback_dir_reader = scrollback_dir.clone();
        let tmux_target_reader = tmux_target.clone();

        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            let mut last_persist = Instant::now();
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let raw = String::from_utf8_lossy(&buf[..n]).to_string();
                        let chunk = strip_probe_noise(&raw);
                        if chunk.is_empty() {
                            continue;
                        }
                        buffer_clone.append(&chunk);
                        let _ = output_tx_clone.send(chunk);
                        if let Some(dir) = &scrollback_dir_reader {
                            if last_persist.elapsed() >= Duration::from_secs(2) {
                                let content = buffer_clone.all_content();
                                let cwd = tmux_target_reader
                                    .as_deref()
                                    .and_then(crate::tmux::pane_cwd);
                                scrollback::save_session(&dir, id, &content, cwd.as_deref());
                                last_persist = Instant::now();
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            if let Some(dir) = &scrollback_dir_reader {
                let content = buffer_clone.all_content();
                let cwd = tmux_target_reader
                    .as_deref()
                    .and_then(crate::tmux::pane_cwd);
                scrollback::save_session(&dir, id, &content, cwd.as_deref());
            }
            *status_reader.write() = TerminalStatus::Exited;
        });

        std::thread::spawn(move || {
            while let Some(command) = cmd_rx.blocking_recv() {
                match command {
                    PtyCommand::Input(data) => {
                        let _ = writer.write_all(data.as_bytes());
                        let _ = writer.flush();
                    }
                    PtyCommand::Resize { cols, rows } => {
                        let _ = master.resize(PtySize {
                            rows,
                            cols,
                            pixel_width: 0,
                            pixel_height: 0,
                        });
                    }
                }
            }
        });

        Ok(Self {
            id,
            name,
            buffer,
            status,
            output_tx,
            cmd_tx,
            child,
            tmux_target,
            scrollback_dir,
        })
    }

    pub fn status(&self) -> TerminalStatus {
        *self.status.read()
    }

    pub fn kill(&self) {
        *self.status.write() = TerminalStatus::Stopped;
        let mut child = self.child.write();
        let _ = child.kill();
    }

    pub fn write_input(&self, data: &str) -> Result<()> {
        if self.status() == TerminalStatus::Exited || self.status() == TerminalStatus::Stopped {
            anyhow::bail!("terminal not running");
        }
        self.cmd_tx.send(PtyCommand::Input(data.to_string()))?;
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.cmd_tx.send(PtyCommand::Resize { cols, rows })?;
        Ok(())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.output_tx.subscribe()
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        if let Some(dir) = &self.scrollback_dir {
            let content = self.buffer.all_content();
            let cwd = self
                .tmux_target
                .as_deref()
                .and_then(crate::tmux::pane_cwd);
            scrollback::save_session(&dir, self.id, &content, cwd.as_deref());
        }
    }
}
