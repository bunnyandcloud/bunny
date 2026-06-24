use crate::state::AppState;
use anyhow::Result;
use bunny_blocks::{
    AuthorSource, BlockKind, BlockPatch, BlockStatus, TerminalBlock,
};
use chrono::Utc;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BlockServerMsg {
    BlocksSnapshot {
        blocks: Vec<TerminalBlock>,
        latest_seq: i64,
    },
    BlockAppend {
        block: TerminalBlock,
    },
    BlockPatch {
        id: Uuid,
        #[serde(flatten)]
        patch: BlockPatch,
    },
}

pub struct BlockHub {
    channels: Mutex<HashMap<Uuid, broadcast::Sender<BlockServerMsg>>>,
}

impl BlockHub {
    pub fn new() -> Self {
        Self {
            channels: Mutex::new(HashMap::new()),
        }
    }

    fn sender(&self, terminal_id: Uuid) -> broadcast::Sender<BlockServerMsg> {
        let mut map = self.channels.lock();
        map.entry(terminal_id)
            .or_insert_with(|| broadcast::channel(256).0)
            .clone()
    }

    pub fn subscribe(&self, terminal_id: Uuid) -> broadcast::Receiver<BlockServerMsg> {
        self.sender(terminal_id).subscribe()
    }

    pub fn publish(&self, terminal_id: Uuid, msg: BlockServerMsg) {
        let _ = self.sender(terminal_id).send(msg);
    }
}

#[derive(Clone)]
struct ActiveCollector {
    output_block_id: Uuid,
    command: String,
    exec_line: String,
    baseline: String,
    last_output: String,
    interactive: bool,
    promoted: bool,
    idle_polls: u32,
    saw_busy: bool,
    tty_snapshot_pushed: bool,
    pager_dismissed: bool,
    last_exit_code: Option<i32>,
}

pub struct OutputCollectors {
    inner: Mutex<HashMap<Uuid, ActiveCollector>>,
}

impl OutputCollectors {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

pub struct AppendBlockParams {
    pub terminal_id: Uuid,
    pub kind: BlockKind,
    pub author_user_id: Option<Uuid>,
    pub author_display: String,
    pub author_source: AuthorSource,
    pub command: Option<String>,
    pub content: String,
    pub status: BlockStatus,
    pub exit_code: Option<i32>,
    pub parent_block_id: Option<Uuid>,
    pub meta: serde_json::Value,
}

pub fn row_to_block(row: bunny_auth::db::TerminalBlockRow) -> TerminalBlock {
    TerminalBlock {
        id: row.id,
        terminal_id: row.terminal_id,
        seq: row.seq,
        kind: parse_block_kind(&row.kind),
        author_user_id: row.author_user_id,
        author_display: row.author_display,
        author_git_name: None,
        author_git_email: None,
        author_source: parse_author_source(&row.author_source),
        created_at: row.created_at.parse().unwrap_or_else(|_| Utc::now()),
        finished_at: row.finished_at.and_then(|s| s.parse().ok()),
        command: row.command,
        content: row.content,
        exit_code: row.exit_code,
        status: parse_block_status(&row.status),
        parent_block_id: row.parent_block_id,
        meta: serde_json::from_str(&row.meta_json).unwrap_or(serde_json::Value::Null),
    }
}

fn parse_block_kind(s: &str) -> BlockKind {
    match s {
        "user_command" => BlockKind::UserCommand,
        "discord_command" => BlockKind::DiscordCommand,
        "output" => BlockKind::Output,
        "process_run" => BlockKind::ProcessRun,
        _ => BlockKind::SystemEvent,
    }
}

pub fn block_kind_str(kind: BlockKind) -> &'static str {
    match kind {
        BlockKind::UserCommand => "user_command",
        BlockKind::DiscordCommand => "discord_command",
        BlockKind::Output => "output",
        BlockKind::ProcessRun => "process_run",
        BlockKind::SystemEvent => "system_event",
    }
}

fn parse_block_status(s: &str) -> BlockStatus {
    match s {
        "pending" => BlockStatus::Pending,
        "running" => BlockStatus::Running,
        "failed" => BlockStatus::Failed,
        "cancelled" => BlockStatus::Cancelled,
        _ => BlockStatus::Completed,
    }
}

pub fn block_status_str(status: BlockStatus) -> &'static str {
    match status {
        BlockStatus::Pending => "pending",
        BlockStatus::Running => "running",
        BlockStatus::Completed => "completed",
        BlockStatus::Failed => "failed",
        BlockStatus::Cancelled => "cancelled",
    }
}

fn parse_author_source(s: &str) -> AuthorSource {
    match s {
        "discord" => AuthorSource::Discord,
        "system" => AuthorSource::System,
        _ => AuthorSource::Web,
    }
}

fn author_source_str(source: AuthorSource) -> &'static str {
    match source {
        AuthorSource::Web => "web",
        AuthorSource::Discord => "discord",
        AuthorSource::System => "system",
    }
}

pub fn author_display_for_user(state: &AppState, user_id: Option<Uuid>) -> String {
    let Some(user_id) = user_id else {
        return "system".into();
    };
    state
        .auth
        .db()
        .lock()
        .get_user_profile(user_id)
        .ok()
        .flatten()
        .and_then(|p| p.git_name.filter(|n| !n.trim().is_empty()))
        .or_else(|| {
            state
                .auth
                .db()
                .lock()
                .get_user_profile(user_id)
                .ok()
                .flatten()
                .map(|p| p.email.split('@').next().unwrap_or("user").into())
        })
        .unwrap_or_else(|| "user".into())
}

pub fn enrich_block_author(state: &AppState, block: &mut TerminalBlock) {
    let Some(user_id) = block.author_user_id else {
        return;
    };
    let Ok(Some(profile)) = state.auth.db().lock().get_user_profile(user_id) else {
        return;
    };
    if block.author_git_name.is_none() {
        block.author_git_name = profile
            .git_name
            .filter(|n| !n.trim().is_empty());
    }
    if block.author_git_email.is_none() {
        block.author_git_email = profile
            .git_email
            .filter(|e| !e.trim().is_empty());
    }
}

