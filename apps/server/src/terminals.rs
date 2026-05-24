//! Terminal persistence and re-attach after agent restart or client disconnect.

use crate::state::AppState;
use anyhow::Result;
use bunny_auth::db::TerminalRow;
use bunny_core::types::TerminalStatus;
use bunny_pty::{scrollback, tmux};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct TerminalRecord {
    pub id: Uuid,
    pub session_id: Uuid,
    pub name: String,
    pub shell: String,
    pub init_command: Option<String>,
    pub cwd: String,
    pub status: String,
    pub cols: u16,
    pub rows: u16,
    pub tmux_target: Option<String>,
}

/// Login directory for new shells (SSH-like: user home, not the agent's cwd).
pub fn default_shell_cwd() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        let home = PathBuf::from(home);
        if home.is_dir() {
            return home;
        }
    }
    PathBuf::from("/")
}

/// Default label stored on stream sessions (metadata only; shells use [`default_shell_cwd`]).
pub fn default_session_path_label() -> String {
    default_shell_cwd().to_string_lossy().into_owned()
}

fn abs_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub fn scrollback_dir(state: &AppState) -> PathBuf {
    PathBuf::from(&state.data_dir).join("terminal-scrollback")
}

/// Disk snapshot + tmux scrollback (if the session/window still exists).
fn collect_persisted_scrollback(state: &AppState, record: &TerminalRecord) -> (String, PathBuf) {
    let dir = scrollback_dir(state);
    let mut hist = scrollback::load(&dir, record.id);
    let mut cwd = scrollback::load_cwd(&dir, record.id).map(PathBuf::from);
    for target in tmux_target_candidates(record) {
        if tmux::target_alive(&target) {
            if let Ok(cap) = tmux::capture_pane(&target) {
                hist = Some(scrollback::merge(hist, cap));
            }
            if let Some(pane_cwd) = tmux::pane_cwd(&target) {
                cwd = Some(PathBuf::from(pane_cwd));
            }
            break;
        }
    }
    let text = hist.unwrap_or_default();
    let cwd_for_save = cwd
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned());
    scrollback::save_session(
        &dir,
        record.id,
        &text,
        cwd_for_save.as_deref(),
    );
    let resume_cwd = cwd.unwrap_or_else(|| PathBuf::from(&record.cwd));
    (text, resume_cwd)
}

fn format_initial_scrollback(history: String, fresh_shell: bool) -> Option<String> {
    if history.is_empty() {
        return None;
    }
    Some(if fresh_shell {
        format!(
            "{history}\r\n\x1b[90m─── history (read-only) — new shell below ───\x1b[0m\r\n"
        )
    } else {
        history
    })
}

pub fn persist_terminal(
    state: &AppState,
    id: Uuid,
    session_id: Uuid,
    name: &str,
    shell: &str,
    init_command: Option<&str>,
    cwd: &Path,
    cols: u16,
    rows: u16,
    tmux_target: Option<&str>,
) -> Result<()> {
    let cwd = abs_path(cwd);
    let auth_db = state.auth.db();
    {
        auth_db.lock().upsert_terminal(
            id,
            session_id,
            name,
            shell,
            init_command,
            &cwd.to_string_lossy(),
            cols,
            rows,
            "running",
            tmux_target,
        )?;
    }
    Ok(())
}

pub fn remove_terminal_record(state: &AppState, id: Uuid) -> Result<()> {
    let auth_db = state.auth.db();
    auth_db.lock().delete_terminal(id)?;
    Ok(())
}

/// Candidate attach targets — per-terminal session first (never alphabetical sort).
fn tmux_target_candidates(record: &TerminalRecord) -> Vec<String> {
    let mut out = Vec::new();
    let per_terminal = tmux::terminal_session_name(record.id);
    if let Some(t) = &record.tmux_target {
        if t != &per_terminal {
            out.push(t.clone());
        }
    }
    out.insert(0, per_terminal);
    let legacy = tmux::inferred_target(record.session_id, record.id);
    if !out.contains(&legacy) {
        out.push(legacy);
    }
    out
}

