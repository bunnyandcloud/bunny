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
    let discord = scrollback::load_discord_sidecar(&dir, record.id);
    let text = scrollback::merge_discord_transcript(&text, &discord);
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

    let secret_env = state.secret_env_for_session(record.session_id);

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
                        &secret_env,
                    )?),
                    true,
                )
            }
        }
    } else {
        (None, false)
    };

    let initial_scrollback = format_initial_scrollback(history, fresh_shell);

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

/// Write secrets to a root-only script and `source` it (values never appear on screen).
fn inject_secrets_via_env_file(
    state: &AppState,
    terminal_id: Uuid,
    secret_env: &std::collections::HashMap<String, String>,
) -> Result<()> {
    let dir = std::path::Path::new(&state.data_dir).join("secret-inject");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{terminal_id}.sh"));
    let mut body = String::from("# bunny — do not commit\n");
    for (key, value) in secret_env {
        if key.starts_with("BUNNY_SECRET_") {
            let escaped = value.replace('\'', "'\"'\"'");
            body.push_str(&format!("export {key}='{escaped}'\n"));
        }
    }
    std::fs::write(&path, &body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    let path_str = path.to_string_lossy();
    state.terminals.write(terminal_id, "stty -echo 2>/dev/null\n")?;
    state.terminals.write(terminal_id, &format!(". \"{path_str}\"\n"))?;
    state.terminals.write(terminal_id, "stty echo 2>/dev/null\n")?;
    Ok(())
}

/// After vault unlock, push `BUNNY_SECRET_*` into shells that are already running.
/// When `session_id` is set, only terminals for that workspace session are updated.
pub fn refresh_secrets_in_running_shells(state: &AppState, session_id: Option<Uuid>) {
    if !state.secrets.lock().is_unlocked() {
        return;
    }

    let mut records: Vec<TerminalRecord> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for (tid, sid) in state.terminal_sessions.read().iter() {
        if session_id.is_some_and(|want| *sid != want) {
            continue;
        }
        if !seen.insert(*tid) {
            continue;
        }
        if let Ok(Some(row)) = state.auth.db().lock().get_terminal(*tid) {
            records.push(row_to_record(row));
        }
    }

    let db_rows = if let Some(sid) = session_id {
        state
            .auth
            .db()
            .lock()
            .list_terminals_for_session(sid)
            .unwrap_or_default()
    } else {
        state
            .auth
            .db()
            .lock()
            .list_terminals_with_status("running")
            .unwrap_or_default()
    };

    for row in db_rows {
        let record = row_to_record(row);
        if session_id.is_some_and(|sid| record.session_id != sid) {
            continue;
        }
        if seen.insert(record.id) {
            records.push(record);
        }
    }

    if records.is_empty() {
        tracing::info!(
            ?session_id,
            "vault unlocked: no terminals to refresh with secrets"
        );
        return;
    }

    let default_shell = state.config.terminal.shell.clone();
    let tmux_installed = tmux::available();
    let mut refreshed = 0usize;

    for record in records {
        let secret_env = state.secret_env_for_session(record.session_id);
        if secret_env.is_empty() {
            continue;
        }

        let cwd = PathBuf::from(&record.cwd);
        let shell = if record.shell.is_empty() {
            default_shell.as_str()
        } else {
            record.shell.as_str()
        };

        let mut done = false;
        if tmux_installed {
            let target = resolve_tmux_target(state, &record)
                .ok()
                .flatten()
                .or_else(|| {
                    let name = tmux::terminal_session_name(record.id);
                    if tmux::has_session(&name) {
                        Some(name)
                    } else {
                        None
                    }
                });
            if let Some(target) = target {
                let cwd = tmux::pane_cwd(&target)
                    .map(PathBuf::from)
                    .unwrap_or(cwd);
                match tmux::reload_shell_secrets(&target, &cwd, shell, &secret_env) {
                    Ok(()) => {
                        state.terminals.refresh_display(record.id);
                        done = true;
                    }
                    Err(e) => tracing::warn!(
                        terminal = %record.id,
                        error = %e,
                        "tmux reload_shell_secrets failed"
                    ),
                }
            }
        }

        if !done && state.terminals.get(record.id).is_some() {
            match inject_secrets_via_env_file(state, record.id, &secret_env) {
                Ok(()) => done = true,
                Err(e) => tracing::warn!(
                    terminal = %record.id,
                    error = %e,
                    "env-file secret inject failed"
                ),
            }
        }

        if !done {
            tracing::warn!(
                terminal = %record.id,
                tmux_installed,
                "vault unlock: could not refresh secrets for this shell"
            );
            continue;
        }

        refreshed += 1;
        tracing::info!(
            terminal = %record.id,
            session = %record.session_id,
            vars = secret_env.len(),
            tmux = tmux_installed,
            "refreshed shell secrets after vault unlock"
        );
    }

    tracing::info!(
        ?session_id,
        refreshed,
        total = seen.len(),
        "vault unlock secret refresh complete"
    );
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

/// Run a command for Discord without typing into the interactive shell (keeps tmux pane clean).
/// Uses the pane's current working directory and session vault secrets.
pub fn exec_discord_shell_command(
    state: &AppState,
    term_id: Uuid,
    session_id: Uuid,
    command: &str,
) -> Result<(String, i32)> {
    use bunny_pty::locale;
    use std::process::Command;

    let auth_db = state.auth.db();
    let row = auth_db
        .lock()
        .get_terminal(term_id)?
        .ok_or_else(|| anyhow::anyhow!("terminal not found"))?;
    let record = row_to_record(row);

    let cwd = if state.terminals.uses_tmux() {
        resolve_tmux_target(state, &record)?
            .and_then(|t| tmux::pane_cwd(&t))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(&record.cwd))
    } else {
        PathBuf::from(&record.cwd)
    };

    let shell = if record.shell.is_empty() {
        state.config.terminal.shell.clone()
    } else {
        record.shell.clone()
    };

    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    let mut cmd = Command::new(&shell);
    cmd.arg("-lc").arg(command).current_dir(&cwd);
    for (k, v) in locale::utf8_locale_vars() {
        cmd.env(k, v);
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("HOME", &home);
    cmd.env("PWD", cwd.display().to_string());
    cmd.env(
        "PATH",
        format!("{home}/.local/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"),
    );
    for (k, v) in state.secret_env_for_session(session_id) {
        cmd.env(k, v);
    }

    let out = cmd.output()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let text = match (stdout.trim_end().is_empty(), stderr.trim_end().is_empty()) {
        (true, true) => String::new(),
        (false, true) => stdout.trim_end().to_string(),
        (true, false) => stderr.trim_end().to_string(),
        (false, false) => format!("{stdout}{stderr}").trim_end().to_string(),
    };
    append_discord_transcript(state, term_id, command, &text);
    Ok((text, out.status.code().unwrap_or(1)))
}

fn discord_transcript_entry(command: &str, output: &str) -> String {
    let display = mirror_output_for_display(output);
    format!("[discord] $ {command}\n{display}\n")
}

fn discord_transcript_entry_terminal(command: &str, output: &str) -> String {
    let display = mirror_output_for_display(output);
    format!("\r\n[discord] $ {command}\r\n{display}\r\n")
}

/// Record Discord runs for Web UI scrollback and snapshot overlay (target shell only).
fn append_discord_transcript(state: &AppState, term_id: Uuid, command: &str, output: &str) {
    let dir = scrollback_dir(state);
    let _ = std::fs::create_dir_all(&dir);
    let entry = discord_transcript_entry(command, output);
    let mut entry_terminal = discord_transcript_entry_terminal(command, output);
    if let Some(prompt) = terminal_prompt_line(state, term_id) {
        entry_terminal.push_str("\r\n");
        entry_terminal.push_str(&prompt);
    }

    let path = discord_transcript_path(state, term_id);
    let mut discord_only = std::fs::read_to_string(&path).unwrap_or_default();
    discord_only.push_str(&entry);
    const MAX_BYTES: usize = 32_768;
    if discord_only.len() > MAX_BYTES {
        let keep = &discord_only[discord_only.len() - MAX_BYTES..];
        discord_only = keep[keep.find('\n').map(|i| i + 1).unwrap_or(0)..].to_string();
    }
    let _ = std::fs::write(&path, discord_only);

    let scroll = scrollback::load(&dir, term_id).unwrap_or_default();
    scrollback::save(&dir, term_id, &format!("{scroll}{entry_terminal}"));

    state.terminals.inject_transcript(term_id, &entry_terminal);
}

fn terminal_prompt_line(state: &AppState, term_id: Uuid) -> Option<String> {
    let target = state.terminals.tmux_target(term_id)?;
    if !tmux::target_alive(&target) {
        return None;
    }
    let cap = tmux::capture_pane_visible(&target).ok()?;
    cap.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(|s| s.to_string())
}

fn discord_transcript_path(state: &AppState, term_id: Uuid) -> PathBuf {
    scrollback_dir(state).join(format!("{}.discord", term_id.as_simple()))
}

/// Main scrollback + persisted Discord transcript (PTY saves tmux-only and would drop [discord] lines).
pub fn load_scrollback_for_replay(state: &AppState, term_id: Uuid) -> String {
    let dir = scrollback_dir(state);
    let base = scrollback::load(&dir, term_id).unwrap_or_default();
    let discord = scrollback::load_discord_sidecar(&dir, term_id);
    scrollback::merge_discord_transcript(&base, &discord)
}

/// WebSocket full replay: keep the longer tmux history, always append missing Discord sidecar lines.
pub fn build_terminal_replay(state: &AppState, term_id: Uuid, live_buffer: &str) -> String {
    let dir = scrollback_dir(state);
    let disk = load_scrollback_for_replay(state, term_id);
    let sidecar = scrollback::load_discord_sidecar(&dir, term_id);
    let base = if live_buffer.len() > disk.len() {
        live_buffer
    } else {
        &disk
    };
    scrollback::merge_discord_transcript(base, &sidecar)
}

/// Recent Discord transcript tail merged into shell snapshots only.
pub fn discord_transcript_for_snapshot(state: &AppState, term_id: Uuid) -> String {
    let path = discord_transcript_path(state, term_id);
    let full = std::fs::read_to_string(&path).unwrap_or_default();
    const TAIL: usize = 1500;
    if full.len() <= TAIL {
        full
    } else {
        full[full.len() - TAIL..].to_string()
    }
}

/// Insert Discord transcript above the shell prompt line for PNG snapshots.
pub fn merge_discord_transcript_into_pane(pane: &str, discord: &str) -> String {
    let discord = discord.trim();
    if discord.is_empty() {
        return pane.to_string();
    }
    let pane = pane.trim_end();
    if pane.contains("[discord] $") {
        return format!("{pane}\n");
    }
    match pane.rfind('\n') {
        Some(i) => {
            let (head, prompt) = pane.split_at(i);
            format!("{head}\n{discord}\n{}", prompt.trim_start_matches('\n'))
        }
        None => format!("{discord}\n{pane}\n"),
    }
}

fn mirror_output_for_display(output: &str) -> String {
    const MAX_CHARS: usize = 2000;
    const MAX_LINES: usize = 24;
    if output.trim().is_empty() {
        return "(no output)".into();
    }
    let lines: Vec<&str> = output.lines().collect();
    if lines.len() > MAX_LINES {
        format!(
            "{}\n… ({} more lines)",
            lines[..MAX_LINES].join("\n"),
            lines.len() - MAX_LINES
        )
    } else if output.len() > MAX_CHARS {
        format!("{}…", &output[..MAX_CHARS])
    } else {
        output.to_string()
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