pub fn append_block(state: &AppState, params: AppendBlockParams) -> Result<TerminalBlock> {
    let id = Uuid::new_v4();
    let now = Utc::now();
    let finished_at = if matches!(
        params.status,
        BlockStatus::Completed | BlockStatus::Failed | BlockStatus::Cancelled
    ) {
        Some(now)
    } else {
        None
    };
    let meta_json = serde_json::to_string(&params.meta)?;
    let db = state.auth.db();
    let mut guard = db.lock();
    let seq = guard.next_terminal_block_seq(params.terminal_id)?;
    guard.insert_terminal_block(
        id,
        params.terminal_id,
        seq,
        block_kind_str(params.kind),
        params.author_user_id,
        &params.author_display,
        author_source_str(params.author_source),
        &now.to_rfc3339(),
        finished_at.as_ref().map(|t| t.to_rfc3339()).as_deref(),
        params.command.as_deref(),
        &params.content,
        params.exit_code,
        block_status_str(params.status),
        params.parent_block_id,
        &meta_json,
    )?;
    drop(guard);

    let mut block = TerminalBlock {
        id,
        terminal_id: params.terminal_id,
        seq,
        kind: params.kind,
        author_user_id: params.author_user_id,
        author_display: params.author_display,
        author_git_name: None,
        author_git_email: None,
        author_source: params.author_source,
        created_at: now,
        finished_at,
        command: params.command,
        content: params.content,
        exit_code: params.exit_code,
        status: params.status,
        parent_block_id: params.parent_block_id,
        meta: params.meta,
    };
    enrich_block_author(state, &mut block);

    state
        .block_hub
        .publish(params.terminal_id, BlockServerMsg::BlockAppend { block: block.clone() });

    publish_session_block_event(state, &block);
    Ok(block)
}

fn publish_session_block_event(state: &AppState, block: &TerminalBlock) {
    let session_id = state.terminal_sessions.read().get(&block.terminal_id).copied();
    let Some(session_id) = session_id else {
        return;
    };
    let payload = serde_json::json!({
        "terminalId": block.terminal_id.to_string(),
        "blockId": block.id.to_string(),
        "seq": block.seq,
        "kind": block_kind_str(block.kind),
        "status": block_status_str(block.status),
    });
    let event = serde_json::json!({
        "type": "terminal.block.changed",
        "sessionId": session_id.to_string(),
        "payload": payload,
    });
    state.realtime.publish(session_id, &event);
}

pub fn patch_block(
    state: &AppState,
    terminal_id: Uuid,
    block_id: Uuid,
    patch: BlockPatch,
) -> Result<()> {
    let now = Utc::now();
    let db = state.auth.db();
    let mut guard = db.lock();
    if let Some(replace) = &patch.content_replace {
        guard.set_terminal_block_content(block_id, replace)?;
    } else if let Some(delta) = &patch.content_delta {
        guard.append_terminal_block_content(block_id, delta)?;
    }
    if let Some(meta) = &patch.meta {
        let meta_json = serde_json::to_string(meta)?;
        guard.update_terminal_block_meta(block_id, &meta_json)?;
    }
    if patch.status.is_some() || patch.exit_code.is_some() || patch.finished_at.is_some() {
        let status = patch
            .status
            .map(block_status_str)
            .unwrap_or("running");
        let finished = patch
            .finished_at
            .as_ref()
            .map(|t| t.to_rfc3339())
            .or_else(|| {
                if patch.status.is_some() {
                    Some(now.to_rfc3339())
                } else {
                    None
                }
            });
        guard.update_terminal_block_status(
            block_id,
            status,
            patch.exit_code,
            finished.as_deref(),
        )?;
    }
    drop(guard);

    state.block_hub.publish(
        terminal_id,
        BlockServerMsg::BlockPatch {
            id: block_id,
            patch,
        },
    );
    Ok(())
}

pub fn list_blocks(
    state: &AppState,
    terminal_id: Uuid,
    from_seq: i64,
    limit: usize,
) -> Result<(Vec<TerminalBlock>, i64)> {
    let db = state.auth.db();
    let rows = db
        .lock()
        .list_terminal_blocks(terminal_id, from_seq, limit)?;
    let latest_seq = rows.last().map(|r| r.seq).unwrap_or(from_seq.saturating_sub(1).max(0));
    let blocks: Vec<TerminalBlock> = rows
        .into_iter()
        .map(|r| {
            let mut b = row_to_block(r);
            enrich_block_author(state, &mut b);
            b
        })
        .collect();
    Ok((blocks, latest_seq))
}

pub fn blocks_snapshot(
    state: &AppState,
    terminal_id: Uuid,
    from_seq: i64,
) -> BlockServerMsg {
    let (blocks, latest_seq) = list_blocks(state, terminal_id, from_seq, 10_000)
        .unwrap_or_default();
    BlockServerMsg::BlocksSnapshot {
        blocks,
        latest_seq,
    }
}

pub fn stop_running_processes(state: &AppState, terminal_id: Uuid) -> Result<()> {
    state.output_collectors.inner.lock().remove(&terminal_id);

    let rows = state
        .auth
        .db()
        .lock()
        .list_terminal_blocks(terminal_id, 0, 500)?;
    let now = Utc::now();
    for row in rows {
        if row.status != "running" {
            continue;
        }
        if row.kind != "process_run" && row.kind != "output" {
            continue;
        }
        let _ = patch_block(
            state,
            terminal_id,
            row.id,
            BlockPatch {
                status: Some(BlockStatus::Cancelled),
                content_delta: None,
                content_replace: None,
                meta: None,
                exit_code: Some(130),
                finished_at: Some(now),
            },
        );
    }

    if let Some(target) = state.terminals.tmux_target(terminal_id) {
        bunny_pty::tmux::configure_pane_for_web(&target);
        state.terminals.refresh_display(terminal_id);
    }
    Ok(())
}

pub fn terminal_has_interactive_session(state: &AppState, terminal_id: Uuid) -> bool {
    let Ok(rows) = state
        .auth
        .db()
        .lock()
        .list_terminal_blocks(terminal_id, 0, 500)
    else {
        return false;
    };
    rows.iter().any(|row| {
        row.status == "running"
            && row.kind == "process_run"
            && serde_json::from_str::<serde_json::Value>(&row.meta_json)
                .ok()
                .and_then(|m| m.get("interactive").and_then(|v| v.as_bool()))
                == Some(true)
    })
}

pub fn terminal_input_locked(state: &AppState, terminal_id: Uuid) -> bool {
    if state.output_collectors.inner.lock().contains_key(&terminal_id) {
        return true;
    }
    state
        .auth
        .db()
        .lock()
        .terminal_has_running_process(terminal_id)
        .unwrap_or(false)
}