/// Resolve tmux attach target; migrates DB when a newer target name is alive.
fn resolve_tmux_target(
    state: &AppState,
    record: &TerminalRecord,
) -> Result<Option<String>> {
    if !state.terminals.uses_tmux() {
        return Ok(None);
    }

    for target in tmux_target_candidates(record) {
        if tmux::target_alive(&target) {
            if record.tmux_target.as_deref() != Some(target.as_str()) {
                let cwd = PathBuf::from(&record.cwd);
                let _ = persist_terminal(
                    state,
                    record.id,
                    record.session_id,
                    &record.name,
                    &record.shell,
                    record.init_command.as_deref(),
                    &cwd,
                    record.cols,
                    record.rows,
                    Some(&target),
                );
            }
            sync_status_to_db(state, record.id, TerminalStatus::Running);
            return Ok(Some(target));
        }
    }

    Ok(None)
}

/// True when there is no live attach client (or the tmux pane died).
pub fn needs_reattach(state: &AppState, id: Uuid) -> bool {
    if state.terminals.get(id).is_none() {
        return true;
    }
    match state.terminals.status(id) {
        Some(TerminalStatus::Running) | Some(TerminalStatus::Starting) => {
            if let Some(target) = state.terminals.tmux_target(id) {
                tmux::pane_is_dead(&target)
            } else {
                false
            }
        }
        _ => true,
    }
}

/// Drop stale attach clients and (re)connect to tmux.
pub fn prepare_terminal_connection(state: &AppState, id: Uuid) -> Result<()> {
    if state.terminals.get(id).is_some() && !needs_reattach(state, id) {
        return Ok(());
    }
    if state.terminals.get(id).is_some() {
        detach_terminal(state, id);
    }
    let auth_db = state.auth.db();
    let row = auth_db
        .lock()
        .get_terminal(id)?
        .ok_or_else(|| anyhow::anyhow!("terminal not in database"))?;
    attach_terminal_record(state, &row_to_record(row))
}

/// Load terminal row from DB and attach to tmux if still alive.
pub fn try_reattach_terminal(state: &AppState, id: Uuid) -> Result<()> {
    prepare_terminal_connection(state, id)
}

fn sync_status_to_db(state: &AppState, id: Uuid, status: TerminalStatus) {
    let s = match status {
        TerminalStatus::Running | TerminalStatus::Starting | TerminalStatus::Reconnectable => {
            "running"
        }
        TerminalStatus::Exited | TerminalStatus::Stopped => "exited",
        TerminalStatus::Crashed => "crashed",
        _ => "running",
    };
    let auth_db = state.auth.db();
    let _ = auth_db.lock().update_terminal_status(id, s);
}

pub fn attach_terminal_record(state: &AppState, record: &TerminalRecord) -> Result<()> {
    if state.terminals.get(record.id).is_some() && !needs_reattach(state, record.id) {
        return Ok(());
    }
    let (history, resume_cwd) = collect_persisted_scrollback(state, record);

    if state.terminals.get(record.id).is_some() {
        detach_terminal(state, record.id);
    }

    let (tmux_target, fresh_shell) = if state.terminals.uses_tmux() {
        match resolve_tmux_target(state, record)? {
            Some(t) => (Some(t), false),
            None => {
                tracing::info!(
                    terminal = %record.id,
                    resume_cwd = %resume_cwd.display(),
                    "tmux session gone after agent stop — starting a fresh shell"
                );
                (
                    Some(tmux::ensure_terminal_session(
                        record.id,
                        &resume_cwd,
                        record.init_command.as_deref(),
                    )?),
                    true,
                )
            }
        }
    } else {
        (None, false)
    };

    let initial_scrollback = format_initial_scrollback(history, fresh_shell);

    let secret_env = state.secret_env_for_session(record.session_id);
    let (term_id, tmux_out) = state.terminals.create_with_id(
        record.id,
        record.session_id,
        &record.name,
        &resume_cwd,
        record.init_command.as_deref(),
        record.cols,
        record.rows,
        secret_env,
        tmux_target.as_deref(),
        initial_scrollback,
    )?;
    debug_assert_eq!(term_id, record.id);
    state.terminals.hydrate_scrollback_from_disk(record.id);
    let persisted_target = tmux_out.as_deref().or(tmux_target.as_deref());
    if persisted_target != record.tmux_target.as_deref() {
        let _ = persist_terminal(
            state,
            record.id,
            record.session_id,
            &record.name,
            &record.shell,
            record.init_command.as_deref(),
            &resume_cwd,
            record.cols,
            record.rows,
            persisted_target,
        );
    }
    state
        .terminal_sessions
        .write()
        .insert(record.id, record.session_id);
    Ok(())
}

