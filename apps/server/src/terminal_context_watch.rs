//! Push terminal cwd/git context over session realtime when it changes (replaces client polling).

use crate::state::AppState;
use crate::terminals::{self, TerminalWorkContext};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

static LAST_PUBLISHED: std::sync::OnceLock<Mutex<HashMap<Uuid, TerminalWorkContext>>> =
    std::sync::OnceLock::new();

fn last_published() -> &'static Mutex<HashMap<Uuid, TerminalWorkContext>> {
    LAST_PUBLISHED.get_or_init(|| Mutex::new(HashMap::new()))
}

fn context_changed(prev: Option<&TerminalWorkContext>, next: &TerminalWorkContext) -> bool {
    match prev {
        None => next.cwd.is_some() || next.git_project.is_some() || next.git_branch.is_some(),
        Some(p) => {
            p.cwd != next.cwd || p.git_project != next.git_project || p.git_branch != next.git_branch
        }
    }
}

pub fn publish_terminal_context_if_changed(state: &AppState, terminal_id: Uuid) {
    let Some(session_id) = terminal_session_id(state, terminal_id) else {
        return;
    };
    let ctx = terminals::terminal_work_context_light(state, terminal_id);
    let mut last = last_published().lock();
    let prev = last.get(&terminal_id);
    if !context_changed(prev, &ctx) {
        return;
    }
    last.insert(terminal_id, ctx.clone());
    drop(last);
    terminals::update_terminal_context_cache(state, terminal_id, &ctx);
    state.realtime.publish(
        session_id,
        &serde_json::json!({
            "type": "terminal.context.changed",
            "terminalId": terminal_id.to_string(),
            "cwd": ctx.cwd,
            "gitProject": ctx.git_project,
            "gitBranch": ctx.git_branch,
        }),
    );
}

fn terminal_session_id(state: &AppState, terminal_id: Uuid) -> Option<Uuid> {
    if let Some(session_id) = state.terminal_sessions.read().get(&terminal_id) {
        return Some(*session_id);
    }
    state
        .auth
        .db()
        .lock()
        .get_terminal(terminal_id)
        .ok()
        .flatten()
        .map(|row| row.1)
}

fn watch_targets(state: &AppState) -> Vec<Uuid> {
    let mut ids: Vec<Uuid> = state.terminals.list_ids();
    if ids.is_empty() {
        ids = state
            .terminal_sessions
            .read()
            .keys()
            .copied()
            .collect();
    } else {
        for id in state.terminal_sessions.read().keys() {
            if !ids.contains(id) {
                ids.push(*id);
            }
        }
    }
    ids.sort_unstable();
    ids.dedup();
    ids
}

pub fn spawn_terminal_context_watcher(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let ids = watch_targets(&state);
            if ids.is_empty() {
                continue;
            }
            let state_bg = state.clone();
            let ids_bg = ids.clone();
            let result = tokio::task::spawn_blocking(move || {
                for id in ids_bg {
                    publish_terminal_context_if_changed(&state_bg, id);
                }
            })
            .await;
            if result.is_err() {
                tracing::warn!("terminal context watcher task failed");
            }
        }
    });
}

pub fn schedule_context_refresh_after_input(state: Arc<AppState>, terminal_id: Uuid) {
    tokio::spawn(async move {
        // Let the shell apply cd / git checkout before reading cwd.
        tokio::time::sleep(Duration::from_millis(450)).await;
        let state_bg = state.clone();
        let _ = tokio::task::spawn_blocking(move || {
            publish_terminal_context_if_changed(&state_bg, terminal_id);
        })
        .await;
    });
}

pub fn input_may_change_context(line: &str) -> bool {
    let line = line.trim();
    line == "cd"
        || line.starts_with("cd ")
        || line.starts_with("pushd ")
        || line.starts_with("source ")
        || line.contains("activate")
        || line.starts_with("deactivate")
        || line.starts_with("git checkout")
        || line.starts_with("git switch")
        || line.starts_with("git clone")
}