pub fn record_discord_transcript_blocks(
    state: &AppState,
    term_id: Uuid,
    command: &str,
    output: Option<&str>,
    exit_code: Option<i32>,
    acting_user_id: Option<Uuid>,
    persistent: bool,
) -> Result<()> {
    let author_display = author_display_for_user(state, acting_user_id);
    let cmd_block = append_block(
        state,
        AppendBlockParams {
            terminal_id: term_id,
            kind: BlockKind::DiscordCommand,
            author_user_id: acting_user_id,
            author_display: author_display.clone(),
            author_source: AuthorSource::Discord,
            command: Some(command.to_string()),
            content: String::new(),
            status: BlockStatus::Completed,
            exit_code: None,
            parent_block_id: None,
            meta: serde_json::json!({}),
        },
    )?;

    if let Some(output) = output {
        let (kind, status, exit_code) = if persistent {
            (BlockKind::ProcessRun, BlockStatus::Running, None)
        } else {
            (
                BlockKind::Output,
                BlockStatus::Completed,
                exit_code.or(Some(0)),
            )
        };
        append_block(
            state,
            AppendBlockParams {
                terminal_id: term_id,
                kind,
                author_user_id: acting_user_id,
                author_display,
                author_source: AuthorSource::Discord,
                command: None,
                content: output.to_string(),
                status,
                exit_code,
                parent_block_id: Some(cmd_block.id),
                meta: serde_json::json!({}),
            },
        )?;
    }
    Ok(())
}

pub fn append_system_event(
    state: &AppState,
    term_id: Uuid,
    message: &str,
    meta: serde_json::Value,
) -> Result<TerminalBlock> {
    append_block(
        state,
        AppendBlockParams {
            terminal_id: term_id,
            kind: BlockKind::SystemEvent,
            author_user_id: None,
            author_display: "system".into(),
            author_source: AuthorSource::System,
            command: None,
            content: message.to_string(),
            status: BlockStatus::Completed,
            exit_code: None,
            parent_block_id: None,
            meta,
        },
    )
}

pub fn submit_user_command(
    state: Arc<AppState>,
    terminal_id: Uuid,
    user_id: Uuid,
    command: &str,
    baseline: String,
) {
    let command = command.trim();
    if command.is_empty() {
        return;
    }
    let author_display = author_display_for_user(&state, Some(user_id));
    let notebook_shells = state.config.terminal.notebook_shells;
    let interactive = if notebook_shells {
        crate::terminals::notebook_user_command_expects_interactive(command)
    } else {
        crate::terminals::user_command_expects_interactive(command)
    };
    let exec_line = crate::terminals::notebook_shell_exec_line(
        command,
        interactive,
        state.config.terminal.notebook_shells,
    );
    let tui_hint = command.split_whitespace().next().unwrap_or(command);
    let output_meta = if interactive {
        serde_json::json!({
            "interactive": true,
            "tui_command": tui_hint,
        })
    } else {
        serde_json::json!({})
    };
    let cmd_block = match append_block(
        &state,
        AppendBlockParams {
            terminal_id,
            kind: BlockKind::UserCommand,
            author_user_id: Some(user_id),
            author_display: author_display.clone(),
            author_source: AuthorSource::Web,
            command: Some(command.to_string()),
            content: String::new(),
            status: BlockStatus::Completed,
            exit_code: None,
            parent_block_id: None,
            meta: serde_json::json!({}),
        },
    ) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(%terminal_id, error = %e, "failed to append user command block");
            return;
        }
    };

    let output_block = match append_block(
        &state,
        AppendBlockParams {
            terminal_id,
            kind: BlockKind::ProcessRun,
            author_user_id: Some(user_id),
            author_display,
            author_source: AuthorSource::Web,
            command: None,
            content: String::new(),
            status: BlockStatus::Running,
            exit_code: None,
            parent_block_id: Some(cmd_block.id),
            meta: output_meta,
        },
    ) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(%terminal_id, error = %e, "failed to append output block");
            return;
        }
    };

    if interactive {
        let _ = promote_to_interactive_process(
            &state,
            terminal_id,
            output_block.id,
            tui_hint,
        );
    }

    state.output_collectors.inner.lock().insert(
        terminal_id,
        ActiveCollector {
            output_block_id: output_block.id,
            command: command.to_string(),
            exec_line,
            baseline,
            last_output: String::new(),
            interactive,
            promoted: interactive,
            idle_polls: 0,
            saw_busy: false,
            tty_snapshot_pushed: false,
            pager_dismissed: false,
            last_exit_code: None,
        },
    );

    tokio::spawn(output_collector_loop(state, terminal_id));
}

/// Record a command the user already ran in the attach TTY (no re-execution).
pub fn record_tty_command(
    state: Arc<AppState>,
    terminal_id: Uuid,
    user_id: Uuid,
    command: &str,
    baseline: String,
) {
    let command = command.trim();
    if command.is_empty() {
        return;
    }
    let author_display = author_display_for_user(&state, Some(user_id));
    let notebook_shells = state.config.terminal.notebook_shells;
    let interactive = if notebook_shells {
        crate::terminals::notebook_user_command_expects_interactive(command)
    } else {
        crate::terminals::user_command_expects_interactive(command)
    };
    let exec_line = command.to_string();
    let tui_hint = command.split_whitespace().next().unwrap_or(command);
    let output_meta = if interactive {
        serde_json::json!({
            "attach_tty": true,
            "interactive": true,
            "tui_command": tui_hint,
        })
    } else {
        serde_json::json!({ "attach_tty": true })
    };
    let cmd_block = match append_block(
        &state,
        AppendBlockParams {
            terminal_id,
            kind: BlockKind::UserCommand,
            author_user_id: Some(user_id),
            author_display: author_display.clone(),
            author_source: AuthorSource::Web,
            command: Some(command.to_string()),
            content: String::new(),
            status: BlockStatus::Completed,
            exit_code: None,
            parent_block_id: None,
            meta: serde_json::json!({ "attach_tty": true }),
        },
    ) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(%terminal_id, error = %e, "failed to append attach tty command block");
            return;
        }
    };

    let output_block = match append_block(
        &state,
        AppendBlockParams {
            terminal_id,
            kind: BlockKind::ProcessRun,
            author_user_id: Some(user_id),
            author_display,
            author_source: AuthorSource::Web,
            command: None,
            content: String::new(),
            status: BlockStatus::Running,
            exit_code: None,
            parent_block_id: Some(cmd_block.id),
            meta: output_meta,
        },
    ) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(%terminal_id, error = %e, "failed to append attach tty output block");
            return;
        }
    };

    if interactive {
        let _ = promote_to_interactive_process(
            &state,
            terminal_id,
            output_block.id,
            tui_hint,
        );
    }

    state.output_collectors.inner.lock().insert(
        terminal_id,
        ActiveCollector {
            output_block_id: output_block.id,
            command: command.to_string(),
            exec_line,
            baseline,
            last_output: String::new(),
            interactive,
            promoted: interactive,
            idle_polls: 0,
            saw_busy: false,
            tty_snapshot_pushed: false,
            pager_dismissed: false,
            last_exit_code: None,
        },
    );

    tokio::spawn(output_collector_loop(state, terminal_id));
}