fn row_to_record(row: TerminalRow) -> TerminalRecord {
    TerminalRecord {
        id: row.0,
        session_id: row.1,
        name: row.2,
        shell: row.3,
        init_command: row.4,
        cwd: row.5,
        status: row.6,
        cols: row.7,
        rows: row.8,
        tmux_target: row.9,
    }
}

pub fn ensure_session_terminals_live(state: &Arc<AppState>, session_id: Uuid) {
    let auth_db = state.auth.db();
    let rows = auth_db
        .lock()
        .list_terminals_for_session(session_id)
        .unwrap_or_default();
    let records: Vec<TerminalRecord> = rows.into_iter().map(row_to_record).collect();

    for record in records {
        if !needs_reattach(state, record.id) {
            continue;
        }
        if let Err(e) = attach_terminal_record(state, &record) {
            tracing::warn!(%record.id, error = %e, "failed to re-attach terminal");
            let _ = auth_db.lock().update_terminal_status(record.id, "exited");
        }
    }
}

/// Kill every terminal for a session (memory + SQLite). Tmux sessions are destroyed.
pub fn teardown_session(state: &AppState, session_id: Uuid) -> Result<()> {
    let auth_db = state.auth.db();
    let db_rows = auth_db
        .lock()
        .list_terminals_for_session(session_id)
        .unwrap_or_default();

    let mut term_ids: std::collections::HashSet<Uuid> =
        db_rows.iter().map(|(id, ..)| *id).collect();
    for (tid, sid) in state.terminal_sessions.read().iter() {
        if *sid == session_id {
            term_ids.insert(*tid);
        }
    }

    for term_id in term_ids {
        if state.terminals.uses_tmux() {
            if let Some(row) = db_rows.iter().find(|(id, ..)| *id == term_id) {
                if let Some(ref target) = row.9 {
                    tmux::kill_target(target);
                } else {
                    tmux::kill_terminal_session(term_id);
                }
            } else {
                tmux::kill_terminal_session(term_id);
            }
        }
        detach_terminal(state, term_id);
    }

    if state.terminals.uses_tmux() {
        tmux::kill_stream_session(session_id);
    }

    auth_db.lock().delete_terminals_for_session(session_id)?;
    Ok(())
}

/// Drop WebSocket attach client only (tmux window keeps running).
pub fn detach_terminal(state: &AppState, id: Uuid) {
    if let Some(session) = state.terminals.remove_attach_only(id) {
        session.kill();
    }
    state.terminal_sessions.write().remove(&id);
}

fn records_for_active_stream_sessions(state: &AppState) -> Vec<TerminalRecord> {
    let auth_db = state.auth.db();
    let db = auth_db.lock();
    let sessions = db
        .list_all_stream_sessions()
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, _, st)| st != "stopped" && st != "expired")
        .map(|(id, _, _)| id)
        .collect::<Vec<_>>();
    let mut records = Vec::new();
    for session_id in sessions {
        for row in db.list_terminals_for_session(session_id).unwrap_or_default() {
            records.push(row_to_record(row));
        }
    }
    records
}

pub fn restore_all_terminals(state: &Arc<AppState>) {
    let auth_db = state.auth.db();
    let records = records_for_active_stream_sessions(state);

    if state.terminals.uses_tmux() {
        tracing::info!("terminal backend: tmux (shells survive agent restarts)");
    }

    for record in records {
        if state.terminals.get(record.id).is_some() {
            continue;
        }
        match attach_terminal_record(state, &record) {
            Ok(()) => tracing::info!(terminal = %record.id, "terminal re-attached after agent start"),
            Err(e) => {
                tracing::warn!(terminal = %record.id, error = %e, "terminal re-attach failed");
                let _ = auth_db.lock().update_terminal_status(record.id, "exited");
            }
        }
    }
}
