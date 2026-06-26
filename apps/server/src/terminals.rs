//! Terminal persistence and re-attach after agent restart or client disconnect.

use crate::git_identity::{apply_bunny_path, apply_git_env, git_env_for_user};
use crate::state::AppState;
use anyhow::Result;
use bunny_auth::db::TerminalRow;
use bunny_core::types::TerminalStatus;
use bunny_i18n::Locale;
use bunny_pty::{scrollback, tmux};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct TerminalRecord {
    pub id: Uuid,
    pub session_id: Uuid,
    pub name: String,
    pub shell: String,
    pub init_command: Option<String>,
    pub cwd: String,
    #[allow(dead_code)]
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
pub(crate) fn collect_persisted_scrollback(state: &AppState, record: &TerminalRecord) -> (String, PathBuf) {
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

/// Next unused `shell N` name for a session (matches Web UI naming).
pub fn next_shell_name(state: &AppState, session_id: Uuid) -> String {
    let rows = state
        .auth
        .db()
        .lock()
        .list_terminals_for_session(session_id)
        .unwrap_or_default();
    let used: std::collections::HashSet<String> = rows.iter().map(|(_, _, name, ..)| name.clone()).collect();
    let mut n = rows.len() + 1;
    while used.contains(&format!("shell {n}")) {
        n += 1;
    }
    format!("shell {n}")
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

/// Read-only tmux target lookup (no DB writes — safe for list/poll endpoints).
fn live_tmux_target_readonly(state: &AppState, record: &TerminalRecord) -> Option<String> {
    if !state.terminals.uses_tmux() {
        return None;
    }
    if let Some(t) = state.terminals.tmux_target(record.id) {
        if tmux::target_alive(&t) {
            return Some(t);
        }
    }
    for target in tmux_target_candidates(record) {
        if tmux::target_alive(&target) {
            return Some(target);
        }
    }
    None
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
    if state.terminals.get(id).is_some() {
        match state.terminals.status(id) {
            Some(TerminalStatus::Running) | Some(TerminalStatus::Starting) => return Ok(()),
            _ => {}
        }
        detach_terminal(state, id);
    }
    let auth_db = state.auth.db();
    let row = auth_db
        .lock()
        .get_terminal(id)?
        .ok_or_else(|| anyhow::anyhow!("terminal not in database"))?;
    attach_terminal_record(state, &row_to_record(row))
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
    if state.terminals.get(record.id).is_some() {
        match state.terminals.status(record.id) {
            Some(TerminalStatus::Running) | Some(TerminalStatus::Starting) => return Ok(()),
            _ => {}
        }
    }
    let (history, resume_cwd) = collect_persisted_scrollback(state, record);

    if state.terminals.get(record.id).is_some() {
        detach_terminal(state, record.id);
    }

    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    let mut session_env = state
        .git_identity
        .terminal_session_env(record.id, &home);
    session_env.extend(state.secret_env_for_session(record.session_id));

    let (tmux_target, fresh_shell) = if state.terminals.uses_tmux() {
        match resolve_tmux_target(state, record)? {
            Some(t) => {
                tmux::apply_session_env(tmux::session_name_from_target(&t), &session_env);
                (Some(t), false)
            }
            None => {
                tracing::info!(
                    terminal = %record.id,
                    resume_cwd = %resume_cwd.display(),
                    "tmux session gone after agent stop — starting a fresh shell"
                );
                let interactive_shell = tmux::interactive_shell_command(
                    std::path::Path::new(&state.data_dir),
                    record.id,
                    &state.config.terminal.shell,
                    &session_env,
                )
                .unwrap_or_else(|_| state.config.terminal.shell.clone());
                (
                    Some(tmux::ensure_terminal_session(
                        record.id,
                        &resume_cwd,
                        record.init_command.as_deref(),
                        &interactive_shell,
                        &session_env,
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
        session_env,
        tmux_target.as_deref(),
        initial_scrollback,
    )?;
    debug_assert_eq!(term_id, record.id);
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
        // Never detach/re-attach shells that already have a live in-memory client (Web UI session).
        if state.terminals.get(record.id).is_some() {
            continue;
        }
        if let Err(e) = attach_terminal_record(state, &record) {
            tracing::warn!(%record.id, error = %e, "failed to re-attach terminal");
            let _ = auth_db.lock().update_terminal_status(record.id, "exited");
        }
    }
}

/// Attach one terminal without touching other shells in the session (Discord thread paths).
pub fn ensure_terminal_attached(state: &AppState, term_id: Uuid) -> Result<()> {
    if state.terminals.get(term_id).is_some() && !needs_reattach(state, term_id) {
        return Ok(());
    }
    let row = state
        .auth
        .db()
        .lock()
        .get_terminal(term_id)?
        .ok_or_else(|| anyhow::anyhow!("terminal not in database"))?;
    attach_terminal_record(state, &row_to_record(row))
}

pub fn notify_terminal_created(state: &AppState, session_id: Uuid, term_id: Uuid, name: &str) {
    state.realtime.publish(
        session_id,
        &serde_json::json!({
            "type": "terminal.status.changed",
            "terminalId": term_id.to_string(),
            "name": name,
            "status": "running",
        }),
    );
}

/// Max file size sent to Discord as an attachment (Discord allows up to 25 MB; stay under).
pub const DISCORD_FILE_ATTACHMENT_MAX: u64 = 24 * 1024 * 1024;

/// Read a file relative to the shell cwd for Discord attachment download.
pub fn read_discord_shell_file(
    state: &AppState,
    term_id: Uuid,
    relative_path: &str,
    max_bytes: u64,
) -> Result<(String, Vec<u8>)> {
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

    let path = resolve_discord_file_path(&cwd, relative_path)?;
    let meta = std::fs::metadata(&path)?;
    if !meta.is_file() {
        anyhow::bail!("not a file: {}", path.display());
    }
    let size = meta.len();
    if size > max_bytes {
        anyhow::bail!(
            "file too large for Discord ({size} bytes, max {max_bytes}) — open it in the Web UI terminal"
        );
    }
    let bytes = std::fs::read(&path)?;
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    Ok((filename, bytes))
}

fn resolve_discord_file_path(cwd: &Path, user_path: &str) -> Result<PathBuf> {
    let user_path = user_path.trim();
    if user_path.is_empty() {
        anyhow::bail!("path required");
    }
    if user_path.contains('\0') || user_path.contains("..") {
        anyhow::bail!("invalid path");
    }
    if user_path.starts_with('/') {
        anyhow::bail!("use a path relative to the shell directory (not absolute)");
    }
    let joined = cwd.join(user_path.trim_start_matches("./"));
    let canonical = joined
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("file not found: {e}"))?;
    let cwd_canon = cwd
        .canonicalize()
        .unwrap_or_else(|_| cwd.to_path_buf());
    if !canonical.starts_with(&cwd_canon) {
        anyhow::bail!("path must stay inside the shell working directory");
    }
    Ok(canonical)
}

/// Run a command for Discord without typing into the interactive shell (keeps tmux pane clean).
/// Uses the pane's current working directory and session vault secrets.
pub fn exec_discord_shell_command(
    state: &AppState,
    term_id: Uuid,
    session_id: Uuid,
    command: &str,
    acting_user_id: Option<Uuid>,
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
    cmd.arg("--noprofile")
        .arg("--norc")
        .arg("-c")
        .arg(discord_subprocess_command(state, term_id, command))
        .current_dir(&cwd);
    for (k, v) in locale::utf8_locale_vars() {
        cmd.env(k, v);
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("HOME", &home);
    cmd.env("PWD", cwd.display().to_string());
    apply_discord_shell_env(&mut cmd, state, term_id, session_id, acting_user_id);
    for (k, v) in state.secret_env_for_session(session_id) {
        cmd.env(k, v);
    }

    let out = run_command_with_timeout(&mut cmd, std::time::Duration::from_secs(120), None)?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let text = match (stdout.trim_end().is_empty(), stderr.trim_end().is_empty()) {
        (true, true) => String::new(),
        (false, true) => stdout.trim_end().to_string(),
        (true, false) => stderr.trim_end().to_string(),
        (false, false) => format!("{stdout}{stderr}").trim_end().to_string(),
    };
    append_discord_transcript(
        state,
        term_id,
        command,
        DiscordTranscriptBody::Output(&text),
        acting_user_id,
    );
    Ok((text, out.status.code().unwrap_or(1)))
}

/// How long `/bunny run` waits for a command to finish before treating it as persistent.
pub const DISCORD_RUN_QUICK_WAIT: std::time::Duration = std::time::Duration::from_secs(8);

/// Install/network-heavy commands need longer than the default quick-wait window.
pub fn discord_run_quick_wait(command: &str) -> std::time::Duration {
    let lower = command.trim().to_lowercase();
    if lower.contains("pip install")
        || lower.contains("pip3 install")
        || lower.contains("npm install")
        || lower.contains("npm ci")
        || lower.contains("yarn add")
        || lower.contains("pnpm add")
        || lower.contains("cargo install")
        || lower.contains("apt install")
        || lower.contains("apt-get install")
    {
        return std::time::Duration::from_secs(45);
    }
    DISCORD_RUN_QUICK_WAIT
}

const BUNNY_EXIT_MARKER: &str = "__BUNNY_EXIT__";
pub(crate) const NOTEBOOK_EXIT_MARKER: &str = BUNNY_EXIT_MARKER;
pub const BUNNY_BACKGROUND_PID_MARKER: &str = "[bunny] background pid=";
const DISCORD_BACKGROUND_START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const DISCORD_RUN_EXCERPT_MAX_LINES: usize = 24;
const DISCORD_RUN_EXCERPT_MAX_CHARS: usize = 1400;

/// Result of `/bunny run` for Discord formatting.
#[derive(Debug, Clone)]
pub struct DiscordShellRunResult {
    pub output: String,
    pub exit_code: i32,
    pub persistent: bool,
}

/// Send Ctrl+C to the foreground process in the tmux pane.
pub fn interrupt_terminal_foreground(state: &AppState, term_id: Uuid) -> Result<()> {
    ensure_terminal_attached(state, term_id)?;
    let auth_db = state.auth.db();
    let row = auth_db
        .lock()
        .get_terminal(term_id)?
        .ok_or_else(|| anyhow::anyhow!("terminal not found"))?;
    let record = row_to_record(row);

    let target = resolve_tmux_target(state, &record)?.ok_or_else(|| {
        anyhow::anyhow!("interrupt requires tmux — use the Web UI terminal (Ctrl+C)")
    })?;

    tmux::interrupt_pane_foreground(&target)
}

/// Send Ctrl+C to the shell tmux pane (stops foreground process started via `/bunny run`).
pub fn exec_discord_shell_interrupt(state: &AppState, term_id: Uuid) -> Result<String> {
    interrupt_terminal_foreground(state, term_id)?;
    // Match Web UI `/terminals/:id/run/stop`: clear notebook collectors and cancel
    // running blocks so the shell unlocks and the Running badge disappears.
    crate::blocks::stop_running_processes(state, term_id)?;

    let note = "Interruption (Ctrl+C) sent from Discord.";
    append_discord_transcript(
        state,
        term_id,
        "run_stop",
        DiscordTranscriptBody::Output(note),
        None,
    );
    Ok(note.to_string())
}

fn discord_shell_idle_command_name(command: &str) -> bool {
    let base = command_base_name(command);
    if base.is_empty() {
        return true;
    }
    const IDLE: &[&str] = &["bash", "zsh", "sh", "dash", "fish", "nu", "ksh", "tcsh"];
    IDLE.iter().any(|shell| base == *shell || base.ends_with(shell))
}

fn command_base_name(command: &str) -> String {
    command
        .rsplit('/')
        .next()
        .unwrap_or(command)
        .trim()
        .to_lowercase()
}

/// `less`/`more` started by `tree`, `git log`, etc. — not an intentional interactive session.
pub fn pane_cmd_is_incidental_pager(pane_cmd: &str, user_command: &str) -> bool {
    let pane = command_base_name(pane_cmd);
    if !matches!(pane.as_str(), "less" | "more" | "most") {
        return false;
    }
    let user_first = command_base_name(
        user_command
            .split_whitespace()
            .next()
            .unwrap_or(user_command),
    );
    !matches!(
        user_first.as_str(),
        "less" | "more" | "most" | "man" | "view"
    )
}

/// Prefix notebook commands so pagers write to the pane instead of spawning `less`.
pub fn notebook_shell_exec_line(
    command: &str,
    interactive: bool,
    notebook_shells: bool,
    virtual_env: Option<&str>,
) -> String {
    if !notebook_shells || interactive {
        return command.to_string();
    }
    // `cd`, `source`, etc. must run in the interactive shell — not `( … )` subshells.
    if notebook_shell_state_command(command) {
        return format!("{command}; echo {BUNNY_EXIT_MARKER}$?");
    }
    let command = virtual_env
        .and_then(|venv| wrap_command_with_virtualenv(venv, command))
        .unwrap_or_else(|| command.to_string());
    tmux_pager_wrap(&command)
}

/// Wrap a command for tmux/notebook subshell capture (disable pagers, collect stderr).
pub fn tmux_pager_wrap(command: &str) -> String {
    let command = pager_safe_command(command);
    let first = command_base_name(
        command
            .split_whitespace()
            .next()
            .unwrap_or(&command),
    );
    let inner = format!("PAGER=cat GIT_PAGER=cat {command}");
    let body = if matches!(first.as_str(), "tree" | "find") {
        format!("({inner}) 2>&1 | cat")
    } else {
        format!("({inner}) 2>&1")
    };
    format!("{body}; echo {BUNNY_EXIT_MARKER}$?")
}

/// Prevent `git log` and similar from blocking on `less` inside capture wrappers.
fn pager_safe_command(command: &str) -> String {
    let cmd = command.trim();
    let first = command_base_name(cmd.split_whitespace().next().unwrap_or(cmd));
    if first != "git" || cmd.contains("--no-pager") {
        return cmd.to_string();
    }
    if let Some(rest) = cmd.strip_prefix("git ") {
        format!("git --no-pager {rest}")
    } else if cmd == "git" {
        "git --no-pager".to_string()
    } else {
        cmd.to_string()
    }
}

/// Shell builtins / state commands that must not run in a notebook subshell wrapper.
pub fn notebook_shell_state_command(command: &str) -> bool {
    let cmd = command.trim();
    if cmd.is_empty() || cmd.starts_with('(') || cmd.starts_with('{') {
        return false;
    }
    const BUILTINS: &[&str] = &["cd", "export", "unset", "source", "deactivate", "pushd", "popd"];
    for b in BUILTINS {
        if cmd == *b || cmd.starts_with(&format!("{b} ")) || cmd.starts_with(&format!("{b}\t")) {
            return true;
        }
    }
    if cmd.starts_with(". ") && cmd.contains("activate") {
        return true;
    }
    false
}

/// Common non-interactive commands — skip interactive promotion waits in the notebook collector.
pub fn notebook_instant_command(command: &str) -> bool {
    let cmd = command.trim();
    if matches!(cmd, "ls" | "pwd" | "ls -la") {
        return true;
    }
    let first = cmd.split_whitespace().next().unwrap_or("");
    let base = command_base_name(first);
    if base == "cd" {
        return true;
    }
    // Wrapped git runs finish quickly; rely on `__BUNNY_EXIT__` instead of echo matching.
    base == "git" && !git_command_expects_interactive(cmd)
}

/// Whether runtime prompt heuristics may promote this command to interactive mode.
pub fn runtime_interactive_promotion_allowed(command: &str, pane_cmd: Option<&str>) -> bool {
    if notebook_instant_command(command) {
        return false;
    }
    if user_command_expects_interactive(command) {
        return true;
    }
    let Some(cmd) = pane_cmd else {
        return false;
    };
    let tui = is_interactive_tui_command(cmd) || pane_process_suggests_interactive(cmd, command);
    tui && !pane_cmd_is_incidental_pager(cmd, command)
}

/// Send `q` to dismiss an incidental `less`/`more` pager.
pub fn dismiss_pager(state: &AppState, terminal_id: Uuid) -> Result<()> {
    state.terminals.write(terminal_id, "q")
}

/// Clear tmux scrollback, attach buffer, and visible shell before an interactive notebook command.
pub fn clear_terminal_for_interactive_session(state: &AppState, term_id: Uuid) -> Result<()> {
    if let Some(target) = state.terminals.tmux_target(term_id) {
        bunny_pty::tmux::clear_pane_history(&target);
    }
    state.terminals.clear_live_buffer(term_id);
    state.terminals.write(term_id, "clear\r")?;
    Ok(())
}

/// Full-screen or alternate-screen programs that need a real TTY (not capture-pane text).
pub fn is_interactive_tui_command(command: &str) -> bool {
    let base = command_base_name(command);
    if base.is_empty() {
        return false;
    }
    const TUI: &[&str] = &[
        "nvim", "vim", "vi", "view", "nano", "micro", "emacs", "emacsclient",
        "htop", "top", "btop", "bashtop", "glances",
        "less", "more", "most", "man",
        "apt", "apt-get", "dpkg", "dpkg-reconfigure",
        "dialog", "whiptail", "nmtui", "alsamixer",
        "mysql", "mariadb", "psql", "sqlite3",
        "mc", "ranger", "nnn", "lf", "vifm",
        "tig", "lazygit", "gitui",
        "claude", "aider",
        "ipython", "bpython",
    ];
    TUI.iter().any(|name| base == *name || base.ends_with(name))
}

fn command_has_non_interactive_flags(lower: &str) -> bool {
    lower.contains(" -y ")
        || lower.ends_with(" -y")
        || lower.contains(" --yes")
        || lower.contains(" --defaults")
        || lower.contains(" --default")
        || lower.contains("ci=true")
        || lower.contains("ci=1")
}

/// git subcommands that read answers from stdin (patch mode, editor, etc.).
pub fn git_command_expects_interactive(command: &str) -> bool {
    let lower = command.trim().to_lowercase();
    let first = command
        .split_whitespace()
        .next()
        .map(command_base_name)
        .unwrap_or_default();
    if first != "git" {
        return false;
    }
    lower.contains(" add -p")
        || lower.contains(" add --patch")
        || lower.contains(" add -i")
        || lower.contains(" add --interactive")
        || lower.contains(" stash -p")
        || lower.contains(" stash --patch")
        || lower.contains(" rebase -i")
        || lower.contains(" rebase --interactive")
        || lower.contains(" am -i")
        || lower.contains(" am --interactive")
        || (lower.contains(" commit")
            && !lower.contains(" -m ")
            && !lower.contains(" --message=")
            && !lower.contains(" --message "))
}

/// pip uninstall/remove prompts for confirmation unless -y/--yes is passed.
pub fn pip_command_expects_interactive(command: &str) -> bool {
    let lower = command.trim().to_lowercase();
    let first = command
        .split_whitespace()
        .next()
        .map(command_base_name)
        .unwrap_or_default();
    if first != "pip" && first != "pip3" {
        return false;
    }
    if command_has_non_interactive_flags(&lower) {
        return false;
    }
    lower.contains(" uninstall") || lower.contains(" remove")
}

/// Suggest a non-interactive pip uninstall/remove for Discord (`-y`).
pub fn suggest_noninteractive_pip_uninstall(command: &str) -> Option<String> {
    if !pip_command_expects_interactive(command) {
        return None;
    }
    let lower = command.to_lowercase();
    for kw in [" uninstall ", " remove "] {
        if let Some(pos) = lower.find(kw) {
            let insert_at = pos + kw.len();
            let mut out = String::new();
            out.push_str(&command[..insert_at]);
            out.push_str("-y ");
            out.push_str(command[insert_at..].trim_start());
            return Some(out);
        }
    }
    None
}

/// Structured i18n error for Discord `/bunny run` preflight (localized on the bridge).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordShellPreflightError {
    pub key: &'static str,
    pub args: Vec<(String, String)>,
}

impl DiscordShellPreflightError {
    pub fn to_api_error(&self) -> crate::api::ApiError {
        let args: Vec<(&str, &str)> = self
            .args
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        crate::api::ApiError::validation_i18n(self.key, &args)
    }

    pub fn localized_message(&self, locale: Locale) -> String {
        let args: Vec<(&str, &str)> = self
            .args
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        bunny_i18n::t(locale, self.key, &args)
    }
}

/// User-facing hint when a Discord shell run is waiting for stdin (y/n, editor, etc.).
pub fn discord_shell_interactive_hint(locale: Locale, command: &str) -> String {
    if let Err(err) = discord_shell_run_preflight(command) {
        return err.localized_message(locale);
    }
    bunny_i18n::t(locale, "discord.run.interactive_generic", &[])
}

/// Reject Discord shell runs that cannot complete without a TTY or stdin.
pub fn discord_shell_run_preflight(command: &str) -> Result<(), DiscordShellPreflightError> {
    if bunny_discord::risk::is_interactive_discord_command(command) {
        return Err(DiscordShellPreflightError {
            key: "discord.run.interactive_tui_blocked",
            args: vec![],
        });
    }
    if user_command_expects_interactive(command) {
        if let Some(suggested) = suggest_noninteractive_pip_uninstall(command) {
            return Err(DiscordShellPreflightError {
                key: "discord.run.interactive_pip_uninstall",
                args: vec![("suggested".into(), suggested)],
            });
        }
        if git_command_expects_interactive(command) {
            return Err(DiscordShellPreflightError {
                key: "discord.run.interactive_git",
                args: vec![],
            });
        }
        return Err(DiscordShellPreflightError {
            key: "discord.run.interactive_generic",
            args: vec![],
        });
    }
    Ok(())
}

/// True when tmux capture shows a shell prompt waiting for y/n (or similar) on stdin.
pub fn pane_text_awaits_user_input(text: &str) -> bool {
    text.lines().rev().take(10).any(|line| {
        let t = line.trim().to_lowercase();
        t.contains("(y/n)")
            || t.contains("[y/n]")
            || t.contains("yes/no")
            || t.contains("proceed (y/n)")
            || (t.ends_with('?')
                && (t.contains("proceed") || t.contains("continue") || t.contains("confirm")))
    })
}

/// npm/yarn/pnpm/npx/bunx and common scaffolders that prompt on stdin.
pub fn package_runner_expects_interactive(command: &str) -> bool {
    let lower = command.to_lowercase();
    if command_has_non_interactive_flags(&lower) {
        return false;
    }

    if lower.contains("create-next-app")
        || lower.contains("create-react-app")
        || lower.contains("create-vite")
        || lower.contains("create-remix")
        || lower.contains("create-svelte")
        || lower.contains("create-t3-app")
        || lower.contains("sv create")
    {
        return true;
    }

    let mut parts = command.split_whitespace();
    let first = command_base_name(parts.next().unwrap_or(""));

    match first.as_str() {
        "npx" | "bunx" => true,
        "npm" | "yarn" | "pnpm" | "bun" => {
            lower.contains(" init")
                || lower.contains(" create")
                || lower.contains("create-")
                || lower.contains(" exec")
        }
        _ => false,
    }
}

/// Foreground process name suggests the user's command needs a real TTY.
pub fn pane_process_suggests_interactive(pane_cmd: &str, user_command: &str) -> bool {
    let base = command_base_name(pane_cmd);
    if base == "git" {
        return git_command_expects_interactive(user_command);
    }
    if base == "node" {
        let lower = user_command.to_lowercase();
        return lower.contains("npx")
            || lower.contains("bunx")
            || lower.contains("create-")
            || package_runner_expects_interactive(user_command);
    }
    false
}

/// True when a user-typed command is likely to need interactive TTY (first token only).
pub fn user_command_expects_interactive(command: &str) -> bool {
    let lower = command.to_lowercase();
    let mut parts = command.split_whitespace();
    let first = parts.next().unwrap_or("");
    let base = command_base_name(first);

    if base == "apt" || base == "apt-get" {
        if lower.contains(" install")
            || lower.contains(" update")
            || lower.contains(" upgrade")
            || lower.contains(" remove")
            || lower.contains(" purge")
            || lower.contains(" autoremove")
            || lower.contains(" -y")
            || lower.contains(" --yes")
        {
            return false;
        }
        return true;
    }

    if package_runner_expects_interactive(command) {
        return true;
    }

    if git_command_expects_interactive(command) {
        return true;
    }

    if pip_command_expects_interactive(command) {
        return true;
    }

    if !is_interactive_tui_command(first) {
        if matches!(base.as_str(), "python" | "python3" | "node" | "ruby" | "irb") {
            return parts.next().is_none();
        }
        return false;
    }
    true
}

/// Like [`user_command_expects_interactive`] but tuned for notebook shells.
pub fn notebook_user_command_expects_interactive(command: &str) -> bool {
    if !user_command_expects_interactive(command) {
        return false;
    }
    let lower = command.trim().to_lowercase();
    // Bare `git commit` fails immediately when nothing is staged — capture the message
    // instead of clearing the pane and opening fullscreen.
    if lower.starts_with("git ")
        && lower.contains(" commit")
        && !lower.contains(" -m ")
        && !lower.contains(" --message=")
        && !lower.contains(" --message ")
    {
        return false;
    }
    true
}

/// Current foreground command in the tmux pane, if any.
pub fn terminal_pane_current_command(state: &AppState, term_id: Uuid) -> Option<String> {
    if !state.terminals.uses_tmux() {
        return None;
    }
    ensure_terminal_attached(state, term_id).ok()?;
    let auth_db = state.auth.db();
    let row = auth_db.lock().get_terminal(term_id).ok().flatten()?;
    let record = row_to_record(row);
    let target = resolve_tmux_target(state, &record).ok()??;
    if tmux::pane_is_dead(&target) {
        return None;
    }
    tmux::pane_current_command(&target)
}

/// True when the tmux pane is running a foreground process (dev server, etc.), not an idle shell prompt.
pub fn discord_shell_pane_busy(state: &AppState, term_id: Uuid) -> Result<bool> {
    if !state.terminals.uses_tmux() {
        return Ok(false);
    }
    ensure_terminal_attached(state, term_id)?;
    let auth_db = state.auth.db();
    let row = auth_db
        .lock()
        .get_terminal(term_id)?
        .ok_or_else(|| anyhow::anyhow!("terminal not found"))?;
    let record = row_to_record(row);
    let Some(target) = resolve_tmux_target(state, &record)? else {
        return Ok(false);
    };
    if tmux::pane_is_dead(&target) {
        return Ok(false);
    }
    if let Some(cmd) = tmux::pane_current_command(&target) {
        if !discord_shell_idle_command_name(&cmd) {
            return Ok(true);
        }
    }
    if tmux::pane_has_non_shell_child(&target) {
        return Ok(true);
    }
    Ok(false)
}

/// Send SIGINT to the shell (Ctrl+C). Uses tmux send-keys when attached — more reliable than PTY attach.
pub fn send_terminal_interrupt(state: &AppState, terminal_id: Uuid) -> Result<()> {
    if let Some(target) = state.terminals.tmux_target(terminal_id) {
        tmux::send_keys_key(&target, "C-c")?;
        return Ok(());
    }
    state.terminals.write(terminal_id, "\x03")
}

/// Working directory for Discord commands — tmux pane cwd when available.
pub fn discord_shell_working_directory(
    state: &AppState,
    term_id: Uuid,
) -> Result<PathBuf> {
    let row = state
        .auth
        .db()
        .lock()
        .get_terminal(term_id)?
        .ok_or_else(|| anyhow::anyhow!("terminal not found"))?;
    let record = row_to_record(row);

    if state.terminals.uses_tmux() {
        if let Some(target) = resolve_tmux_target(state, &record)? {
            if let Some(cwd) = tmux::pane_cwd(&target) {
                return Ok(PathBuf::from(cwd));
            }
        }
    }
    Ok(PathBuf::from(record.cwd))
}

/// Best-effort cwd for a shell (live tmux pane → scrollback sidecar → DB).
pub fn terminal_live_cwd(state: &AppState, term_id: Uuid) -> Option<PathBuf> {
    let record = state
        .auth
        .db()
        .lock()
        .get_terminal(term_id)
        .ok()
        .flatten()
        .map(row_to_record);

    if let Some(record) = record {
        if let Some(target) = live_tmux_target_readonly(state, &record) {
            if let Some(cwd) = tmux::pane_cwd(&target) {
                return Some(PathBuf::from(cwd));
            }
        }
        if let Some(cwd) = scrollback::load_cwd(&scrollback_dir(state), term_id) {
            return Some(PathBuf::from(cwd));
        }
        return Some(PathBuf::from(record.cwd));
    }
    if let Some(target) = state.terminals.tmux_target(term_id) {
        if let Some(cwd) = tmux::pane_cwd(&target) {
            return Some(PathBuf::from(cwd));
        }
    }
    scrollback::load_cwd(&scrollback_dir(state), term_id).map(PathBuf::from)
}

#[derive(Debug, Clone)]
pub struct TerminalWorkContext {
    pub cwd: Option<String>,
    pub git_project: Option<String>,
    pub git_branch: Option<String>,
}

const TERMINAL_CONTEXT_TTL: Duration = Duration::from_secs(3);

/// Cached cwd/git context for list/poll endpoints (no tmux subprocess on hot path).
pub fn terminal_work_context_for_list(state: &AppState, term_id: Uuid) -> TerminalWorkContext {
    let now = Instant::now();
    if let Some(hit) = state.terminal_context_cache.lock().get(&term_id).cloned() {
        if hit.expires_at > now {
            return TerminalWorkContext {
                cwd: hit.cwd,
                git_project: hit.git_project,
                git_branch: hit.git_branch,
            };
        }
    }
    let ctx = terminal_work_context_light(state, term_id);
    state.terminal_context_cache.lock().insert(
        term_id,
        crate::state::TerminalContextCacheEntry {
            cwd: ctx.cwd.clone(),
            git_project: ctx.git_project.clone(),
            git_branch: ctx.git_branch.clone(),
            expires_at: now + TERMINAL_CONTEXT_TTL,
        },
    );
    ctx
}

pub fn update_terminal_context_cache(state: &AppState, term_id: Uuid, ctx: &TerminalWorkContext) {
    state.terminal_context_cache.lock().insert(
        term_id,
        crate::state::TerminalContextCacheEntry {
            cwd: ctx.cwd.clone(),
            git_project: ctx.git_project.clone(),
            git_branch: ctx.git_branch.clone(),
            expires_at: Instant::now() + TERMINAL_CONTEXT_TTL,
        },
    );
}

pub fn terminal_work_context_light(state: &AppState, term_id: Uuid) -> TerminalWorkContext {
    let Some(cwd) = terminal_live_cwd_light(state, term_id) else {
        return TerminalWorkContext {
            cwd: None,
            git_project: None,
            git_branch: None,
        };
    };
    let git = crate::discord_git::terminal_git_context(&cwd);
    TerminalWorkContext {
        cwd: Some(cwd.to_string_lossy().into_owned()),
        git_project: git.project,
        git_branch: git.branch,
    }
}

fn terminal_live_cwd_light(state: &AppState, term_id: Uuid) -> Option<PathBuf> {
    if let Some(cwd) = scrollback::load_cwd(&scrollback_dir(state), term_id) {
        return Some(PathBuf::from(cwd));
    }
    let record = state
        .auth
        .db()
        .lock()
        .get_terminal(term_id)
        .ok()
        .flatten()
        .map(row_to_record)?;
    Some(PathBuf::from(record.cwd))
}

pub fn terminal_work_context(state: &AppState, term_id: Uuid) -> TerminalWorkContext {
    let Some(cwd) = terminal_live_cwd(state, term_id) else {
        return TerminalWorkContext {
            cwd: None,
            git_project: None,
            git_branch: None,
        };
    };
    let git = crate::discord_git::terminal_git_context(&cwd);
    TerminalWorkContext {
        cwd: Some(cwd.to_string_lossy().into_owned()),
        git_project: git.project,
        git_branch: git.branch,
    }
}

/// Run a command for Discord via the shell tmux pane (generic: quick finish or persistent process).
pub fn exec_discord_shell_command_run(
    state: &AppState,
    term_id: Uuid,
    session_id: Uuid,
    command: &str,
    acting_user_id: Option<Uuid>,
    locale: Locale,
) -> Result<DiscordShellRunResult> {
    ensure_terminal_attached(state, term_id)?;
    prepare_discord_tmux_git_actor(state, term_id, session_id, acting_user_id)?;
    let auth_db = state.auth.db();
    let row = auth_db
        .lock()
        .get_terminal(term_id)?
        .ok_or_else(|| anyhow::anyhow!("terminal not found"))?;
    let record = row_to_record(row);

    // Run in the live tmux shell so Discord commands inherit cwd, exports, venv, etc.
    // Subprocess fallback only when no tmux target is available.
    let target = match resolve_tmux_target(state, &record)? {
        Some(t) => t,
        None => {
            let (text, code) = exec_discord_shell_command_timed(
                state,
                term_id,
                session_id,
                command,
                std::time::Duration::from_secs(40),
                None,
                acting_user_id,
            )?;
            return Ok(DiscordShellRunResult {
                output: text,
                exit_code: code,
                persistent: false,
            });
        }
    };

    // Visible pane only — full scrollback breaks prefix deltas after prior runs.
    let baseline = capture_pane_visible_text(&target).unwrap_or_default();
    let wrapped = discord_run_wrap_command(command, true);
    tmux::send_keys_literal(&target, &wrapped, true)?;
    std::thread::sleep(std::time::Duration::from_millis(350));

    let started = std::time::Instant::now();
    let mut last_delta = String::new();
    let mut last_cap = String::new();
    while started.elapsed() < discord_run_quick_wait(command) {
        let Ok(cap) = capture_pane_visible_text(&target) else {
            std::thread::sleep(std::time::Duration::from_millis(200));
            continue;
        };
        last_cap = cap.clone();
        // Compare against pre-command baseline (not post-send snapshot): fast commands
        // like `ls` often finish before the first poll, so the exit marker is already
        // in the pane while the delta vs after_send would stay empty.
        let since = pane_text_delta_for_discord_run(&baseline, &cap, command, &wrapped);
        if !since.is_empty() {
            last_delta = since.clone();
        }
        if let Some((output, code)) =
            discord_try_parse_finished_command(&baseline, &cap, command, &wrapped, &since)
        {
            let text = discord_output_or_shell_state_message(
                state,
                term_id,
                command,
                &cap,
                finalize_discord_run_excerpt(command, &output),
            );
            append_discord_transcript(
                state,
                term_id,
                command,
                discord_transcript_body_for_run_output(&text),
                acting_user_id,
            );
            return Ok(DiscordShellRunResult {
                output: text,
                exit_code: code,
                persistent: false,
            });
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    // One last capture — fast commands can finish right as the quick-wait window closes.
    std::thread::sleep(std::time::Duration::from_millis(600));
    if let Ok(cap) = capture_pane_visible_text(&target) {
        last_cap = cap.clone();
        let since = pane_text_delta_for_discord_run(&baseline, &cap, command, &wrapped);
        if !since.is_empty() {
            last_delta = since.clone();
        }
        if let Some((output, code)) =
            discord_try_parse_finished_command(&baseline, &cap, command, &wrapped, &since)
        {
            let text = discord_output_or_shell_state_message(
                state,
                term_id,
                command,
                &cap,
                finalize_discord_run_excerpt(command, &output),
            );
            append_discord_transcript(
                state,
                term_id,
                command,
                discord_transcript_body_for_run_output(&text),
                acting_user_id,
            );
            return Ok(DiscordShellRunResult {
                output: text,
                exit_code: code,
                persistent: false,
            });
        }
    }

    if let Some((output, code)) =
        discord_try_parse_finished_command(&baseline, &last_cap, command, &wrapped, &last_delta)
    {
        let text = discord_output_or_shell_state_message(
            state,
            term_id,
            command,
            &last_cap,
            finalize_discord_run_excerpt(command, &output),
        );
        append_discord_transcript(
            state,
            term_id,
            command,
            discord_transcript_body_for_run_output(&text),
            acting_user_id,
        );
        return Ok(DiscordShellRunResult {
            output: text,
            exit_code: code,
            persistent: false,
        });
    }

    if let Some(text) =
        crate::blocks::shell_state_success_message(state, term_id, command, &last_cap)
    {
        append_discord_transcript(
            state,
            term_id,
            command,
            DiscordTranscriptBody::Output(&text),
            acting_user_id,
        );
        return Ok(DiscordShellRunResult {
            output: text,
            exit_code: 0,
            persistent: false,
        });
    }

    let excerpt = finalize_discord_run_excerpt(command, &last_delta);
    if pane_text_awaits_user_input(&last_cap) || pane_text_awaits_user_input(&excerpt) {
        let hint = discord_shell_interactive_hint(locale, command);
        append_discord_transcript(
            state,
            term_id,
            command,
            DiscordTranscriptBody::Output(&hint),
            acting_user_id,
        );
        return Ok(DiscordShellRunResult {
            output: hint,
            exit_code: 1,
            persistent: false,
        });
    }
    append_discord_transcript(
        state,
        term_id,
        command,
        DiscordTranscriptBody::Output(&excerpt),
        acting_user_id,
    );
    Ok(DiscordShellRunResult {
        output: excerpt,
        exit_code: 0,
        persistent: true,
    })
}

/// Shell builtins that must run in the interactive tmux pane (not a `bash -lc` subshell).
fn discord_run_needs_interactive_shell(command: &str) -> bool {
    let cmd = command.trim();
    if cmd.is_empty() || cmd.starts_with('(') || cmd.starts_with('{') {
        return false;
    }
    const BUILTINS: &[&str] = &["cd", "export", "unset", "source", "deactivate", "pushd", "popd"];
    for b in BUILTINS {
        if cmd == *b || cmd.starts_with(&format!("{b} ")) || cmd.starts_with(&format!("{b}\t")) {
            return true;
        }
    }
    cmd == "." || cmd.starts_with(". ") || cmd.starts_with(".\t")
}

fn discord_output_or_shell_state_message(
    state: &AppState,
    term_id: Uuid,
    command: &str,
    cap: &str,
    sanitized: String,
) -> String {
    if !discord_display_output_is_empty(&sanitized) {
        return sanitized;
    }
    crate::blocks::shell_state_success_message(state, term_id, command, cap)
        .unwrap_or_else(|| "(no output)".to_string())
}

pub(crate) fn discord_display_output_is_empty(output: &str) -> bool {
    output.trim().is_empty() || output.trim() == "(no output)"
}

fn discord_transcript_body_for_run_output(text: &str) -> DiscordTranscriptBody<'_> {
    if text.trim().is_empty() || text == "(no output)" {
        DiscordTranscriptBody::CommandOnly
    } else {
        DiscordTranscriptBody::Output(text)
    }
}

/// Delta since baseline for Discord `/bunny run`, with fallbacks when prompt changes break prefix matching.
fn pane_text_delta_for_discord_run(
    before: &str,
    after: &str,
    command: &str,
    wrapped: &str,
) -> String {
    for cmd in [command, wrapped] {
        let delta = pane_text_delta(before, after, Some(cmd));
        if split_on_exit_marker(&delta).is_some() {
            return delta;
        }
    }
    let since = pane_text_since_baseline(before, after);
    if split_on_exit_marker(&since).is_some() {
        return since;
    }
    let after_norm = normalize_pane_text(after);
    if let Some(marker_idx) = after_norm.rfind(BUNNY_EXIT_MARKER) {
        if exit_marker_is_new(before, after, marker_idx) {
            let new_suffix = pane_text_new_suffix(before, after);
            if let Some(rel) = new_suffix.rfind(BUNNY_EXIT_MARKER) {
                let marker_line = new_suffix[rel..].lines().next().unwrap_or("");
                let rest = marker_line.trim().strip_prefix(BUNNY_EXIT_MARKER);
                if rest.is_some_and(|r| r.trim().parse::<i32>().is_ok()) {
                    return new_suffix[..rel + marker_line.len()].trim_end().to_string();
                }
            }
        }
    }
    for cmd in [command, wrapped] {
        let delta = pane_text_delta(before, after, Some(cmd));
        if !delta.is_empty() {
            return delta;
        }
    }
    since
}

fn discord_run_wrap_command(command: &str, in_tmux_shell: bool) -> String {
    if discord_run_needs_interactive_shell(command) {
        return format!("{}; echo {BUNNY_EXIT_MARKER}$?", command.trim());
    }
    if in_tmux_shell {
        return tmux_pager_wrap(command);
    }
    format!(
        "bash --noprofile --norc -c {}; echo {BUNNY_EXIT_MARKER}$?",
        shell_single_quote(command)
    )
}

fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn wrap_long_running_discord_command(command: &str) -> String {
    format!(
        "log=\"/tmp/bunny-discord-bg-$$.log\"; \
         nohup bash -lc {} >\"$log\" 2>&1 & pid=$!; \
         sleep 1; \
         if kill -0 \"$pid\" 2>/dev/null; then \
           echo \"{BUNNY_BACKGROUND_PID_MARKER}$pid\"; \
           exit 0; \
         else \
           cat \"$log\" 2>/dev/null; \
           wait \"$pid\"; \
           exit $?; \
         fi",
        shell_single_quote(command)
    )
}

fn capture_pane_text(target: &str) -> Result<String> {
    tmux::capture_pane(target).map(|s| strip_ansi_escapes(&s))
}

fn capture_pane_visible_text(target: &str) -> Result<String> {
    tmux::capture_pane_visible(target).map(|s| strip_ansi_escapes(&s))
}

pub fn capture_interactive_tty_snapshot(state: &AppState, term_id: Uuid) -> String {
    let visible = capture_pane_visible_for_terminal(state, term_id).unwrap_or_default();
    if !visible.trim().is_empty() {
        return visible;
    }
    let full = capture_pane_for_terminal(state, term_id).unwrap_or_default();
    let lines: Vec<&str> = full.lines().collect();
    let start = lines.len().saturating_sub(48);
    lines[start..].join("\n")
}

/// Active `VIRTUAL_ENV` from the live tmux shell, if any.
pub fn terminal_active_virtual_env(state: &AppState, term_id: Uuid) -> Option<String> {
    if !state.terminals.uses_tmux() {
        return None;
    }
    let _ = ensure_terminal_attached(state, term_id).ok()?;
    let target = terminal_tmux_target(state, term_id)?;

    if let Some(venv) = read_pane_env_var(&target, "VIRTUAL_ENV") {
        if virtual_env_has_activate(&venv) {
            return Some(venv);
        }
    }

    if let Some(venv) = virtual_env_from_pane_path(&target) {
        return Some(venv);
    }

    if pane_shows_virtualenv_prompt(state, term_id) {
        if let Some(cwd) = bunny_pty::tmux::pane_cwd(&target) {
            let cwd = std::path::PathBuf::from(cwd);
            if virtual_env_has_activate(&cwd.to_string_lossy()) {
                return Some(cwd.to_string_lossy().into_owned());
            }
        }
    }

    None
}

fn terminal_tmux_target(state: &AppState, term_id: Uuid) -> Option<String> {
    if let Some(target) = state.terminals.tmux_target(term_id) {
        if bunny_pty::tmux::target_alive(&target) {
            return Some(target);
        }
    }
    let row = state.auth.db().lock().get_terminal(term_id).ok()??;
    let record = row_to_record(row);
    resolve_tmux_target(state, &record).ok()?
}

fn read_pane_env_var(target: &str, key: &str) -> Option<String> {
    let value = bunny_pty::tmux::pane_shell_env_var(target, key)?;
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    Some(value.to_string())
}

fn virtual_env_has_activate(venv: &str) -> bool {
    std::path::Path::new(venv)
        .join("bin/activate")
        .is_file()
}

fn virtual_env_from_pane_path(target: &str) -> Option<String> {
    let path = read_pane_env_var(target, "PATH")?;
    for segment in path.split(':') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        let bin = std::path::Path::new(segment);
        if bin.file_name().and_then(|s| s.to_str()) != Some("bin") {
            continue;
        }
        let venv = bin.parent()?;
        if virtual_env_has_activate(&venv.to_string_lossy()) {
            return Some(venv.to_string_lossy().into_owned());
        }
    }
    None
}

fn pane_shows_virtualenv_prompt(state: &AppState, term_id: Uuid) -> bool {
    let cap = capture_pane_visible_for_terminal(state, term_id).unwrap_or_default();
    pane_text_shows_virtualenv_prefix(&cap)
}

fn pane_text_shows_virtualenv_prefix(cap: &str) -> bool {
    cap.lines().rev().any(|line| {
        let t = line.trim();
        let (Some(open), Some(close)) = (t.find('('), t.find(')')) else {
            return false;
        };
        if close <= open + 1 {
            return false;
        }
        let rest = t[close + 1..].trim();
        rest.is_empty()
            || rest.ends_with('$')
            || rest.ends_with('#')
            || rest.ends_with('%')
            || rest.contains('$')
            || rest.contains('#')
    })
}

/// Prefix a command with `source …/bin/activate` when a venv is active.
pub fn wrap_command_with_virtualenv(venv: &str, command: &str) -> Option<String> {
    if notebook_shell_state_command(command) {
        return None;
    }
    let activate = std::path::Path::new(venv).join("bin/activate");
    if !activate.is_file() {
        return None;
    }
    Some(format!(
        "source {} && {}",
        shell_single_quote(&activate.to_string_lossy()),
        command
    ))
}

fn discord_subprocess_command(
    state: &AppState,
    term_id: Uuid,
    command: &str,
) -> String {
    terminal_active_virtual_env(state, term_id)
        .and_then(|venv| wrap_command_with_virtualenv(&venv, command))
        .unwrap_or_else(|| command.to_string())
}

/// Virtualenv prefix from the live shell, e.g. `(test-venv) `.
pub fn terminal_shell_prompt_prefix(state: &AppState, term_id: Uuid) -> String {
    let _ = ensure_terminal_attached(state, term_id);
    if let Some(target) = state.terminals.tmux_target(term_id) {
        if let Some(venv) = bunny_pty::tmux::pane_shell_env_var(&target, "VIRTUAL_ENV") {
            let venv = venv.trim();
            if !venv.is_empty() {
                if let Some(name) = std::path::Path::new(venv)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .filter(|s| !s.is_empty())
                {
                    return format!("({name}) ");
                }
            }
        }
    }
    let cap = match capture_pane_visible_for_terminal(state, term_id) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    let prompt = cap.lines().rev().find(|line| {
        let t = line.trim();
        t.contains('@') && (t.ends_with('#') || t.ends_with('$') || t.ends_with("%"))
    });
    let Some(prompt) = prompt else {
        return String::new();
    };
    let t = prompt.trim();
    let (Some(open), Some(close)) = (t.find('('), t.find(')')) else {
        return String::new();
    };
    if close <= open + 1 {
        return String::new();
    }
    format!("{} ", &t[open..=close])
}

/// Capture tmux pane text for a terminal (used by block output collector).
pub fn capture_pane_for_terminal(state: &AppState, term_id: Uuid) -> Result<String> {
    let auth_db = state.auth.db();
    let row = auth_db
        .lock()
        .get_terminal(term_id)?
        .ok_or_else(|| anyhow::anyhow!("terminal not found"))?;
    let record = row_to_record(row);
    let target = resolve_tmux_target(state, &record)?
        .ok_or_else(|| anyhow::anyhow!("no tmux target"))?;
    capture_pane_text(&target)
}

/// Visible pane only — avoids dumping full tmux scrollback after interactive sessions.
pub fn capture_pane_visible_for_terminal(state: &AppState, term_id: Uuid) -> Result<String> {
    let auth_db = state.auth.db();
    let row = auth_db
        .lock()
        .get_terminal(term_id)?
        .ok_or_else(|| anyhow::anyhow!("terminal not found"))?;
    let record = row_to_record(row);
    let target = resolve_tmux_target(state, &record)?
        .ok_or_else(|| anyhow::anyhow!("no tmux target"))?;
    capture_pane_visible_text(&target)
}

fn normalize_pane_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Text appended to a tmux pane since `before` was captured.
/// Falls back to prompt/command anchoring when resize or scroll breaks prefix matching.
pub fn pane_text_since_baseline(before: &str, after: &str) -> String {
    pane_text_delta(before, after, None)
}

/// Like [`pane_text_since_baseline`] but can anchor on a submitted shell command.
pub fn pane_text_delta(before: &str, after: &str, command: Option<&str>) -> String {
    let before = normalize_pane_text(before);
    let after = normalize_pane_text(after);

    if before.is_empty() {
        return after.trim().to_string();
    }
    if after.is_empty() {
        return String::new();
    }
    if after.starts_with(&before) {
        return after[before.len()..].trim().to_string();
    }

    let b: Vec<char> = before.chars().collect();
    let a: Vec<char> = after.chars().collect();
    let mut i = 0;
    while i < b.len() && i < a.len() && b[i] == a[i] {
        i += 1;
    }

    let strong = i == b.len() || (i * 100 / b.len().max(1)) >= 80;

    if let Some(cmd) = command {
        if let Some(delta) = pane_text_after_command_echo(&after, cmd) {
            return strip_lines_in_baseline(&delta, &before);
        }
    }

    if let Some(delta) = pane_text_after_prompt_anchor(&before, &after) {
        return strip_lines_in_baseline(&delta, &before);
    }

    if strong {
        return a[i..].iter().collect::<String>().trim().to_string();
    }

    String::new()
}

fn strip_lines_in_baseline(delta: &str, before: &str) -> String {
    let baseline: std::collections::HashSet<&str> = before.lines().map(str::trim).collect();
    let is_content_line = |t: &str| {
        !t.is_empty() && !t.starts_with(BUNNY_EXIT_MARKER) && !is_shell_prompt_line(t)
    };
    let has_new_content = delta.lines().any(|l| {
        let t = l.trim();
        is_content_line(t) && !baseline.contains(t)
    });
    let filtered: String = delta
        .lines()
        .filter(|l| {
            let t = l.trim();
            if t.is_empty() {
                return false;
            }
            if t.starts_with(BUNNY_EXIT_MARKER) {
                return true;
            }
            if is_shell_prompt_line(t) {
                return false;
            }
            // Repeated command output (e.g. two `ls` in the same dir) reuses lines
            // already present in the baseline — keep them unless newer lines exist too.
            if !has_new_content {
                return is_content_line(t) || !baseline.contains(t);
            }
            !baseline.contains(t)
        })
        .collect::<Vec<_>>()
        .join("\n");
    if filtered.trim().is_empty() && !delta.trim().is_empty() {
        return delta.trim().to_string();
    }
    filtered
}

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

/// Extract output lines following the last echo of `command` until the next shell prompt.
pub fn extract_command_output_from_pane(cap: &str, command: &str) -> String {
    let cap = normalize_pane_text(cap);
    let lines: Vec<&str> = cap.lines().collect();
    let mut cmd_idx = None;
    for (idx, line) in lines.iter().enumerate() {
        if line_echoes_command(line, command) {
            cmd_idx = Some(idx);
        }
    }
    let Some(start) = cmd_idx else {
        return String::new();
    };
    let mut out: Vec<&str> = Vec::new();
    for line in &lines[start + 1..] {
        if is_shell_prompt_line(line) {
            break;
        }
        out.push(line);
    }
    out.join("\n").trim().to_string()
}

fn pane_text_after_prompt_anchor(before: &str, after: &str) -> Option<String> {
    let anchor = before.lines().rev().find(|l| !l.trim().is_empty())?;
    let anchor = anchor.trim_end();
    if anchor.is_empty() {
        return None;
    }
    let pos = after.rfind(anchor)?;
    let rest = after[pos + anchor.len()..]
        .trim_start_matches(['\n', '\r'])
        .to_string();
    Some(rest)
}

fn command_echo_variants(command: &str) -> Vec<String> {
    let mut variants = vec![command.to_string()];
    let safe = pager_safe_command(command);
    if safe != command {
        variants.push(safe);
    }
    if let Some(body) = command.split("; echo ").next() {
        let body = body.trim().to_string();
        if !body.is_empty() && body != command && !variants.contains(&body) {
            variants.push(body);
        }
    }
    variants
}

fn line_echoes_command(line: &str, command: &str) -> bool {
    for variant in command_echo_variants(command) {
        if line_echoes_command_variant(line, &variant) {
            return true;
        }
    }
    false
}

fn git_command_echo_tail(command: &str) -> Option<String> {
    let mut cmd = command.trim();
    cmd = cmd.split("; echo ").next().unwrap_or(cmd).trim();
    if let Some(inner) = cmd.strip_prefix('(').and_then(|s| s.split(')').next()) {
        cmd = inner.trim();
    }
    let mut body = cmd;
    if let Some(rest) = body.strip_prefix("PAGER=cat GIT_PAGER=cat ") {
        body = rest;
    }
    body = body.strip_suffix(" 2>&1").unwrap_or(body).trim();
    if let Some(rest) = body.strip_prefix("git --no-pager ") {
        return Some(rest.to_string());
    }
    if let Some(rest) = body.strip_prefix("git ") {
        return Some(rest.to_string());
    }
    None
}

/// Tmux often clips long echoed lines — match when the pane line contains a git prefix of `tail`.
fn line_echoes_git_tail(line: &str, tail: &str) -> bool {
    if !line.contains('#') && !line.contains('@') {
        return false;
    }
    for git_prefix in ["git --no-pager ", "git "] {
        let full = format!("{git_prefix}{tail}");
        if line.contains(&full) {
            return true;
        }
        if let Some(pos) = line.find(git_prefix) {
            let after = line[pos + git_prefix.len()..].trim_end();
            if !after.is_empty() && tail.starts_with(after) && after.len() >= 3 {
                return true;
            }
        }
    }
    false
}

fn line_echoes_command_variant(line: &str, command: &str) -> bool {
    let t = line.trim();
    if t.is_empty() || command.is_empty() {
        return false;
    }
    if t == command {
        return true;
    }
    // Avoid `ls` matching `rails`, `tools`, or directory listings via `ends_with("ls")`.
    if command.len() > 3 && t.ends_with(command) {
        return true;
    }
    if let Some(tail) = git_command_echo_tail(command) {
        if line_echoes_git_tail(t, &tail) {
            return true;
        }
        // Git subcommands share the same tmux wrapper prefix (`PAGER=cat … git --no-pa`).
        // Do not fall through to the generic wrapper prefix or `git status` echoes
        // false-match `git commit` (and vice versa).
        return false;
    }
    if let Some(tail) = command.strip_prefix("git ") {
        if line_echoes_git_tail(t, tail) {
            return true;
        }
    }
    // Tmux truncates long echoed wrapper lines — match a stable inner prefix.
    if (t.contains('#') || t.contains('@')) && command.len() >= 24 {
        let body = command
            .trim_start_matches('(')
            .split(") 2>&1")
            .next()
            .unwrap_or(command);
        let prefix_len = body.len().min(36);
        if prefix_len >= 16 && t.contains(&body[..prefix_len]) {
            return true;
        }
    }
    if (t.contains('#') || t.contains('$') || t.contains('@')) && t.contains(command) {
        if command.len() <= 3 {
            return t.contains(&format!(" {command} "))
                || t.contains(&format!(" {command})"))
                || t.contains(&format!(" {command};"))
                || t.ends_with(&format!(" {command}"))
                || t.contains(&format!("#{command}"))
                || t.contains(&format!("${command}"));
        }
        return true;
    }
    false
}

fn pane_text_after_command_echo(after: &str, command: &str) -> Option<String> {
    let mut last_match: Option<usize> = None;
    for (idx, line) in after.lines().enumerate() {
        if line_echoes_command(line, command) {
            last_match = Some(idx);
        }
    }
    let start = last_match? + 1;
    let lines: Vec<_> = after.lines().skip(start).collect();
    Some(lines.join("\n").trim().to_string())
}

/// Output + exit code from the last command echo through a complete exit marker.
pub fn parse_captured_run_after_last_echo(
    text: &str,
    command: &str,
    exec_line: &str,
) -> Option<(String, i32)> {
    if text.trim().is_empty() {
        return None;
    }
    let mut last_match: Option<usize> = None;
    for cmd in command_echo_variants(exec_line) {
        for variant in command_echo_variants(&cmd) {
            update_last_command_echo_match(text, &variant, &mut last_match);
        }
    }
    for variant in command_echo_variants(command) {
        update_last_command_echo_match(text, &variant, &mut last_match);
    }
    let start = last_match? + 1;
    let after_echo: String = text.lines().skip(start).collect::<Vec<_>>().join("\n");
    split_on_exit_marker(&after_echo)
}

fn update_last_command_echo_match(text: &str, variant: &str, last_match: &mut Option<usize>) {
    let lines: Vec<&str> = text.lines().collect();
    for (idx, line) in lines.iter().enumerate() {
        if line_echoes_command_variant(line, variant) {
            *last_match = Some(last_match.map_or(idx, |m| m.max(idx)));
        }
        if idx + 1 < lines.len() {
            let joined = format!("{}{}", line, lines[idx + 1]);
            if line_echoes_command_variant(&joined, variant) {
                *last_match = Some(last_match.map_or(idx, |m| m.max(idx)));
            }
        }
    }
}

/// Text appended after `before` was captured (prefix match or best-effort delta).
pub fn pane_text_new_suffix(before: &str, after: &str) -> String {
    let before_norm = normalize_pane_text(before);
    let after_norm = normalize_pane_text(after);
    if after_norm.starts_with(&before_norm) {
        after_norm[before_norm.len()..].to_string()
    } else {
        pane_text_since_baseline(before, after)
    }
}

/// True when `marker_pos` points at an exit marker appended after `before` was captured.
fn exit_marker_is_new(before: &str, after: &str, marker_pos: usize) -> bool {
    let before_norm = normalize_pane_text(before);
    let after_norm = normalize_pane_text(after);
    if after_norm.starts_with(&before_norm) {
        return marker_pos >= before_norm.len();
    }
    if pane_text_since_baseline(before, after).contains(BUNNY_EXIT_MARKER) {
        return true;
    }
    // Scrollback can shift without a shared prefix — compare marker counts instead.
    let before_markers = before_norm.matches(BUNNY_EXIT_MARKER).count();
    let after_markers = after_norm[..marker_pos.saturating_add(BUNNY_EXIT_MARKER.len())]
        .matches(BUNNY_EXIT_MARKER)
        .count();
    after_markers > before_markers
}

/// Parse a finished Discord/tmux command from delta or full-pane fallback.
fn discord_try_parse_finished_command(
    baseline: &str,
    cap: &str,
    command: &str,
    wrapped: &str,
    since: &str,
) -> Option<(String, i32)> {
    let suffix = pane_text_new_suffix(baseline, cap);
    for text in [&suffix, since, cap] {
        if let Some((output, code)) = parse_captured_run_after_last_echo(text, command, wrapped) {
            return Some((strip_shell_capture_artifacts(command, &output), code));
        }
    }
    // Tmux often truncates echoed wrapper lines — fall back to the exit marker alone.
    for text in [since, &suffix, cap] {
        if let Some((output, code)) = split_on_exit_marker(text) {
            if text == cap {
                let after_norm = normalize_pane_text(cap);
                let idx = after_norm.rfind(BUNNY_EXIT_MARKER)?;
                if !exit_marker_is_new(baseline, cap, idx) {
                    continue;
                }
            }
            return Some((strip_shell_capture_artifacts(command, &output), code));
        }
    }
    for cmd in [wrapped, command] {
        let delta = pane_text_delta(baseline, cap, Some(cmd));
        if let Some((output, code)) = split_on_exit_marker(&delta) {
            return Some((strip_shell_capture_artifacts(command, &output), code));
        }
    }
    parse_finished_from_newest_exit_marker(baseline, cap, command, wrapped)
}

/// Last-resort parse when scrollback shifted but a new exit marker appeared on screen.
fn parse_finished_from_newest_exit_marker(
    baseline: &str,
    cap: &str,
    command: &str,
    wrapped: &str,
) -> Option<(String, i32)> {
    let cap_norm = normalize_pane_text(cap);
    let idx = cap_norm.rfind(BUNNY_EXIT_MARKER)?;
    if !exit_marker_is_new(baseline, cap, idx) {
        return None;
    }
    if let Some((output, code)) = parse_captured_run_after_last_echo(&cap_norm, command, wrapped) {
        return Some((strip_shell_capture_artifacts(command, &output), code));
    }
    let (output, code) = split_on_exit_marker(&cap_norm)?;
    Some((strip_shell_capture_artifacts(command, &output), code))
}

/// Capture output for fast notebook commands using the same pane delta logic as
/// non-instant commands (prefix match, command echo, baseline line stripping).
pub fn capture_instant_notebook_output(
    baseline: &str,
    cap: &str,
    command: &str,
    exec_line: &str,
) -> (String, Option<i32>) {
    let mut raw = String::new();
    for cmd in [exec_line, command] {
        let delta = pane_text_delta(baseline, cap, Some(cmd));
        if !delta.trim().is_empty() {
            raw = delta;
            break;
        }
    }
    if raw.is_empty() {
        raw = pane_text_delta(baseline, cap, None);
    }
    let (parsed, code) = notebook_parse_captured_output(&raw);
    if parsed.trim().is_empty() {
        if let Some((fallback, code)) =
            parse_finished_from_newest_exit_marker(baseline, cap, command, exec_line)
        {
            return (fallback, Some(code));
        }
    }
    (parsed, code)
}

fn split_on_exit_marker(delta: &str) -> Option<(String, i32)> {
    let idx = delta.rfind(BUNNY_EXIT_MARKER)?;
    let output = strip_shell_capture_artifacts("", &delta[..idx]);
    let code_str = delta[idx + BUNNY_EXIT_MARKER.len()..]
        .lines()
        .next()?
        .trim();
    let code: i32 = code_str.parse().ok()?;
    Some((output, code))
}

/// Parse notebook wrapper output (`__BUNNY_EXIT__` suffix) into text + exit code.
pub fn notebook_parse_captured_output(raw: &str) -> (String, Option<i32>) {
    if let Some((output, code)) = split_on_exit_marker(raw) {
        (output, Some(code))
    } else {
        (raw.to_string(), None)
    }
}

fn finalize_discord_run_excerpt(command: &str, raw: &str) -> String {
    let sanitized = sanitize_discord_terminal_excerpt(raw);
    curate_discord_install_output(command, &sanitized)
}

/// Keep pip/npm install replies short in Discord — progress bars are noisy in chat.
fn curate_discord_install_output(command: &str, output: &str) -> String {
    let text = output.trim();
    if text.is_empty() {
        return String::new();
    }
    let lower = command.to_lowercase();
    if lower.contains("pip install") || lower.contains("pip3 install") {
        return curate_pip_discord_output(text);
    }
    if lower.contains("npm install") || lower.contains("npm ci") {
        return curate_npm_discord_output(text);
    }
    text.to_string()
}

fn curate_pip_discord_output(output: &str) -> String {
    let mut highlights: Vec<String> = Vec::new();
    for line in output.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let tl = t.to_lowercase();
        if tl.contains("successfully installed")
            || tl.contains("requirement already satisfied")
            || tl.starts_with("error")
            || tl.contains("could not find a version")
            || tl.contains("no matching distribution")
            || tl.contains("permission denied")
            || tl.contains("externally-managed-environment")
        {
            highlights.push(t.to_string());
        }
    }
    if !highlights.is_empty() {
        return highlights.join("\n");
    }
    tail_non_empty_lines(output, 6)
}

fn curate_npm_discord_output(output: &str) -> String {
    let mut highlights: Vec<String> = Vec::new();
    for line in output.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let tl = t.to_lowercase();
        if tl.starts_with("added ")
            || tl.starts_with("up to date")
            || tl.contains("packages are looking for funding")
            || tl.starts_with("npm err")
            || tl.contains("error code")
        {
            highlights.push(t.to_string());
        }
    }
    if !highlights.is_empty() {
        return highlights.join("\n");
    }
    tail_non_empty_lines(output, 6)
}

fn tail_non_empty_lines(output: &str, max: usize) -> String {
    let lines: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let start = lines.len().saturating_sub(max);
    lines[start..].join("\n")
}

/// Clean tmux capture for Discord: drop wrapper noise, dev-server boilerplate, cap size.
fn sanitize_discord_terminal_excerpt(raw: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut in_nextjs_origin_warning = false;
    let mut prev_blank = false;

    for line in raw.lines() {
        let trimmed = line.trim();

        if should_skip_discord_run_line(trimmed) || is_mostly_box_drawing(trimmed) {
            continue;
        }

        if in_nextjs_origin_warning {
            if trimmed.contains("Read more:")
                || trimmed.starts_with("https://nextjs.org/docs/")
            {
                in_nextjs_origin_warning = false;
            }
            continue;
        }

        if trimmed.contains("Cross origin request detected") {
            in_nextjs_origin_warning = true;
            continue;
        }

        if trimmed.is_empty() {
            if prev_blank {
                continue;
            }
            prev_blank = true;
        } else {
            prev_blank = false;
        }

        lines.push(line.to_string());
    }

    let take = lines.len().min(DISCORD_RUN_EXCERPT_MAX_LINES);
    let start = lines.len().saturating_sub(take);
    let mut text = lines[start..].join("\n");
    if text.chars().count() > DISCORD_RUN_EXCERPT_MAX_CHARS {
        let truncated: String = text
            .chars()
            .take(DISCORD_RUN_EXCERPT_MAX_CHARS)
            .collect();
        text = format!("{truncated}\n…");
    }
    text.trim().to_string()
}

fn should_skip_discord_run_line(trimmed: &str) -> bool {
    is_exit_capture_noise_line(trimmed)
        || trimmed.starts_with("bash -lc ")
        || trimmed.starts_with("bash --noprofile")
        || trimmed.contains("[discord] $")
}

/// Wrapper echoes and partial `__BUNNY_EXIT__$?` tmux capture artifacts.
fn is_exit_capture_noise_line(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() || t == "?" {
        return true;
    }
    if t.contains(BUNNY_EXIT_MARKER) || t.contains("BUNNY_EXIT__") {
        return true;
    }
    if t.starts_with("(PAGER=cat GIT_PAGER=cat") || t.starts_with("PAGER=cat GIT_PAGER=cat") {
        return true;
    }
    if t.contains("PAGER=cat GIT_PAGER=cat") && t.contains("2>&1") {
        return true;
    }
    // Truncated tmux echo: prompt + wrapper without the trailing `2>&1; echo …`.
    if t.contains("PAGER=cat GIT_PAGER=cat git") && (t.contains('#') || t.contains('@')) {
        return true;
    }
    if t.contains("; echo ") && t.contains("EXIT__") {
        return true;
    }
    if t.ends_with("$?") && (t.contains("EXIT__") || t.len() <= 12) {
        return true;
    }
    false
}

/// Clean Discord/tmux captured output before notebook blocks or API excerpts.
pub(crate) fn curate_discord_shell_output(command: &str, output: &str) -> String {
    let text = strip_shell_capture_artifacts(command, output);
    if text.is_empty() {
        "(no output)".to_string()
    } else {
        text
    }
}

fn strip_shell_capture_artifacts(command: &str, output: &str) -> String {
    let output = strip_ansi_escapes(output);
    let output = output.replace("\r\n", "\n").replace('\r', "\n");
    output
        .lines()
        .filter(|line| {
            let t = line.trim();
            !t.is_empty()
                && t != command
                && !is_exit_capture_noise_line(line)
                && !is_shell_prompt_line(line)
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn is_mostly_box_drawing(s: &str) -> bool {
    if s.len() < 8 {
        return false;
    }
    let box_chars = s
        .chars()
        .filter(|c| matches!(c, '─' | '━' | '│' | '┌' | '┐' | '└' | '┘' | '╭' | '╮' | '╰' | '╯' | '▲'))
        .count();
    box_chars * 3 > s.len()
}

#[cfg(test)]
mod discord_run_tests {
    use super::*;

    #[test]
    fn virtualenv_subprocess_path_prepends_venv_bin() {
        let path = discord_subprocess_path_for_venv("/root/project", None, "/usr/bin");
        assert_eq!(path, "/root/project/bin:/usr/bin");
    }

    #[test]
    fn virtualenv_subprocess_path_prefers_pane_path() {
        let path = discord_subprocess_path_for_venv(
            "/root/project",
            Some("/root/project/bin:/usr/bin"),
            "/usr/bin",
        );
        assert_eq!(path, "/root/project/bin:/usr/bin");
    }

    #[test]
    fn activate_after_prompt_change_still_finds_exit_marker() {
        let baseline = "root@host:~/project# ";
        let cap = concat!(
            "root@host:~/project# source bin/activate; echo __BUNNY_EXIT__$?\n",
            "__BUNNY_EXIT__0\n",
            "(project) root@host:~/project# "
        );
        let wrapped = "source bin/activate; echo __BUNNY_EXIT__$?";
        let since = pane_text_delta_for_discord_run(baseline, cap, "source bin/activate", wrapped);
        let (_, code) = split_on_exit_marker(&since).unwrap();
        assert_eq!(code, 0);
        assert!(!since.is_empty());
    }

    #[test]
    fn split_on_exit_marker_leaves_empty_not_no_output_placeholder() {
        let (out, code) = split_on_exit_marker("source bin/activate; echo __BUNNY_EXIT__$?\n__BUNNY_EXIT__0").unwrap();
        assert_eq!(code, 0);
        assert!(out.is_empty());
    }

    #[test]
    fn curate_strips_partial_exit_marker_echo() {
        let out = curate_discord_shell_output("pwd", "_BUNNY_EXIT__$?\n/root/project");
        assert_eq!(out, "/root/project");
    }

    #[test]
    fn exit_marker_parses_code() {
        let (out, code) = split_on_exit_marker("hello\nworld\n__BUNNY_EXIT__0").unwrap();
        assert_eq!(out, "hello\nworld");
        assert_eq!(code, 0);
    }

    #[test]
    fn fast_command_detected_against_pre_command_baseline() {
        let baseline = "root@host:~/app# ";
        let cap = concat!(
            "root@host:~/app# bash -lc 'ls'; echo __BUNNY_EXIT__$?\n",
            "AGENTS.md\nREADME.md\n__BUNNY_EXIT__0\n",
            "root@host:~/app# "
        );
        let since = pane_text_since_baseline(baseline, cap);
        let (out, code) = split_on_exit_marker(&since).unwrap();
        assert_eq!(code, 0);
        assert!(out.contains("AGENTS.md"));
    }

    #[test]
    fn pane_delta_after_resize_mismatch_uses_command_echo() {
        let baseline = "line one\nline two\nroot@host:~/app# ";
        let cap = concat!(
            "wrapped line one\n",
            "wrapped line two\n",
            "root@host:~/app# sudo ls\n",
            "bash: sudo: command not found\n",
            "root@host:~/app# "
        );
        let since = pane_text_delta(baseline, cap, Some("sudo ls"));
        assert!(since.contains("command not found"));
        assert!(!since.contains("line one"));
        assert!(!since.contains("git add"));
    }

    #[test]
    fn pane_delta_keeps_repeated_ls_output() {
        let baseline = concat!(
            "root@host:~# (PAGER=cat GIT_PAGER=cat ls) 2>&1; echo __BUNNY_EXIT__$?\n",
            "project\n",
            "__BUNNY_EXIT__0\n",
            "root@host:~# (PAGER=cat GIT_PAGER=cat pwd) 2>&1; echo __BUNNY_EXIT__$?\n",
            "/root\n",
            "__BUNNY_EXIT__0\n",
            "root@host:~# "
        );
        let cap = format!(
            "{baseline}(PAGER=cat GIT_PAGER=cat ls) 2>&1; echo __BUNNY_EXIT__$?\n\
             project\n\
             __BUNNY_EXIT__0\n\
             root@host:~# "
        );
        let exec = "(PAGER=cat GIT_PAGER=cat ls) 2>&1; echo __BUNNY_EXIT__$?";
        let (parsed, code) = capture_instant_notebook_output(baseline, &cap, "ls", exec);
        assert_eq!(code, Some(0));
        assert!(parsed.contains("project"));
    }

    #[test]
    fn pane_delta_keeps_repeated_ls_via_command_echo_path() {
        let baseline = concat!(
            "root@host:~# (PAGER=cat GIT_PAGER=cat ls) 2>&1; echo __BUNNY_EXIT__$?\n",
            "project\n",
            "__BUNNY_EXIT__0\n",
            "root@host:~# (PAGER=cat GIT_PAGER=cat pwd) 2>&1; echo __BUNNY_EXIT__$?\n",
            "/root\n",
            "__BUNNY_EXIT__0\n",
            "root@host:~# "
        );
        let cap = concat!(
            "project\n",
            "__BUNNY_EXIT__0\n",
            "root@host:~# (PAGER=cat GIT_PAGER=cat pwd) 2>&1; echo __BUNNY_EXIT__$?\n",
            "/root\n",
            "__BUNNY_EXIT__0\n",
            "root@host:~# (PAGER=cat GIT_PAGER=cat ls) 2>&1; echo __BUNNY_EXIT__$?\n",
            "project\n",
            "__BUNNY_EXIT__0\n",
            "root@host:~# "
        );
        let exec = "(PAGER=cat GIT_PAGER=cat ls) 2>&1; echo __BUNNY_EXIT__$?";
        let delta = pane_text_delta(baseline, cap, Some(exec));
        let (parsed, code) = notebook_parse_captured_output(&delta);
        assert_eq!(code, Some(0));
        assert_eq!(parsed.trim(), "project");
    }

    #[test]
    fn pane_delta_visible_capture_strips_baseline_lines() {
        let baseline = concat!(
            "root@host:~# (PAGER=cat GIT_PAGER=cat ls) 2>&1; echo __BUNNY_EXIT__$?\n",
            "project\n",
            "__BUNNY_EXIT__0\n",
            "root@host:~# (PAGER=cat GIT_PAGER=cat cd project) 2>&1; echo __BUNNY_EXIT__$?\n",
            "__BUNNY_EXIT__0\n",
            "root@host:~/project# "
        );
        // Visible pane: suffix of history still on screen plus the new command output.
        let cap = concat!(
            "project\n",
            "__BUNNY_EXIT__0\n",
            "root@host:~# (PAGER=cat GIT_PAGER=cat cd project) 2>&1; echo __BUNNY_EXIT__$?\n",
            "__BUNNY_EXIT__0\n",
            "root@host:~/project# (PAGER=cat GIT_PAGER=cat ls) 2>&1; echo __BUNNY_EXIT__$?\n",
            "bin  lib  pyvenv.cfg  tentative.md\n",
            "__BUNNY_EXIT__0\n",
            "root@host:~/project# "
        );
        let exec = "(PAGER=cat GIT_PAGER=cat ls) 2>&1; echo __BUNNY_EXIT__$?";
        let (parsed, code) = capture_instant_notebook_output(baseline, cap, "ls", exec);
        assert_eq!(code, Some(0));
        assert!(parsed.contains("bin  lib  pyvenv.cfg  tentative.md"));
        assert!(!parsed.contains("project"));
    }

    #[test]
    fn pane_delta_strips_stale_ls_output_between_successive_runs() {
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
        let exec = "(PAGER=cat GIT_PAGER=cat ls) 2>&1; echo __BUNNY_EXIT__$?";
        let (parsed, code) = capture_instant_notebook_output(baseline, &cap, "ls", exec);
        assert_eq!(code, Some(0));
        assert!(parsed.contains("bin  lib  pyvenv.cfg  tentative.md"));
        assert!(!parsed.contains("project"));
    }

    #[test]
    fn pane_delta_uses_last_command_echo_not_first() {
        let baseline = "root@host:~/app# ls\nfile.txt\nroot@host:~/app# ";
        let cap = concat!(
            "root@host:~/app# ls\n",
            "file.txt\n",
            "root@host:~/app# git add -p\n",
            "No changes.\n",
            "root@host:~/app# "
        );
        let since = pane_text_delta(baseline, cap, Some("git add -p"));
        assert!(since.contains("No changes."));
        assert!(!since.contains("file.txt"));
        assert!(!since.contains("git add -h"));
    }

    #[test]
    fn extract_command_output_finds_pip_error_after_prompt_line() {
        let cap = concat!(
            "root@host:~/app# pip3 install requests\n",
            "error: externally-managed-environment\n",
            "× This environment is externally managed\n",
            "root@host:~/app# "
        );
        let out = extract_command_output_from_pane(cap, "pip3 install requests");
        assert!(out.contains("externally-managed-environment"));
        assert!(!out.contains("root@host"));
    }

    #[test]
    fn strip_baseline_keeps_output_when_all_lines_would_be_filtered() {
        let before = "error: externally-managed-environment\n";
        let delta = "error: externally-managed-environment\n";
        let out = strip_lines_in_baseline(delta, before);
        assert!(out.contains("externally-managed-environment"));
    }

    #[test]
    fn pane_delta_never_dumps_full_pane_on_weak_prefix() {
        let baseline = "old history line\nroot@host:~/app# ";
        let cap = "completely different layout\nroot@host:~/app# ";
        let since = pane_text_delta(baseline, cap, Some("missing-cmd"));
        assert!(since.is_empty());
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_single_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn cd_runs_in_interactive_shell_not_subshell() {
        assert!(discord_run_needs_interactive_shell("cd my-app"));
        assert!(discord_run_needs_interactive_shell("cd my-app && npm install"));
        let wrapped = discord_run_wrap_command("cd my-app", true);
        assert!(!wrapped.contains("bash -lc"));
        assert!(!wrapped.contains("bash --noprofile"));
        assert!(wrapped.contains(BUNNY_EXIT_MARKER));
    }

    #[test]
    fn ordinary_commands_use_bash_subshell_outside_tmux() {
        assert!(!discord_run_needs_interactive_shell("ls -la"));
        let wrapped = discord_run_wrap_command("ls -la", false);
        assert!(wrapped.contains("bash --noprofile --norc -c "));
    }

    #[test]
    fn ordinary_commands_use_tmux_subshell_in_pane() {
        let wrapped = discord_run_wrap_command("pip list", true);
        assert!(wrapped.contains("PAGER=cat GIT_PAGER=cat pip list"));
        assert!(!wrapped.contains("bash --noprofile"));
        assert!(wrapped.contains(BUNNY_EXIT_MARKER));
    }

    #[test]
    fn pane_text_detects_virtualenv_prompt() {
        assert!(pane_text_shows_virtualenv_prefix("(project) $ "));
        assert!(pane_text_shows_virtualenv_prefix("root@host:~/project# source bin/activate\n(project) $ "));
        assert!(!pane_text_shows_virtualenv_prefix("root@host:~/project# "));
    }

    #[test]
    fn virtual_env_from_path_prefix() {
        let venv = virtual_env_from_pane_path_for_test("/root/project/bin:/usr/bin");
        assert_eq!(venv, Some("/root/project".to_string()));
    }

    fn virtual_env_from_pane_path_for_test(path: &str) -> Option<String> {
        for segment in path.split(':') {
            let segment = segment.trim();
            if segment.is_empty() {
                continue;
            }
            let bin = std::path::Path::new(segment);
            if bin.file_name().and_then(|s| s.to_str()) != Some("bin") {
                continue;
            }
            let venv = bin.parent()?;
            return Some(venv.to_string_lossy().into_owned());
        }
        None
    }

    #[test]
    fn sanitize_strips_nextjs_cross_origin_block() {
        let raw = "▲ Next.js 16.1.6\n- Local: http://localhost:3000\nCross origin request detected from 127.0.0.1\nallowedDevOrigins in next.config.js\nRead more: https://nextjs.org/docs/app/api-reference/config/next-config-js/allowedDevOrigins\n✓ Ready in 200ms";
        let out = sanitize_discord_terminal_excerpt(raw);
        assert!(!out.contains("Cross origin"));
        assert!(!out.contains("allowedDevOrigins"));
        assert!(out.contains("Ready in"));
    }

    #[test]
    fn snapshot_live_fold_prefers_richer_tmux_capture() {
        let tmux = "root@host:~/app# ls\nAGENTS.md\nREADME.md\nroot@host:~/app# ";
        let buffer = "root@host:~/app# ";
        let out = super::fold_live_snapshot_parts(vec![tmux.to_string(), buffer.to_string()]);
        assert!(out.contains("AGENTS.md"));
        assert!(out.contains("ls"));
    }

    #[test]
    fn snapshot_merge_keeps_disk_history_over_short_tmux_capture() {
        let disk = (0..60)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let tmux = "root@host:~/app# ";
        let out = super::merge_snapshot_sources(disk.clone(), tmux);
        assert!(out.contains("line 0"));
        assert!(out.contains("line 59"));
        assert!(out.len() >= disk.len());
    }

    #[test]
    fn snapshot_merge_appends_live_bytes_after_disk() {
        let disk = "root@host:~/app# ls\nAGENTS.md\nREADME.md\n";
        let live = "root@host:~/app# ls\nAGENTS.md\nREADME.md\npackage.json\nroot@host:~/app# ";
        let out = super::merge_snapshot_sources(disk.to_string(), live);
        assert!(out.contains("package.json"));
    }

    #[test]
    fn prepare_snapshot_turns_tty_carriage_returns_into_lines() {
        let raw = "root@host:~/app# ls\rAGENTS.md\rREADME.md\rroot@host:~/app# ";
        let clean = super::prepare_snapshot_terminal_text(raw);
        assert!(clean.contains("AGENTS.md"));
        assert!(clean.contains("README.md"));
        assert!(clean.contains("ls"));
    }

    #[test]
    fn transcript_summarizes_claude_json_result() {
        let raw = r#"{"session_id":"s1","result":"test.md créé et commit effectué.","usage":{"input_tokens":1}}"#;
        let cmd = "claude -p --output-format json --permission-mode acceptEdits 'fais un commit'";
        let out = super::sanitize_discord_transcript_output(cmd, raw);
        assert_eq!(out, "test.md créé et commit effectué.");
        assert!(!out.contains("input_tokens"));
    }

    #[test]
    fn transcript_summarizes_claude_bash_permission() {
        let raw = r#"{"permission_denials":[{"tool_name":"Bash","tool_input":{"command":"git add test.md"}}]}"#;
        let cmd = "claude -p --output-format json '…'";
        let out = super::sanitize_discord_transcript_output(cmd, raw);
        assert!(out.contains("autorisation shell"));
        assert!(out.contains("git add test.md"));
    }

    #[test]
    fn transcript_leaves_ordinary_command_output() {
        let out = super::sanitize_discord_transcript_output("git log -1", "abc1234 fix\n");
        assert_eq!(out, "abc1234 fix");
    }

    #[test]
    fn git_interactive_commands_detected() {
        assert!(git_command_expects_interactive("git add -p"));
        assert!(git_command_expects_interactive("git add --patch"));
        assert!(git_command_expects_interactive("git rebase -i HEAD~2"));
        assert!(git_command_expects_interactive("git commit"));
        assert!(!git_command_expects_interactive("git log"));
        assert!(!git_command_expects_interactive("git commit -m 'fix'"));
        assert!(user_command_expects_interactive("git add -p"));
    }

    #[test]
    fn non_interactive_commands_use_subprocess_only_as_fallback() {
        assert!(!discord_run_needs_interactive_shell("git log"));
        assert!(!discord_run_needs_interactive_shell("ls -la"));
        let tmux_wrapped = discord_run_wrap_command("git log", true);
        assert!(tmux_wrapped.contains(BUNNY_EXIT_MARKER));
        assert!(tmux_wrapped.contains("PAGER=cat GIT_PAGER=cat git --no-pager log"));
        let subprocess_wrapped = discord_run_wrap_command("git log", false);
        assert!(subprocess_wrapped.contains("bash --noprofile"));
        // Interactive builtins still need the tmux pane.
        assert!(discord_run_needs_interactive_shell("cd my-app"));
    }

    #[test]
    fn pane_detects_pip_uninstall_confirmation_prompt() {
        let cap = "Proceed (y/n)?";
        assert!(pane_text_awaits_user_input(cap));
    }

    #[test]
    fn pip_uninstall_suggests_yes_flag_for_discord() {
        assert_eq!(
            suggest_noninteractive_pip_uninstall("pip uninstall requests"),
            Some("pip uninstall -y requests".to_string())
        );
        assert_eq!(
            suggest_noninteractive_pip_uninstall("pip uninstall -y requests"),
            None
        );
    }

    #[test]
    fn pip_uninstall_blocked_from_discord_without_yes() {
        assert!(discord_shell_run_preflight("pip uninstall requests").is_err());
        assert!(discord_shell_run_preflight("pip uninstall -y requests").is_ok());
    }

    #[test]
    fn pip_uninstall_preflight_carries_i18n_key() {
        let err = discord_shell_run_preflight("pip uninstall requests").unwrap_err();
        assert_eq!(err.key, "discord.run.interactive_pip_uninstall");
        assert_eq!(err.args[0].1, "pip uninstall -y requests");
        let en = bunny_i18n::t(
            Locale::En,
            err.key,
            &[("suggested", &err.args[0].1)],
        );
        assert!(en.contains("interactive confirmation"));
        let fr = bunny_i18n::t(
            Locale::Fr,
            err.key,
            &[("suggested", &err.args[0].1)],
        );
        assert!(fr.contains("confirmation interactive"));
    }

    #[test]
    fn pip_uninstall_detected_as_interactive() {
        assert!(pip_command_expects_interactive("pip uninstall requests"));
        assert!(pip_command_expects_interactive("pip3 uninstall requests"));
        assert!(!pip_command_expects_interactive("pip uninstall -y requests"));
        assert!(!pip_command_expects_interactive("pip install requests"));
        assert!(user_command_expects_interactive("pip uninstall requests"));
    }

    #[test]
    fn pip_install_gets_extended_quick_wait() {
        assert_eq!(
            discord_run_quick_wait("pip install requests"),
            std::time::Duration::from_secs(45)
        );
        assert_eq!(discord_run_quick_wait("ls -la"), DISCORD_RUN_QUICK_WAIT);
    }

    #[test]
    fn curate_pip_install_keeps_success_line_only() {
        let raw = "Collecting requests\n  Downloading requests-2.32.0.whl (120 kB)\nInstalling collected packages: requests\nSuccessfully installed requests-2.32.0 urllib3-2.2.0";
        let out = curate_pip_discord_output(raw);
        assert!(out.contains("Successfully installed requests"));
        assert!(!out.contains("Downloading"));
    }

    #[test]
    fn curate_pip_install_keeps_already_satisfied() {
        let raw = "Requirement already satisfied: requests in ./lib/python3.12/site-packages (2.32.0)";
        let out = curate_pip_discord_output(raw);
        assert!(out.contains("Requirement already satisfied"));
    }

    #[test]
    fn line_echoes_truncated_git_log_oneline() {
        let line = "root@2ccd835b85cb:~/project# (PAGER=cat GIT_PAGER=cat git --no-pager log -1 --on";
        assert!(line_echoes_command(line, "git log -1 --oneline"));
        let wrapped = discord_run_wrap_command("git log -1 --oneline", true);
        assert!(line_echoes_command(line, &wrapped));
    }

    #[test]
    fn parse_truncated_git_log_oneline_echo_returns_single_line() {
        let wrapped = discord_run_wrap_command("git log -1 --oneline", true);
        let truncated = "(PAGER=cat GIT_PAGER=cat git --no-pager log -1 --on";
        let text = format!(
            "root@host:~/project# {truncated}\n\
             58a3135 (HEAD -> master) added yo\n\
             __BUNNY_EXIT__0\n\
             root@host:~/project# "
        );
        let (out, code) =
            parse_captured_run_after_last_echo(&text, "git log -1 --oneline", &wrapped).unwrap();
        assert_eq!(code, 0);
        assert_eq!(out.trim(), "58a3135 (HEAD -> master) added yo");
    }

    #[test]
    fn parse_after_last_echo_ignores_prior_git_log_when_echo_truncated() {
        let wrapped = discord_run_wrap_command("git log -1 --oneline", true);
        let truncated = "(PAGER=cat GIT_PAGER=cat git --no-pager log -1 --on";
        let text = format!(
            "58a3135 (HEAD -> master) added yo\n\
             root@host:~/project# {truncated}\n\
             58a3135 (HEAD -> master) added yo\n\
             __BUNNY_EXIT__0\n\
             root@host:~/project# "
        );
        let (out, code) =
            parse_captured_run_after_last_echo(&text, "git log -1 --oneline", &wrapped).unwrap();
        assert_eq!(code, 0);
        assert_eq!(out.matches("58a3135").count(), 1);
    }

    #[test]
    fn discord_git_status_parses_via_exit_marker_without_echo_match() {
        let baseline = "(project) root@host:~/project# ";
        let wrapped = discord_run_wrap_command("git status", true);
        let cap = format!(
            "(project) root@host:~/project# {wrapped}\n\
             On branch master\n\
             Untracked files:\n\
               .gitignore\n\
             nothing added to commit but untracked files present (use \"git add\" to track)\n\
             __BUNNY_EXIT__0\n\
             (project) root@host:~/project# "
        );
        let since = pane_text_delta_for_discord_run(baseline, &cap, "git status", &wrapped);
        let parsed =
            discord_try_parse_finished_command(baseline, &cap, "git status", &wrapped, &since)
                .expect("git status should finish via exit marker");
        assert_eq!(parsed.1, 0);
        assert!(parsed.0.contains("On branch master"));
        assert!(parsed.0.contains(".gitignore"));
    }

    #[test]
    fn notebook_instant_includes_non_interactive_git() {
        assert!(notebook_instant_command("git status"));
        assert!(notebook_instant_command("git log -1 --oneline"));
        assert!(notebook_instant_command("git commit -m \"fix\""));
        assert!(!notebook_instant_command("git commit"));
        assert!(!notebook_instant_command("git add -p"));
    }

    #[test]
    fn wrapped_commit_echo_tail_parses_subcommand() {
        let wrapped = discord_run_wrap_command("git commit -m \"nothing\"", true);
        assert_eq!(
            git_command_echo_tail(&wrapped).as_deref(),
            Some("commit -m \"nothing\"")
        );
    }

    #[test]
    fn git_status_echo_does_not_match_git_commit_command() {
        let line = "root@host:~/project# (PAGER=cat GIT_PAGER=cat git --no-pager status) 2>&1; echo __BUNNY_EXIT__$?";
        let commit = "git commit -m \"nothing\"";
        assert!(!line_echoes_command(line, commit));
        let wrapped = discord_run_wrap_command(commit, true);
        for v in command_echo_variants(&wrapped) {
            assert!(
                !line_echoes_command_variant(line, &v),
                "status echo matched commit variant: {v:?}"
            );
        }
    }

    #[test]
    fn parse_git_commit_after_git_status_uses_commit_echo_not_status() {
        let status_wrapped = discord_run_wrap_command("git status", true);
        let commit = "git commit -m \"nothing2\"";
        let commit_wrapped = discord_run_wrap_command(commit, true);
        let text = format!(
            "root@host:~/project# {status_wrapped}\n\
             On branch master\n\
             Untracked files:\n\
               .gitignore\n\
             nothing added to commit but untracked files present (use \"git add\" to track)\n\
             __BUNNY_EXIT__0\n\
             root@host:~/project# {commit_wrapped}\n\
             On branch master\n\
             Untracked files:\n\
               .gitignore\n\
             nothing added to commit but untracked files present (use \"git add\" to track)\n\
             __BUNNY_EXIT__1\n\
             root@host:~/project# "
        );
        let (out, code) =
            parse_captured_run_after_last_echo(&text, commit, &commit_wrapped).unwrap();
        assert_eq!(code, 1);
        assert_eq!(out.matches("On branch master").count(), 1);
        assert!(out.contains("nothing added to commit"));
    }

    #[test]
    fn line_echoes_git_status_with_no_pager_and_truncated_wrap() {
        let line = "root@2ccd835b85cb:~/project# (PAGER=cat GIT_PAGER=cat git --no-pager status) 2>&1";
        assert!(line_echoes_command(line, "git status"));
        let wrapped = discord_run_wrap_command("git status", true);
        assert!(line_echoes_command(line, &wrapped));
    }

    #[test]
    fn parse_after_last_echo_returns_only_latest_git_status() {
        let wrapped = discord_run_wrap_command("git status", true);
        let truncated = "(PAGER=cat GIT_PAGER=cat git --no-pager status) 2>&1";
        let text = format!(
            "root@host:~/project# {truncated}\n\
             On branch master\n\
             Untracked files:\n\
               .gitignore\n\
             __BUNNY_EXIT__0\n\
             root@host:~/project# {truncated}\n\
             On branch master\n\
             Untracked files:\n\
               .gitignore\n\
             __BUNNY_EXIT__0\n\
             root@host:~/project# "
        );
        let (out, code) =
            parse_captured_run_after_last_echo(&text, "git status", &wrapped).unwrap();
        assert_eq!(code, 0);
        assert_eq!(out.matches("On branch master").count(), 1);
        assert!(out.contains(".gitignore"));
    }

    #[test]
    fn pane_new_marker_finds_git_log_when_delta_misses_exit() {
        let baseline = "(project) root@host:~/project# ";
        let wrapped = discord_run_wrap_command("git log -1 --oneline", true);
        let cap = concat!(
            "(project) root@host:~/project# ",
            "(PAGER=cat GIT_PAGER=cat git --no-pager log -1 --oneline) 2>&1; echo __BUNNY_EXIT__$?\n",
            "58a3135 (HEAD -> master) added yo\n",
            "__BUNNY_EXIT__0\n",
            "(project) root@host:~/project# "
        );
        let (out, code) = discord_try_parse_finished_command(
            baseline,
            cap,
            "git log -1 --oneline",
            &wrapped,
            "",
        )
        .unwrap();
        assert_eq!(code, 0);
        assert!(out.contains("58a3135"));
    }

    #[test]
    fn discord_try_parse_uses_pane_fallback_when_delta_empty() {
        let baseline = "(project) root@host:~/project# ";
        let wrapped = discord_run_wrap_command("git log -1 --oneline", true);
        let cap = concat!(
            "(project) root@host:~/project# ",
            "(PAGER=cat GIT_PAGER=cat git --no-pager log -1 --oneline) 2>&1; echo __BUNNY_EXIT__$?\n",
            "58a3135 (HEAD -> master) added yo\n",
            "__BUNNY_EXIT__0\n",
            "(project) root@host:~/project# "
        );
        let (out, code) = discord_try_parse_finished_command(
            baseline,
            cap,
            "git log -1 --oneline",
            &wrapped,
            "",
        )
        .unwrap();
        assert_eq!(code, 0);
        assert!(out.contains("58a3135"));
    }

    #[test]
    fn git_status_after_prior_commands_finds_new_exit_marker() {
        let baseline = concat!(
            "(project) root@host:~/project# (PAGER=cat GIT_PAGER=cat pwd) 2>&1; echo __BUNNY_EXIT__$?\n",
            "/root/project\n",
            "__BUNNY_EXIT__0\n",
            "(project) root@host:~/project# "
        );
        let wrapped = discord_run_wrap_command("git status", true);
        let cap = format!(
            "{baseline}{wrapped}\nOn branch master\nUntracked files:\n  .gitignore\n__BUNNY_EXIT__0\n(project) root@host:~/project# "
        );
        let since = pane_text_delta_for_discord_run(baseline, &cap, "git status", &wrapped);
        let (out, code) = discord_try_parse_finished_command(
            baseline,
            &cap,
            "git status",
            &wrapped,
            &since,
        )
        .unwrap();
        assert_eq!(code, 0);
        assert!(out.contains("On branch master"));
        assert!(out.contains(".gitignore"));
    }

    #[test]
    fn git_status_parses_when_scrollback_prefix_diverges() {
        let baseline = concat!(
            "older scrollback line\n",
            "(project) root@host:~/project# (PAGER=cat GIT_PAGER=cat git --no-pager log) 2>&1; echo __BUNNY_EXIT__$?\n",
            "commit abc\n",
            "__BUNNY_EXIT__0\n",
            "(project) root@host:~/project# "
        );
        let wrapped = discord_run_wrap_command("git status", true);
        let cap = concat!(
            "completely different tmux layout after resize\n",
            "(project) root@host:~/project# (PAGER=cat GIT_PAGER=cat git --no-pager s\n",
            "tatus) 2>&1; echo __BUNNY_EXIT__$?\n",
            "On branch master\n",
            "Untracked files:\n",
            "  .gitignore\n",
            "nothing added to commit but untracked files present (use \"git add\" to track)\n",
            "__BUNNY_EXIT__0\n",
            "(project) root@host:~/project# "
        );
        let since = pane_text_delta_for_discord_run(baseline, cap, "git status", &wrapped);
        assert!(
            since.is_empty(),
            "prefix mismatch should leave delta empty: {since:?}"
        );
        let (out, code) = discord_try_parse_finished_command(
            baseline,
            cap,
            "git status",
            &wrapped,
            &since,
        )
        .expect("git status should parse via newest exit marker");
        assert_eq!(code, 0);
        assert!(out.contains("On branch master"));
        assert!(out.contains(".gitignore"));
    }

    #[test]
    fn pager_safe_command_prefixes_git_no_pager() {
        assert_eq!(
            pager_safe_command("git log -1 --oneline"),
            "git --no-pager log -1 --oneline"
        );
        assert_eq!(
            pager_safe_command("git --no-pager log"),
            "git --no-pager log"
        );
    }

    #[test]
    fn incidental_pager_not_interactive_for_tree() {
        assert!(pane_cmd_is_incidental_pager("less", "tree"));
        assert!(pane_cmd_is_incidental_pager("more", "git log"));
        assert!(!pane_cmd_is_incidental_pager("less", "less"));
        assert!(!pane_cmd_is_incidental_pager("less", "man ls"));
        assert!(!user_command_expects_interactive("tree"));
    }

    #[test]
    fn notebook_exec_line_disables_pager() {
        assert_eq!(
            notebook_shell_exec_line("tree", false, true, None),
            "(PAGER=cat GIT_PAGER=cat tree) 2>&1 | cat; echo __BUNNY_EXIT__$?"
        );
        assert_eq!(
            notebook_shell_exec_line("tree", false, false, None),
            "tree"
        );
        assert_eq!(
            notebook_shell_exec_line("ls", false, true, None),
            "(PAGER=cat GIT_PAGER=cat ls) 2>&1; echo __BUNNY_EXIT__$?"
        );
        assert_eq!(
            notebook_shell_exec_line("npx create-next-app", true, true, None),
            "npx create-next-app"
        );
    }

    #[test]
    fn wrap_command_with_existing_activate() {
        let base = std::env::temp_dir().join(format!("bunny-venv-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("bin")).unwrap();
        std::fs::write(base.join("bin/activate"), "").unwrap();
        let venv = base.to_string_lossy().into_owned();
        let wrapped = wrap_command_with_virtualenv(&venv, "pip list").unwrap();
        assert!(wrapped.contains("source "));
        assert!(wrapped.contains("bin/activate"));
        assert!(wrapped.ends_with("pip list"));
        let line = notebook_shell_exec_line("pip list", false, true, Some(&venv));
        assert!(line.contains("source "));
        assert!(line.contains("pip list"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn notebook_state_commands_run_in_current_shell() {
        assert!(notebook_shell_state_command("cd yo"));
        assert!(notebook_shell_state_command("source bin/activate"));
        assert!(notebook_shell_state_command(". venv/bin/activate"));
        assert!(notebook_shell_state_command("deactivate"));
        assert!(!notebook_shell_state_command("ls"));
        assert_eq!(
            notebook_shell_exec_line("source bin/activate", false, true, None),
            "source bin/activate; echo __BUNNY_EXIT__$?"
        );
        assert_eq!(
            notebook_shell_exec_line("cd yo", false, true, None),
            "cd yo; echo __BUNNY_EXIT__$?"
        );
    }

    #[test]
    fn notebook_instant_commands_skip_interactive_heuristics() {
        assert!(notebook_instant_command("ls"));
        assert!(notebook_instant_command("pwd"));
        assert!(notebook_instant_command("ls -la"));
        assert!(notebook_instant_command("cd"));
        assert!(notebook_instant_command("cd .."));
        assert!(notebook_instant_command("cd yo"));
        assert!(notebook_instant_command("cd /tmp"));
        assert!(!notebook_instant_command("ls -la /tmp"));
        assert!(!runtime_interactive_promotion_allowed("ls", Some("less")));
    }

    #[test]
    fn notebook_parse_exit_marker() {
        let (out, code) = notebook_parse_captured_output("On branch main\n__BUNNY_EXIT__1");
        assert_eq!(out, "On branch main");
        assert_eq!(code, Some(1));
        let (out, code) = notebook_parse_captured_output("hello");
        assert_eq!(out, "hello");
        assert_eq!(code, None);
    }

    #[test]
    fn notebook_bare_git_commit_not_interactive() {
        assert!(!notebook_user_command_expects_interactive("git commit"));
        assert!(!notebook_user_command_expects_interactive("git commit -m msg"));
        assert!(user_command_expects_interactive("git commit"));
    }

    #[test]
    fn tree_not_runtime_interactive() {
        assert!(!runtime_interactive_promotion_allowed("tree", Some("less")));
        assert!(!runtime_interactive_promotion_allowed("tree", Some("tree")));
        assert!(runtime_interactive_promotion_allowed("less", Some("less")));
        assert!(runtime_interactive_promotion_allowed(
            "pip uninstall requests",
            Some("pip")
        ));
    }

    #[test]
    fn package_runners_detected_as_interactive() {
        assert!(user_command_expects_interactive("npx create-next-app@latest"));
        assert!(user_command_expects_interactive("bunx create-vite"));
        assert!(user_command_expects_interactive("npm init"));
        assert!(!user_command_expects_interactive("npx --yes create-next-app@latest"));
        assert!(!user_command_expects_interactive("ls -la"));
    }

    #[test]
    fn pane_node_process_suggests_interactive_for_npx() {
        assert!(pane_process_suggests_interactive(
            "node",
            "npx create-next-app@latest"
        ));
        assert!(!pane_process_suggests_interactive("node", "node -e 'console.log(1)'"));
    }

    #[test]
    fn subprocess_transcript_injects_full_summarized_output() {
        let raw = r#"{"result":"line one\nline two"}""#;
        let cmd = "claude -p --output-format json '…'";
        let entry = super::discord_transcript_entry_terminal(
            cmd,
            &super::DiscordTranscriptBody::Output(raw),
        );
        assert!(entry.contains("line one"));
        assert!(entry.contains("line two"));
        assert!(!entry.contains("input_tokens"));
    }
}

/// Run a Discord shell command with a hard timeout (avoids hanging the API on interactive tools).
pub fn exec_discord_shell_command_timed(
    state: &AppState,
    term_id: Uuid,
    session_id: Uuid,
    command: &str,
    timeout: std::time::Duration,
    thread_id: Option<&str>,
    acting_user_id: Option<Uuid>,
) -> Result<(String, i32)> {
    use bunny_pty::locale;
    use std::process::{Command, Stdio};

    if bunny_discord::risk::is_interactive_discord_command(command) {
        anyhow::bail!(
            "interactive command not supported from Discord — use the Web UI terminal, or e.g. `head -n 80 landing-page.html` / `cat landing-page.html`"
        );
    }

    let long_running = bunny_discord::risk::is_long_running_discord_shell_command(command);
    let with_venv = discord_subprocess_command(state, term_id, command);
    let run_command = if long_running {
        wrap_long_running_discord_command(&with_venv)
    } else {
        with_venv
    };
    let run_timeout = if long_running {
        DISCORD_BACKGROUND_START_TIMEOUT
    } else {
        timeout
    };

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
    cmd.arg("--noprofile")
        .arg("--norc")
        .arg("-c")
        .arg(&run_command)
        .current_dir(&cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in locale::utf8_locale_vars() {
        cmd.env(k, v);
    }
    cmd.env("TERM", "dumb");
    cmd.env("HOME", &home);
    cmd.env("PWD", cwd.display().to_string());
    apply_discord_shell_env(&mut cmd, state, term_id, session_id, acting_user_id);
    for (k, v) in state.secret_env_for_session(session_id) {
        cmd.env(k, v);
    }

    let out = run_command_with_timeout(&mut cmd, run_timeout, thread_id.map(|t| (state, t)))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let text = match (stdout.trim_end().is_empty(), stderr.trim_end().is_empty()) {
        (true, true) => String::new(),
        (false, true) => stdout.trim_end().to_string(),
        (true, false) => stderr.trim_end().to_string(),
        (false, false) => format!("{stdout}{stderr}").trim_end().to_string(),
    };
    append_discord_transcript(
        state,
        term_id,
        command,
        DiscordTranscriptBody::Output(&text),
        acting_user_id,
    );
    Ok((text, out.status.code().unwrap_or(1)))
}

pub const THREAD_CLAUDE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Run `claude -p` for a Discord thread; registers subprocess PID for `/thread/stop`.
pub fn exec_discord_shell_command_for_thread(
    state: &AppState,
    term_id: Uuid,
    session_id: Uuid,
    thread_id: &str,
    command: &str,
    acting_user_id: Option<Uuid>,
) -> Result<(String, i32)> {
    exec_discord_shell_command_timed(
        state,
        term_id,
        session_id,
        command,
        THREAD_CLAUDE_TIMEOUT,
        Some(thread_id),
        acting_user_id,
    )
}

fn apply_discord_shell_env(
    cmd: &mut std::process::Command,
    state: &AppState,
    term_id: Uuid,
    _session_id: Uuid,
    acting_user_id: Option<Uuid>,
) {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    apply_bunny_path(cmd, state, &home);
    cmd.env("BUNNY_TERMINAL_ID", term_id.to_string());
    if let Some(user) = acting_user_id {
        state.git_identity.set_actor(term_id, user, true);
        if let Ok(git_env) = git_env_for_user(state, user) {
            apply_git_env(cmd, &git_env);
        }
    }
    apply_discord_shell_virtualenv(cmd, state, term_id, &home);
}

/// Mirror an active virtualenv from the live tmux shell into Discord subprocess runs.
fn apply_discord_shell_virtualenv(
    cmd: &mut std::process::Command,
    state: &AppState,
    term_id: Uuid,
    home: &str,
) {
    if !state.terminals.uses_tmux() {
        return;
    }
    let Some(target) = state.terminals.tmux_target(term_id) else {
        return;
    };
    let Some(venv) = bunny_pty::tmux::pane_shell_env_var(&target, "VIRTUAL_ENV") else {
        return;
    };
    let venv = venv.trim();
    if venv.is_empty() {
        return;
    }
    cmd.env("VIRTUAL_ENV", venv);
    let pane_path = bunny_pty::tmux::pane_shell_env_var(&target, "PATH");
    let base_path = state.git_identity.path_with_bunny_bin(home);
    cmd.env(
        "PATH",
        discord_subprocess_path_for_venv(venv, pane_path.as_deref(), &base_path),
    );
}

fn discord_subprocess_path_for_venv(
    venv: &str,
    pane_path: Option<&str>,
    base_path: &str,
) -> String {
    if let Some(path) = pane_path.map(str::trim).filter(|p| !p.is_empty()) {
        return path.to_string();
    }
    format!(
        "{}:{}",
        std::path::Path::new(venv).join("bin").display(),
        base_path
    )
}

fn prepare_discord_tmux_git_actor(
    state: &AppState,
    term_id: Uuid,
    session_id: Uuid,
    acting_user_id: Option<Uuid>,
) -> Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    if let Some(user) = acting_user_id {
        state.git_identity.set_actor(term_id, user, true);
    }
    let mut session_env = state
        .git_identity
        .terminal_session_env(term_id, &home);
    session_env.extend(state.secret_env_for_session(session_id));
    let auth_db = state.auth.db();
    let row = auth_db
        .lock()
        .get_terminal(term_id)?
        .ok_or_else(|| anyhow::anyhow!("terminal not found"))?;
    let record = row_to_record(row);
    if let Some(target) = resolve_tmux_target(state, &record)? {
        let session = tmux::session_name_from_target(&target);
        tmux::apply_session_env(&session, &session_env);
    }
    Ok(())
}

pub fn cancel_thread_claude_run(state: &AppState, thread_id: &str) -> bool {
    let pid = state.thread_claude_pids.lock().remove(thread_id);
    if let Some(pid) = pid {
        let _ = std::process::Command::new("kill")
            .arg(pid.to_string())
            .status();
        return true;
    }
    false
}

struct TimedOutput {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    status: std::process::ExitStatus,
}

fn run_command_with_timeout(
    cmd: &mut std::process::Command,
    timeout: std::time::Duration,
    pid_registry: Option<(&AppState, &str)>,
) -> Result<TimedOutput> {
    use std::io::Read;
    use std::process::Stdio;
    use std::thread;
    use std::time::Instant;

    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("spawn failed: {e}"))?;

    if let Some((state, thread_id)) = pid_registry {
        state
            .thread_claude_pids
            .lock()
            .insert(thread_id.to_string(), child.id());
    }

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let started = Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if let Some((state, thread_id)) = pid_registry {
                    state.thread_claude_pids.lock().remove(thread_id);
                }
                if let Some(mut out) = child.stdout.take() {
                    let _ = out.read_to_end(&mut stdout);
                }
                if let Some(mut err) = child.stderr.take() {
                    let _ = err.read_to_end(&mut stderr);
                }
                return Ok(TimedOutput {
                    stdout,
                    stderr,
                    status,
                });
            }
            Ok(None) => {
                if started.elapsed() > timeout {
                    if let Some((state, thread_id)) = pid_registry {
                        state.thread_claude_pids.lock().remove(thread_id);
                    }
                    let _ = child.kill();
                    let _ = child.wait();
                    anyhow::bail!(
                        "command timed out after {}s — interactive editors (nvim, vim) are not supported from Discord",
                        timeout.as_secs()
                    );
                }
                thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// Strip ANSI escapes so injected Discord output does not clear or corrupt the xterm view.
pub fn strip_ansi_escapes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for ch in chars.by_ref() {
                    if ch.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// What to store in the Discord sidecar / Web UI transcript overlay.
enum DiscordTranscriptBody<'a> {
    /// Output already visible in the live tmux pane — marker line only.
    CommandOnly,
    /// Subprocess (not in tmux) — full summarized output in scrollback and live inject.
    Output(&'a str),
}

const DISCORD_TRANSCRIPT_OUTPUT_MAX_CHARS: usize = 6_000;

fn is_claude_print_discord_command(command: &str) -> bool {
    let c = command.trim();
    c.starts_with("claude -p") || c.starts_with("claude --print")
}

fn bash_command_from_claude_denial(d: &serde_json::Value) -> Option<String> {
    d.get("tool_input")
        .and_then(|i| i.get("command"))
        .and_then(|c| c.as_str())
        .map(str::to_string)
        .or_else(|| d.get("command").and_then(|c| c.as_str()).map(str::to_string))
}

/// Human-readable shell transcript for `claude -p --output-format json` (not the raw JSON blob).
fn summarize_claude_print_json_for_transcript(raw: &str) -> String {
    let trimmed = raw.trim();
    let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return trimmed.to_string();
    };
    if let Some(denials) = v.get("permission_denials").and_then(|d| d.as_array()) {
        for d in denials {
            if d.get("tool_name").and_then(|t| t.as_str()) == Some("Bash") {
                let cmd = bash_command_from_claude_denial(d).unwrap_or_else(|| "?".into());
                return format!("[claude] autorisation shell requise:\n{cmd}");
            }
        }
    }
    if v.get("ask_user_question").is_some() {
        return "[claude] question en attente (voir Discord)".to_string();
    }
    let is_error = v.get("is_error").and_then(|b| b.as_bool()) == Some(true)
        || v
            .get("subtype")
            .and_then(|s| s.as_str())
            .is_some_and(|s| s.starts_with("error_"));
    if is_error {
        return v
            .get("result")
            .and_then(|r| r.as_str())
            .map(|s| format!("[claude erreur] {s}"))
            .unwrap_or_else(|| "[claude erreur]".to_string());
    }
    match v.get("result") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) if other.is_string() => other.as_str().unwrap_or("").to_string(),
        Some(_) => "[claude] réponse structurée (voir Discord)".to_string(),
        None => "[claude] (pas de résultat texte)".to_string(),
    }
}

fn cap_discord_transcript_output(text: &str) -> String {
    let n = text.chars().count();
    if n <= DISCORD_TRANSCRIPT_OUTPUT_MAX_CHARS {
        return text.to_string();
    }
    let truncated: String = text
        .chars()
        .take(DISCORD_TRANSCRIPT_OUTPUT_MAX_CHARS)
        .collect();
    format!("{truncated}\n… [sortie tronquée]")
}

fn sanitize_discord_transcript_output(command: &str, output: &str) -> String {
    let output = curate_discord_shell_output(command, output);
    if output == "(no output)" {
        return output;
    }
    let summarized = if is_claude_print_discord_command(command) {
        summarize_claude_print_json_for_transcript(&output)
    } else {
        output
    };
    cap_discord_transcript_output(&summarized)
}

fn summarize_discord_command(command: &str) -> String {
    for prefix in ["claude -p ", "claude --print "] {
        if let Some(rest) = command.strip_prefix(prefix) {
            let rest = rest.trim();
            let unquoted = rest
                .strip_prefix('\'')
                .and_then(|s| s.strip_suffix('\''))
                .or_else(|| {
                    rest.strip_prefix('"')
                        .and_then(|s| s.strip_suffix('"'))
                })
                .unwrap_or(rest);
            let preview: String = unquoted.chars().take(72).collect();
            let suffix = if unquoted.chars().count() > 72 {
                "…"
            } else {
                ""
            };
            return format!("claude -p '{preview}{suffix}'");
        }
    }
    let n: String = command.chars().take(120).collect();
    if command.chars().count() > 120 {
        format!("{n}…")
    } else {
        n
    }
}

fn discord_transcript_entry(command: &str, body: &DiscordTranscriptBody<'_>) -> String {
    let label = summarize_discord_command(command);
    match body {
        DiscordTranscriptBody::CommandOnly => format!("[discord] $ {label}\n"),
        DiscordTranscriptBody::Output(output) => {
            let output = sanitize_discord_transcript_output(command, output);
            format!("[discord] $ {label}\n{output}\n")
        }
    }
}

fn discord_transcript_entry_terminal(command: &str, body: &DiscordTranscriptBody<'_>) -> String {
    let label = summarize_discord_command(command);
    match body {
        DiscordTranscriptBody::CommandOnly => format!("\r\n[discord] $ {label}\r\n"),
        DiscordTranscriptBody::Output(output) => {
            let output = sanitize_discord_transcript_output(command, output);
            format!("\r\n[discord] $ {label}\r\n{output}\r\n")
        }
    }
}

/// Record Discord runs for Web UI scrollback and snapshot overlay (target shell only).
fn append_discord_transcript(
    state: &AppState,
    term_id: Uuid,
    command: &str,
    body: DiscordTranscriptBody<'_>,
    acting_user_id: Option<Uuid>,
) {
    let (output, exit_code, persistent) = match &body {
        DiscordTranscriptBody::CommandOnly => (None, None, false),
        DiscordTranscriptBody::Output(text) => {
            let persistent = bunny_discord::risk::is_long_running_discord_shell_command(command);
            (Some(text.as_ref()), Some(0), persistent)
        }
    };

    let _ = crate::blocks::record_discord_transcript_blocks(
        state,
        term_id,
        command,
        output,
        exit_code,
        acting_user_id,
        persistent,
    );

    if state.config.terminal.notebook_shells {
        return;
    }

    let dir = scrollback_dir(state);
    let _ = std::fs::create_dir_all(&dir);

    let entry = discord_transcript_entry(command, &body);
    let entry_terminal = discord_transcript_entry_terminal(command, &body);

    let path = discord_transcript_path(state, term_id);
    let mut discord_only = std::fs::read_to_string(&path).unwrap_or_default();
    if !discord_only.is_empty() && !discord_only.ends_with('\n') {
        discord_only.push('\n');
    }
    discord_only.push_str(&entry);
    trim_tail_bytes(&mut discord_only, 256 * 1024);
    let _ = std::fs::write(&path, discord_only);

    // Append to on-disk scrollback without scrollback::save() re-merge (which can drop history).
    let scroll_path = scrollback::scrollback_path(&dir, term_id);
    let mut scroll = std::fs::read_to_string(&scroll_path).unwrap_or_default();
    if !scroll.is_empty() && !scroll.ends_with("\n") {
        scroll.push_str("\r\n");
    }
    scroll.push_str(&entry_terminal);
    trim_tail_bytes(&mut scroll, 512 * 1024);
    let _ = std::fs::write(&scroll_path, scroll);

    state.terminals.inject_transcript(term_id, &entry_terminal);
}

fn trim_tail_bytes(text: &mut String, max_bytes: usize) {
    if text.len() <= max_bytes {
        return;
    }
    let keep = &text[text.len() - max_bytes..];
    *text = keep[keep.find('\n').map(|i| i + 1).unwrap_or(0)..].to_string();
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

/// Last N logical lines returned by Discord `/bunny snapshot` (Web UI scrollback, not a pane image).
pub const DISCORD_SNAPSHOT_MAX_LINES: usize = 50;

/// Merge persisted scrollback with shorter live sources (tmux pane or attach buffer).
#[cfg(test)]
fn merge_snapshot_sources(base: String, extension: &str) -> String {
    let extension = extension.trim_end();
    if extension.is_empty() {
        return base;
    }
    scrollback::merge(
        if base.trim().is_empty() {
            None
        } else {
            Some(base)
        },
        extension.to_string(),
    )
}

/// Combine live tmux / attach-buffer chunks into the current shell view.
fn fold_live_snapshot_parts(parts: Vec<String>) -> String {
    let mut best = String::new();
    for part in parts {
        let part = part.trim_end().to_string();
        if part.is_empty() {
            continue;
        }
        if best.is_empty() {
            best = part;
            continue;
        }
        if part.contains(best.trim()) {
            best = part;
        } else if best.contains(part.trim()) {
            continue;
        } else {
            let suffix = pane_text_since_baseline(&best, &part);
            if !suffix.trim().is_empty() && !best.contains(suffix.trim()) {
                if !best.ends_with('\n') {
                    best.push('\n');
                }
                best.push_str(&suffix);
            }
        }
    }
    best
}

/// Normalize PTY/tmux bytes into logical lines for Discord (TTY `\r` → newlines).
fn prepare_snapshot_terminal_text(text: &str) -> String {
    let expanded = text.replace("\r\n", "\n").replace('\r', "\n");
    crate::compositor::normalize_terminal_text(&expanded)
}

/// Best-effort view of what is **currently** in the shell (live tmux pane + attach buffer).
/// Persisted disk scrollback is only used when the shell is not running / has no live capture.
pub fn terminal_display_text(state: &AppState, term_id: Uuid) -> String {
    let _ = prepare_terminal_connection(state, term_id);

    let mut live_parts: Vec<String> = Vec::new();

    if let Ok(Some(row)) = state.auth.db().lock().get_terminal(term_id) {
        let record = row_to_record(row);
        for target in tmux_target_candidates(&record) {
            if tmux::target_alive(&target) {
                if let Ok(cap) = tmux::capture_pane(&target) {
                    live_parts.push(strip_ansi_escapes(&cap));
                }
                break;
            }
        }
    }

    if let Some(end) = state.terminals.buffer_offset(term_id) {
        if let Some(rows) = state.terminals.buffer_replay_range(term_id, 0, end) {
            let buf: String = rows.iter().map(|(_, d)| d.as_str()).collect();
            live_parts.push(buf);
        }
    }
    if let Some(live) = state.terminals.recent_output(term_id) {
        live_parts.push(live);
    }

    let mut text = fold_live_snapshot_parts(live_parts);

    if text.trim().is_empty() {
        if let Ok(Some(row)) = state.auth.db().lock().get_terminal(term_id) {
            let record = row_to_record(row);
            text = collect_persisted_scrollback(state, &record).0;
        }
        if text.trim().is_empty() {
            text = load_scrollback_for_replay(state, term_id);
        }
    }

    text
}

/// Tail of shell scrollback for Discord (redacted, ANSI-stripped, last `max_lines` lines).
pub fn discord_shell_snapshot_text(state: &AppState, term_id: Uuid, max_lines: usize) -> String {
    let raw = terminal_display_text(state, term_id);
    if raw.trim().is_empty() {
        tracing::warn!(terminal = %term_id, "discord snapshot: no scrollback from any source");
        return String::new();
    }
    let redacted = state.redactor.read().redact_text(&raw);
    let clean = prepare_snapshot_terminal_text(&redacted);
    let excerpt = tail_logical_lines(&clean, max_lines);
    if excerpt.trim().is_empty() {
        tracing::warn!(
            terminal = %term_id,
            raw_bytes = raw.len(),
            clean_bytes = clean.len(),
            "discord snapshot: content lost during normalization"
        );
    }
    excerpt
}

fn tail_logical_lines(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let take = lines.len().min(max_lines);
    let start = lines.len().saturating_sub(take);
    lines[start..].join("\n").trim_end().to_string()
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