pub fn on_user_command_submitted(
    state: Arc<AppState>,
    terminal_id: Uuid,
    user_id: Uuid,
    command: &str,
) {
    let baseline = crate::terminals::capture_pane_for_terminal(&state, terminal_id).unwrap_or_default();
    submit_user_command(state, terminal_id, user_id, command, baseline);
}

async fn output_collector_loop(state: Arc<AppState>, terminal_id: Uuid) {
    let started = std::time::Instant::now();
    let max_duration = std::time::Duration::from_secs(3600);
    let mut poll_ms = 100u64;

    loop {
        if started.elapsed() > max_duration {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(poll_ms)).await;

        let collector = state.output_collectors.inner.lock().get(&terminal_id).cloned();
        let Some(mut collector) = collector else {
            return;
        };
        let instant = crate::terminals::notebook_instant_command(&collector.command);
        poll_ms = if instant { 20 } else { 100 };

        let busy = tokio::task::spawn_blocking({
            let state = Arc::clone(&state);
            move || crate::terminals::discord_shell_pane_busy(&state, terminal_id)
        })
        .await
        .ok()
        .and_then(|r| r.ok())
        .unwrap_or(false);

        let pane_cmd = if busy {
            tokio::task::spawn_blocking({
                let state = Arc::clone(&state);
                move || crate::terminals::terminal_pane_current_command(&state, terminal_id)
            })
            .await
            .ok()
            .flatten()
        } else {
            None
        };

        if busy && !collector.interactive && !crate::terminals::notebook_instant_command(&collector.command) {
            if let Some(ref cmd) = pane_cmd {
                if crate::terminals::pane_cmd_is_incidental_pager(cmd, &collector.command)
                    && !collector.pager_dismissed
                {
                    let state_bg = Arc::clone(&state);
                    let _ = tokio::task::spawn_blocking(move || {
                        let _ = crate::terminals::dismiss_pager(&state_bg, terminal_id);
                    })
                    .await;
                    collector.pager_dismissed = true;
                }
                let tui = crate::terminals::is_interactive_tui_command(cmd)
                    || crate::terminals::pane_process_suggests_interactive(cmd, &collector.command);
                if tui
                    && !crate::terminals::pane_cmd_is_incidental_pager(cmd, &collector.command)
                {
                    collector.interactive = true;
                    let _ = promote_to_interactive_process(
                        &state,
                        terminal_id,
                        collector.output_block_id,
                        cmd,
                    );
                    collector.promoted = true;
                }
            }
        }

        if busy {
            collector.saw_busy = true;
        }

        if !collector.interactive && !instant {
            let baseline = collector.baseline.clone();
            let command = collector.command.clone();
            let exec_line = collector.exec_line.clone();
            let (sanitized, exit_code) = tokio::task::spawn_blocking({
                let state = Arc::clone(&state);
                move || {
                    capture_non_interactive_output(
                        &state,
                        terminal_id,
                        &baseline,
                        &command,
                        &exec_line,
                    )
                }
            })
            .await
            .ok()
            .unwrap_or_default();
            if let Some(code) = exit_code {
                collector.last_exit_code = Some(code);
            }
            if !sanitized.is_empty() {
                collector.saw_busy = true;
            }
            if sanitized != collector.last_output {
                let _ = patch_block(
                    &state,
                    terminal_id,
                    collector.output_block_id,
                    BlockPatch {
                        status: None,
                        content_delta: None,
                        content_replace: Some(sanitized.clone()),
                        meta: None,
                        exit_code: None,
                        finished_at: None,
                    },
                );
                collector.last_output = sanitized;
            }
        }

        if busy
            && !collector.promoted
            && !collector.interactive
            && crate::terminals::runtime_interactive_promotion_allowed(
                &collector.command,
                pane_cmd.as_deref(),
            )
            && output_suggests_interactive_prompt(&collector.last_output)
        {
            collector.promoted = true;
            collector.interactive = true;
            let hint = pane_cmd
                .as_deref()
                .unwrap_or(collector.command.split_whitespace().next().unwrap_or(&collector.command));
            let _ = promote_to_interactive_process(
                &state,
                terminal_id,
                collector.output_block_id,
                hint,
            );
        }

        state
            .output_collectors
            .inner
            .lock()
            .insert(terminal_id, collector.clone());

        if collector.interactive {
            if busy && !collector.tty_snapshot_pushed {
                let snap = tokio::task::spawn_blocking({
                    let state = Arc::clone(&state);
                    move || crate::terminals::capture_interactive_tty_snapshot(&state, terminal_id)
                })
                .await
                .unwrap_or_default();
                if output_suggests_interactive_prompt(&snap)
                    || snap.to_lowercase().contains("proceed (y/n)")
                {
                    let hint = pane_cmd
                        .as_deref()
                        .or_else(|| collector.command.split_whitespace().next());
                    let _ = patch_interactive_tty_snapshot(
                        &state,
                        terminal_id,
                        collector.output_block_id,
                        &snap,
                        hint,
                    );
                    collector.tty_snapshot_pushed = true;
                }
            }
            if busy {
                collector.idle_polls = 0;
            } else if started.elapsed() > std::time::Duration::from_millis(900) {
                let collector_snapshot = collector.clone();
                let state_bg = Arc::clone(&state);
                let output = tokio::task::spawn_blocking(move || {
                    interactive_session_output(&state_bg, terminal_id, &collector_snapshot)
                })
                .await
                .ok()
                .unwrap_or_default();

                if interactive_failure_output(&output, &collector.command) {
                    finish_interactive_collector(&state, terminal_id, &collector, &output, true);
                    return;
                }

                collector.idle_polls += 1;
                if collector.idle_polls >= 4 {
                    finish_interactive_collector(&state, terminal_id, &collector, &output, false);
                    return;
                }
            }
            state
                .output_collectors
                .inner
                .lock()
                .insert(terminal_id, collector);
            continue;
        }

        let done_waiting = if instant {
            !busy
        } else {
            started.elapsed() > std::time::Duration::from_millis(300)
                || (collector.saw_busy && started.elapsed() > std::time::Duration::from_millis(120))
        };

        if !collector.interactive && !busy && done_waiting {
            if !instant
                && exec_line_expects_exit_marker(&collector.exec_line)
                && collector.last_exit_code.is_none()
                && started.elapsed() < std::time::Duration::from_millis(2000)
            {
                state
                    .output_collectors
                    .inner
                    .lock()
                    .insert(terminal_id, collector);
                continue;
            }

            let (mut final_output, exit_code) = tokio::task::spawn_blocking({
                let state = Arc::clone(&state);
                let baseline = collector.baseline.clone();
                let command = collector.command.clone();
                let exec_line = collector.exec_line.clone();
                move || {
                    let (out, code) = capture_non_interactive_output(
                        &state,
                        terminal_id,
                        &baseline,
                        &command,
                        &exec_line,
                    );
                    let cap =
                        crate::terminals::capture_pane_for_terminal(&state, terminal_id)
                            .unwrap_or_default();
                    reconcile_git_commit_capture(&command, &exec_line, &out, &cap, code)
                }
            })
            .await
            .ok()
            .unwrap_or_default();

            let exit_code = exit_code
                .or(collector.last_exit_code)
                .unwrap_or(0);
            let failed = exit_code != 0;
            let status = if failed {
                BlockStatus::Failed
            } else {
                BlockStatus::Completed
            };

            if final_output != collector.last_output {
                let _ = patch_block(
                    &state,
                    terminal_id,
                    collector.output_block_id,
                    BlockPatch {
                        status: None,
                        content_delta: None,
                        content_replace: Some(final_output.clone()),
                        meta: None,
                        exit_code: None,
                        finished_at: None,
                    },
                );
            }
            let _ = patch_block(
                &state,
                terminal_id,
                collector.output_block_id,
                BlockPatch {
                    status: Some(status),
                    content_delta: None,
                    content_replace: None,
                    meta: None,
                    exit_code: Some(exit_code),
                    finished_at: Some(Utc::now()),
                },
            );
            state.output_collectors.inner.lock().remove(&terminal_id);
            return;
        }
    }

    if let Some(collector) = state.output_collectors.inner.lock().remove(&terminal_id) {
        let _ = patch_block(
            &state,
            terminal_id,
            collector.output_block_id,
            BlockPatch {
                status: Some(BlockStatus::Completed),
                content_delta: None,
                content_replace: None,
                meta: None,
                exit_code: Some(0),
                finished_at: Some(Utc::now()),
            },
        );
    }
}

fn output_suggests_interactive_prompt(output: &str) -> bool {
    if output.is_empty() {
        return false;
    }
    for line in output.lines().rev().take(10) {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let lower = t.to_lowercase();
        if t.contains('?')
            && (t.contains('›')
                || t.contains('…')
                || lower.contains("yes / no")
                || lower.contains("(y/n)")
                || lower.contains("[y/n]")
                || (t.contains('[') && t.contains(',')))
        {
            return true;
        }
        if t.contains('?')
            && t.contains('[')
            && t.contains('/')
            && t.contains(']')
        {
            return true;
        }
        if lower.contains("stage this hunk") || lower.contains("stage deletion") {
            return true;
        }
        if lower.contains("press ") && lower.contains("enter") {
            return true;
        }
        if t.contains('❯') || (t.contains('✔') && t.contains('?')) {
            return true;
        }
        if lower.contains("select ") || lower.contains("choose ") {
            return true;
        }
    }
    false
}

fn interactive_session_output(
    state: &AppState,
    terminal_id: Uuid,
    collector: &ActiveCollector,
) -> String {
    let cap = crate::terminals::capture_pane_visible_for_terminal(state, terminal_id)
        .or_else(|_| crate::terminals::capture_pane_for_terminal(state, terminal_id))
        .unwrap_or_default();
    let raw = crate::terminals::pane_text_delta(
        &collector.baseline,
        &cap,
        Some(&collector.command),
    );
    sanitize_collected_output(
        &strip_tty_noise_lines(&raw),
        &collector.command,
        &collector.exec_line,
    )
}

/// Remove lines that are mostly xterm arrow-key echo (^[[A / ^[[B).
fn strip_tty_noise_lines(raw: &str) -> String {
    raw.lines()
        .filter(|line| {
            let t = line.trim();
            if t.is_empty() {
                return false;
            }
            let stripped: String = t.chars().filter(|c| *c != '^' && *c != '[').collect();
            if stripped.is_empty() || stripped.chars().all(|c| c == 'A' || c == 'B' || c == 'C' || c == 'D') {
                return false;
            }
            !t.starts_with("^[[")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn interactive_failure_output(output: &str, command: &str) -> bool {
    if output.is_empty() {
        return false;
    }
    let first = command.split_whitespace().next().unwrap_or(command);
    for line in output.lines() {
        let line = line.trim();
        if line.contains("command not found") {
            return true;
        }
        if line.contains("No such file or directory") && line.contains(first) {
            return true;
        }
        if line.contains(first) && line.ends_with(": not found") {
            return true;
        }
    }
    false
}

fn finish_interactive_collector(
    state: &AppState,
    terminal_id: Uuid,
    collector: &ActiveCollector,
    output: &str,
    failed: bool,
) {
    if let Some(target) = state.terminals.tmux_target(terminal_id) {
        bunny_pty::tmux::configure_pane_for_web(&target);
    }
    let status = if failed {
        BlockStatus::Failed
    } else {
        BlockStatus::Completed
    };
    let exit_code = if failed { Some(127) } else { Some(0) };
    let _ = patch_block(
        state,
        terminal_id,
        collector.output_block_id,
        BlockPatch {
            status: Some(status),
            content_delta: None,
            content_replace: Some(output.to_string()),
            meta: None,
            exit_code,
            finished_at: Some(Utc::now()),
        },
    );
    state.output_collectors.inner.lock().remove(&terminal_id);
}

fn patch_interactive_tty_snapshot(
    state: &AppState,
    terminal_id: Uuid,
    block_id: Uuid,
    tty_snapshot: &str,
    tui_command: Option<&str>,
) -> Result<()> {
    if tty_snapshot.trim().is_empty() {
        return Ok(());
    }
    let mut meta = serde_json::json!({
        "interactive": true,
        "tty_snapshot": tty_snapshot,
    });
    if let Some(cmd) = tui_command {
        meta["tui_command"] = serde_json::json!(cmd);
    }
    let meta_json = serde_json::to_string(&meta)?;
    {
        let auth_db = state.auth.db();
        let mut guard = auth_db.lock();
        guard.update_terminal_block_meta(block_id, &meta_json)?;
    }
    patch_block(
        state,
        terminal_id,
        block_id,
        BlockPatch {
            status: None,
            content_delta: None,
            content_replace: None,
            meta: Some(meta),
            exit_code: None,
            finished_at: None,
        },
    )
}

fn promote_to_interactive_process(
    state: &AppState,
    terminal_id: Uuid,
    block_id: Uuid,
    tui_command: &str,
) -> Result<()> {
    let tty_snapshot = crate::terminals::capture_interactive_tty_snapshot(state, terminal_id);
    let meta = serde_json::json!({
        "interactive": true,
        "tui_command": tui_command,
        "tty_snapshot": tty_snapshot,
    });
    let meta_json = serde_json::to_string(&meta)?;
    {
        let auth_db = state.auth.db();
        let mut guard = auth_db.lock();
        guard.update_terminal_block_kind(block_id, "process_run")?;
        guard.set_terminal_block_content(block_id, "")?;
        guard.update_terminal_block_meta(block_id, &meta_json)?;
    }
    if let Some(target) = state.terminals.tmux_target(terminal_id) {
        bunny_pty::tmux::configure_pane_for_interactive(&target);
    }
    patch_block(
        state,
        terminal_id,
        block_id,
        BlockPatch {
            status: Some(BlockStatus::Running),
            content_delta: None,
            content_replace: Some(String::new()),
            meta: Some(meta),
            exit_code: None,
            finished_at: None,
        },
    )
}

/// Drop echoed command lines and shell prompts from captured pane output.
fn capture_non_interactive_output(
    state: &AppState,
    terminal_id: Uuid,
    baseline: &str,
    command: &str,
    exec_line: &str,
) -> (String, Option<i32>) {
    if crate::terminals::notebook_instant_command(command) {
        return capture_instant_command_output(state, terminal_id, baseline, command, exec_line);
    }

    fn capture_from_cap(
        cap: &str,
        baseline: &str,
        command: &str,
        exec_line: &str,
    ) -> (String, Option<i32>) {
        let mut exit_code = None;
        for cmd in [exec_line, command] {
            let delta = crate::terminals::pane_text_delta(baseline, cap, Some(cmd));
            let (parsed, code) = crate::terminals::notebook_parse_captured_output(&delta);
            if code.is_some() {
                exit_code = code;
            }
            let from_delta = sanitize_collected_output(&parsed, command, exec_line);
            if !from_delta.is_empty() {
                if command.to_lowercase().contains("git commit")
                    && git_commit_nothing_staged_in_cap(cap, command, exec_line)
                {
                    let full = git_commit_output_from_pane(cap, command, exec_line);
                    if !full.is_empty() {
                        return (full, exit_code.or(Some(1)));
                    }
                }
                return (from_delta, exit_code);
            }
            let extracted = crate::terminals::extract_command_output_from_pane(cap, cmd);
            let (parsed, code) = crate::terminals::notebook_parse_captured_output(&extracted);
            if code.is_some() {
                exit_code = code;
            }
            let from_extract = sanitize_collected_output(&parsed, command, exec_line);
            if !from_extract.is_empty() {
                if command.to_lowercase().contains("git commit")
                    && git_commit_nothing_staged_in_cap(cap, command, exec_line)
                {
                    let full = git_commit_output_from_pane(cap, command, exec_line);
                    if !full.is_empty() {
                        return (full, exit_code.or(Some(1)));
                    }
                }
                return (from_extract, exit_code);
            }
        }
        (String::new(), exit_code)
    }

    let cap = crate::terminals::capture_pane_for_terminal(state, terminal_id).unwrap_or_default();
    let (mut out, exit_code) = capture_from_cap(&cap, baseline, command, exec_line);
    if !out.is_empty() {
        return (out, exit_code);
    }
    if out.is_empty() {
        out = infer_failure_message_from_pane(command, &cap, exec_line);
        if !out.is_empty() {
            return (out, exit_code.or(Some(1)));
        }
    }
    let cap_vis =
        crate::terminals::capture_pane_visible_for_terminal(state, terminal_id).unwrap_or_default();
    let (mut out, exit_code) = capture_from_cap(&cap_vis, baseline, command, exec_line);
    if !out.is_empty() {
        return (out, exit_code);
    }
    if out.is_empty() {
        out = infer_failure_message_from_pane(command, &cap_vis, exec_line);
        if !out.is_empty() {
            return (out, exit_code.or(Some(1)));
        }
    }
    let silent = infer_silent_success_message(command, &cap_vis, baseline);
    (silent, exit_code)
}

/// Instant notebook commands: delta since command baseline, then command-echo fallback.
fn capture_instant_command_output(
    state: &AppState,
    terminal_id: Uuid,
    baseline: &str,
    command: &str,
    exec_line: &str,
) -> (String, Option<i32>) {
    let cap = crate::terminals::capture_pane_for_terminal(state, terminal_id).unwrap_or_default();
    let (parsed, mut exit_code) =
        crate::terminals::capture_instant_notebook_output(baseline, &cap, command, exec_line);
    let out = sanitize_collected_output(&parsed, command, exec_line);
    if !out.is_empty() {
        if exit_code.is_none() {
            let (_, code) = crate::terminals::notebook_parse_captured_output(&cap);
            exit_code = code;
        }
        return (out, exit_code);
    }
    let cap_vis =
        crate::terminals::capture_pane_visible_for_terminal(state, terminal_id).unwrap_or_default();
    let (parsed, mut exit_code) =
        crate::terminals::capture_instant_notebook_output(baseline, &cap_vis, command, exec_line);
    let out = sanitize_collected_output(&parsed, command, exec_line);
    if exit_code.is_none() {
        let (_, code) = crate::terminals::notebook_parse_captured_output(&cap_vis);
        exit_code = code;
    }
    (out, exit_code)
}

fn exec_line_expects_exit_marker(exec_line: &str) -> bool {
    exec_line.contains(crate::terminals::NOTEBOOK_EXIT_MARKER)
}

fn git_commit_nothing_staged_in_cap(cap: &str, command: &str, exec_line: &str) -> bool {
    for cmd in [exec_line, command] {
        let extracted = crate::terminals::extract_command_output_from_pane(cap, cmd);
        let (parsed, _) = crate::terminals::notebook_parse_captured_output(&extracted);
        let lower = parsed.to_lowercase();
        if lower.contains("nothing to commit")
            || lower.contains("nothing added to commit")
            || lower.contains("no changes added to commit")
        {
            return true;
        }
    }
    false
}

fn git_commit_output_from_pane(cap: &str, command: &str, exec_line: &str) -> String {
    for cmd in [exec_line, command] {
        let extracted = crate::terminals::extract_command_output_from_pane(cap, cmd);
        let (parsed, _) = crate::terminals::notebook_parse_captured_output(&extracted);
        let out = sanitize_collected_output(&parsed, command, exec_line);
        if !out.is_empty() {
            return out;
        }
    }
    String::new()
}

fn reconcile_git_commit_capture(
    command: &str,
    exec_line: &str,
    output: &str,
    cap: &str,
    exit_code: Option<i32>,
) -> (String, Option<i32>) {
    if !command.to_lowercase().contains("git commit") {
        return (output.to_string(), exit_code);
    }
    if !git_commit_nothing_staged_in_cap(cap, command, exec_line) {
        return (output.to_string(), exit_code);
    }
    let full = git_commit_output_from_pane(cap, command, exec_line);
    let out = if full.is_empty() {
        output.to_string()
    } else {
        full
    };
    (out, Some(1))
}

fn infer_failure_message_from_pane(command: &str, cap: &str, exec_line: &str) -> String {
    let lower = command.trim().to_lowercase();
    if lower.contains("git commit") {
        let out = git_commit_output_from_pane(cap, command, exec_line);
        let ol = out.to_lowercase();
        if ol.contains("nothing to commit")
            || ol.contains("nothing added to commit")
            || ol.contains("no changes added to commit")
        {
            return out;
        }
    }
    for line in cap.lines() {
        let t = line.trim();
        if t.is_empty()
            || is_shell_prompt_line(t)
            || t == command
            || t == exec_line
            || t.contains(crate::terminals::NOTEBOOK_EXIT_MARKER)
        {
            continue;
        }
        let tl = t.to_lowercase();
        if lower.starts_with("git ") && tl.contains("fatal:") {
            return t.to_string();
        }
    }
    String::new()
}

fn infer_silent_success_message(command: &str, cap: &str, baseline: &str) -> String {
    let cmd = command.trim();
    let lower = cmd.to_lowercase();

    if lower.starts_with("python3 -m venv ")
        || lower.starts_with("python -m venv ")
        || lower.starts_with("python3 -m virtualenv ")
    {
        if let Some(path) = cmd.split_whitespace().nth(3) {
            let activate = std::path::Path::new(path).join("bin/activate");
            if activate.is_file() {
                return format!("Virtual environment created at {path}");
            }
        }
    }

    if lower.starts_with("source ") && lower.contains("activate") {
        for line in cap.lines().rev().take(8) {
            let t = line.trim();
            if let (Some(a), Some(b)) = (t.find('('), t.find(')')) {
                if b > a + 1 && (t.contains('#') || t.contains('$')) {
                    return format!("Activated {}", &t[a..=b]);
                }
            }
        }
    }

    if lower.starts_with("deactivate") || lower.contains("/deactivate") {
        return "Virtual environment deactivated.".to_string();
    }

    String::new()
}

fn sanitize_collected_output(raw: &str, command: &str, exec_line: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if is_notebook_capture_noise(trimmed, command, exec_line) {
            continue;
        }
        if is_shell_prompt_line(trimmed) {
            continue;
        }
        // Prompt + command echo, e.g. root@host:~# ls
        if trimmed.ends_with(command)
            && (trimmed.contains(":~#")
                || trimmed.contains(":~$")
                || trimmed.contains("$ ")
                || (trimmed.contains('@') && trimmed.contains('#')))
        {
            continue;
        }
        lines.push(line.to_string());
    }
    while lines
        .last()
        .is_some_and(|l| is_shell_prompt_line(l.trim()) || is_notebook_capture_noise(l.trim(), command, exec_line))
    {
        lines.pop();
    }
    lines.join("\n")
}

/// Strip notebook wrapper echoes and partial `__BUNNY_EXIT__$?` capture artifacts.
fn is_notebook_capture_noise(line: &str, command: &str, exec_line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() || t == "?" {
        return true;
    }
    if t == command || t == exec_line {
        return true;
    }
    if t.contains(crate::terminals::NOTEBOOK_EXIT_MARKER) || t.contains("BUNNY_EXIT__") {
        return true;
    }
    if t.starts_with("PAGER=cat GIT_PAGER=cat") {
        return true;
    }
    if t.starts_with("(PAGER=cat GIT_PAGER=cat") {
        return true;
    }
    if t.contains("PAGER=cat GIT_PAGER=cat") && t.contains("2>&1") {
        return true;
    }
    if t.contains("; echo ") && t.contains("EXIT__") {
        return true;
    }
    // Wrapped / split marker echo: `UNNY_EXIT__$?`, `T__$?`, `__0`, etc.
    if t.ends_with("$?") && (t.contains("EXIT__") || t.len() <= 12) {
        return true;
    }
    if t.contains("EXIT__") {
        return true;
    }
    false
}

/// Typical bash/zsh prompt line: `user@host:path#` or `user@host:path$`.
fn is_shell_prompt_line(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    let Some(at) = t.find('@') else {
        return false;
    };
    let after_at = &t[at + 1..];
    if !after_at.contains(':') {
        return false;
    }
    matches!(t.chars().last(), Some('#' | '$' | '%'))
}

pub fn migrate_scrollback_to_blocks(state: &AppState) -> Result<()> {
    let db = state.auth.db();
    let already = db.lock().app_meta_get("terminal_blocks_migrated")?;
    if already.as_deref() == Some("1") {
        return Ok(());
    }

    let term_ids = db.lock().list_all_terminal_ids()?;
    for term_id in term_ids {
        let merged = crate::terminals::load_scrollback_for_replay(state, term_id);
        if merged.trim().is_empty() {
            continue;
        }
        let existing = db.lock().list_terminal_blocks(term_id, 1, 1)?;
        if !existing.is_empty() {
            continue;
        }
        import_scrollback_text(state, term_id, &merged)?;
    }

    db.lock().app_meta_set("terminal_blocks_migrated", "1")?;
    Ok(())
}

fn import_scrollback_text(state: &AppState, term_id: Uuid, text: &str) -> Result<()> {
    let mut current_command: Option<String> = None;
    let mut output_lines: Vec<String> = Vec::new();

    let flush_output = |state: &AppState,
                        term_id: Uuid,
                        cmd: &Option<String>,
                        lines: &mut Vec<String>| {
        if lines.is_empty() {
            return;
        }
        let content = lines.join("\n");
        lines.clear();
        let _ = append_block(
            state,
            AppendBlockParams {
                terminal_id: term_id,
                kind: if cmd.is_some() {
                    BlockKind::Output
                } else {
                    BlockKind::SystemEvent
                },
                author_user_id: None,
                author_display: if cmd.is_some() {
                    "discord".into()
                } else {
                    "system".into()
                },
                author_source: if cmd.is_some() {
                    AuthorSource::Discord
                } else {
                    AuthorSource::System
                },
                command: None,
                content,
                status: BlockStatus::Completed,
                exit_code: Some(0),
                parent_block_id: None,
                meta: serde_json::json!({"migrated": true}),
            },
        );
    };

    for line in text.lines() {
        if let Some(cmd) = line.strip_prefix("[discord] $ ") {
            flush_output(state, term_id, &current_command, &mut output_lines);
            current_command = Some(cmd.trim().to_string());
            let _ = append_block(
                state,
                AppendBlockParams {
                    terminal_id: term_id,
                    kind: BlockKind::DiscordCommand,
                    author_user_id: None,
                    author_display: "discord".into(),
                    author_source: AuthorSource::Discord,
                    command: current_command.clone(),
                    content: String::new(),
                    status: BlockStatus::Completed,
                    exit_code: None,
                    parent_block_id: None,
                    meta: serde_json::json!({"migrated": true}),
                },
            );
        } else {
            output_lines.push(line.to_string());
        }
    }
    flush_output(state, term_id, &current_command, &mut output_lines);
    Ok(())
}

#[cfg(test)]
mod capture_noise_tests {
    use super::*;

    #[test]
    fn strips_exit_marker_fragments() {
        let cmd = "ls";
        let exec = "(PAGER=cat GIT_PAGER=cat ls) 2>&1; echo __BUNNY_EXIT__$?";
        let raw = "UNNY_EXIT__$?\nbin  hello.md  lib\npyvenv.cfg";
        assert_eq!(
            sanitize_collected_output(raw, cmd, exec),
            "bin  hello.md  lib\npyvenv.cfg"
        );
    }

    #[test]
    fn strips_short_exit_fragments_and_wrapper_echo() {
        let cmd = "echo hello";
        let exec = "(PAGER=cat GIT_PAGER=cat echo hello) 2>&1; echo __BUNNY_EXIT__$?";
        let raw = "T__$?\nhello";
        assert_eq!(sanitize_collected_output(raw, cmd, exec), "hello");
        assert!(is_notebook_capture_noise(
            "PAGER=cat GIT_PAGER=cat echo hello",
            cmd,
            exec
        ));
    }

    #[test]
    fn strips_lone_question_mark() {
        let cmd = "ls -la";
        let exec = "(PAGER=cat GIT_PAGER=cat ls -la) 2>&1; echo __BUNNY_EXIT__$?";
        let raw = "?\ntotal 56";
        assert_eq!(sanitize_collected_output(raw, cmd, exec), "total 56");
    }

    #[test]
    fn instant_capture_uses_baseline_prefix() {
        let baseline = concat!(
            "root@host:~# (PAGER=cat GIT_PAGER=cat ls) 2>&1; echo __BUNNY_EXIT__$?\n",
            "project\n",
            "__BUNNY_EXIT__0\n",
            "root@host:~# (PAGER=cat GIT_PAGER=cat cd project) 2>&1; echo __BUNNY_EXIT__$?\n",
            "__BUNNY_EXIT__0\n",
            "root@host:~/project# "
        );
        let cap = format!(
            "{baseline}(PAGER=cat GIT_PAGER=cat ls) 2>&1; echo __BUNNY_EXIT__$?\n\
             bin  lib  pyvenv.cfg  tentative.md\n\
             __BUNNY_EXIT__0\n\
             root@host:~/project# "
        );
        let cmd = "ls";
        let exec = "(PAGER=cat GIT_PAGER=cat ls) 2>&1; echo __BUNNY_EXIT__$?";
        let (parsed, code) =
            crate::terminals::capture_instant_notebook_output(baseline, &cap, cmd, exec);
        let out = sanitize_collected_output(&parsed, cmd, exec);
        assert_eq!(code, Some(0));
        assert_eq!(out, "bin  lib  pyvenv.cfg  tentative.md");
        assert!(!out.contains("project"));
    }

    #[test]
    fn line_echoes_command_short_names_avoid_false_positives() {
        use crate::terminals::extract_command_output_from_pane;
        let cap = concat!(
            "root@host:~/project# (PAGER=cat GIT_PAGER=cat ls) 2>&1; echo __BUNNY_EXIT__$?\n",
            "bin  lib  pyvenv.cfg  tentative.md\n",
            "__BUNNY_EXIT__0\n",
            "root@host:~/project# "
        );
        let out = extract_command_output_from_pane(cap, "ls");
        let (parsed, _) = crate::terminals::notebook_parse_captured_output(&out);
        let clean = sanitize_collected_output(
            &parsed,
            "ls",
            "(PAGER=cat GIT_PAGER=cat ls) 2>&1; echo __BUNNY_EXIT__$?",
        );
        assert_eq!(clean, "bin  lib  pyvenv.cfg  tentative.md");
    }

    #[test]
    fn git_commit_without_staged_files_is_failure() {
        let cap = concat!(
            "root@host:~/yo# (PAGER=cat GIT_PAGER=cat git commit -m \"nothing\") 2>&1; echo __BUNNY_EXIT__$?\n",
            "On branch master\n",
            "\n",
            "Initial commit\n",
            "\n",
            "nothing to commit (create/copy files and use \"git add\" to track)\n",
            "__BUNNY_EXIT__1\n",
        );
        let cmd = "git commit -m \"nothing\"";
        let exec = "(PAGER=cat GIT_PAGER=cat git commit -m \"nothing\") 2>&1; echo __BUNNY_EXIT__$?";
        assert!(git_commit_nothing_staged_in_cap(cap, cmd, exec));
        let (out, code) = reconcile_git_commit_capture(cmd, exec, "Initial commit", cap, Some(0));
        assert_eq!(code, Some(1));
        assert!(out.contains("nothing to commit"));
        assert!(out.contains("Initial commit"));
    }

    #[test]
    fn git_commit_stale_scrollback_does_not_trigger_false_nothing_staged() {
        let cap = concat!(
            "earlier: nothing to commit (create/copy files and use \"git add\" to track)\n",
            "root@host:~/project# (PAGER=cat GIT_PAGER=cat git commit -m \"try\") 2>&1; echo __BUNNY_EXIT__$?\n",
            "[master deadbeef] try\n",
            " 1 file changed, 1 insertion(+)\n",
            "__BUNNY_EXIT__0\n",
            "root@host:~/project# "
        );
        let cmd = "git commit -m \"try\"";
        let exec = "(PAGER=cat GIT_PAGER=cat git commit -m \"try\") 2>&1; echo __BUNNY_EXIT__$?";
        assert!(!git_commit_nothing_staged_in_cap(cap, cmd, exec));
        let inferred = infer_failure_message_from_pane(cmd, cap, exec);
        assert!(inferred.is_empty());
    }
}
