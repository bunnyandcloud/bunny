//! Discord ↔ Claude Code: persistent sessions and interactive approvals.

use crate::api::ApiError;
use crate::discord_ops::{remember_discord_shell, resolve_discord_shell};
use crate::state::AppState;
use crate::task_runner::{shell_single_quote, wrap_prompt};
use crate::terminals;
use bunny_discord::AgentTaskMode;
use bunny_pty::tmux;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub const CLAUDE_PANE_REASON_PREFIX: &str = "claude_pane:";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudePaneApprovalCtx {
    pub term_id: Uuid,
    pub pane_marker: usize,
    pub guild_id: String,
    pub channel_id: String,
}

pub struct DiscordClaudeRun {
    pub output: String,
    pub exit_code: i32,
    pub shell: String,
    pub needs_approval: bool,
    pub approval_summary: Option<String>,
    pub claude_pane_ctx: Option<ClaudePaneApprovalCtx>,
}

pub fn clear_claude_session(state: &AppState, guild_id: &str, channel_id: &str) -> Result<(), ApiError> {
    state
        .discord
        .lock()
        .set_claude_session_id(guild_id, channel_id, None)
        .map_err(|e| ApiError::validation(&e.to_string()))
}

pub async fn run_discord_claude(
    state: Arc<AppState>,
    session_id: Uuid,
    guild_id: &str,
    channel_id: &str,
    mode: AgentTaskMode,
    prompt: &str,
    shell_name: Option<&str>,
) -> Result<DiscordClaudeRun, ApiError> {
    let term_id = resolve_discord_shell(&state, session_id, guild_id, channel_id, shell_name)?;
    terminals::ensure_session_terminals_live(&state, session_id);

    let shell_label = state
        .auth
        .db()
        .lock()
        .get_terminal(term_id)
        .ok()
        .flatten()
        .map(|row| row.2)
        .unwrap_or_else(|| "shell".into());

    let state_bg = state.clone();
    let prompt_owned = prompt.to_string();
    let guild = guild_id.to_string();
    let channel = channel_id.to_string();
    let result = tokio::task::spawn_blocking(move || {
        run_claude_print_session(
            &state_bg,
            term_id,
            session_id,
            &guild,
            &channel,
            mode,
            &prompt_owned,
        )
    })
    .await
    .map_err(|e| ApiError::validation(&e.to_string()))??;

    remember_discord_shell(&state, guild_id, channel_id, term_id);

    Ok(DiscordClaudeRun {
        output: result.output,
        exit_code: result.exit_code,
        shell: shell_label,
        needs_approval: result.needs_approval,
        approval_summary: result.approval_summary,
        claude_pane_ctx: result.claude_pane_ctx,
    })
}

struct InnerRun {
    output: String,
    exit_code: i32,
    needs_approval: bool,
    approval_summary: Option<String>,
    claude_pane_ctx: Option<ClaudePaneApprovalCtx>,
}

fn run_claude_print_session(
    state: &AppState,
    term_id: Uuid,
    session_id: Uuid,
    guild_id: &str,
    channel_id: &str,
    mode: AgentTaskMode,
    prompt: &str,
) -> Result<InnerRun, ApiError> {
    let resume = state
        .discord
        .lock()
        .get_claude_session_id(guild_id, channel_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?;

    let wrapped = wrap_prompt(mode, prompt);
    let mut cmd = String::from("claude -p --output-format json");
    if let Some(extra) = claude_print_mode_flags(mode) {
        cmd.push(' ');
        cmd.push_str(extra);
    }
    if let Some(ref sid) = resume {
        if !sid.is_empty() {
            cmd.push_str(" --resume ");
            cmd.push_str(sid);
        }
    }
    cmd.push(' ');
    cmd.push_str(&shell_single_quote(&wrapped));

    let (raw, exit_code) =
        terminals::exec_discord_shell_command(state, term_id, session_id, &cmd)?;

    let (output, session_id) = parse_claude_json_output(&raw);
    if let Some(sid) = session_id {
        let _ = state
            .discord
            .lock()
            .set_claude_session_id(guild_id, channel_id, Some(&sid));
    }

    Ok(InnerRun {
        output,
        exit_code,
        needs_approval: false,
        approval_summary: None,
        claude_pane_ctx: None,
    })
}

/// Extra CLI flags per agent mode (`claude -p` non-interactive).
fn claude_print_mode_flags(mode: AgentTaskMode) -> Option<&'static str> {
    match mode {
        // Auto-approve file edits/writes so Discord does not stall on permission prompts.
        AgentTaskMode::Do => Some("--permission-mode acceptEdits"),
        _ => None,
    }
}

pub fn parse_claude_json_output(raw: &str) -> (String, Option<String>) {
    let trimmed = raw.trim();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if v.get("is_error").and_then(|b| b.as_bool()) == Some(true) {
            let msg = v
                .get("result")
                .and_then(|r| r.as_str())
                .unwrap_or(trimmed);
            return (msg.to_string(), v.get("session_id").and_then(|s| s.as_str()).map(str::to_string));
        }
        let result = v
            .get("result")
            .and_then(|r| r.as_str())
            .unwrap_or(trimmed)
            .to_string();
        let session_id = v
            .get("session_id")
            .and_then(|s| s.as_str())
            .map(str::to_string);
        return (result, session_id);
    }
    (trimmed.to_string(), None)
}

#[allow(dead_code)]
fn run_claude_tmux_interactive(
    state: &AppState,
    term_id: Uuid,
    _session_id: Uuid,
    guild_id: &str,
    channel_id: &str,
    prompt: &str,
) -> Result<InnerRun, ApiError> {
    let target = state
        .terminals
        .tmux_target(term_id)
        .ok_or_else(|| ApiError::validation("shell tmux session not available — open the shell in Web UI first"))?;

    ensure_claude_running(&target)?;
    wait_claude_input_ready(&target, Duration::from_secs(60))?;

    let baseline = capture_pane_stripped(&target).unwrap_or_default();
    let full_prompt = wrap_prompt(AgentTaskMode::Do, prompt);
    tmux::send_keys_literal(&target, &full_prompt, true)?;
    state.terminals.refresh_display(term_id);
    thread::sleep(Duration::from_millis(500));
    let after_send = capture_pane_stripped(&target).unwrap_or_else(|_| baseline.clone());

    match poll_claude_pane_diff(&target, &after_send, Duration::from_secs(175)) {
        PollOutcome::Complete(output) => Ok(InnerRun {
            output,
            exit_code: 0,
            needs_approval: false,
            approval_summary: None,
            claude_pane_ctx: None,
        }),
        PollOutcome::Permission { summary, pane_text } => {
            let summary = extract_permission_summary(&pane_text).unwrap_or(summary);
            Ok(InnerRun {
                output: summary.clone(),
                exit_code: 0,
                needs_approval: true,
                approval_summary: Some(summary.chars().take(500).collect()),
                claude_pane_ctx: Some(ClaudePaneApprovalCtx {
                    term_id,
                    pane_marker: after_send.len(),
                    guild_id: guild_id.to_string(),
                    channel_id: channel_id.to_string(),
                }),
            })
        }
        PollOutcome::Timeout(partial) => Ok(InnerRun {
            output: if partial.is_empty() {
                "_(timeout waiting for Claude — check the Web UI terminal)_".into()
            } else {
                format!("{partial}\n\n_(timeout — suite dans le terminal Web UI)_")
            },
            exit_code: 1,
            needs_approval: false,
            approval_summary: None,
            claude_pane_ctx: None,
        }),
    }
}

pub fn continue_claude_after_approval(
    state: &AppState,
    ctx: &ClaudePaneApprovalCtx,
    approve: bool,
) -> Result<(String, i32), ApiError> {
    let target = state
        .terminals
        .tmux_target(ctx.term_id)
        .ok_or_else(|| ApiError::validation("tmux session gone"))?;

    let key = if approve { "y" } else { "n" };
    tmux::send_keys_literal(&target, key, true)?;
    state.terminals.refresh_display(ctx.term_id);

    if !approve {
        return Ok((
            "Permission refusée — Claude n’a pas exécuté l’action.".into(),
            0,
        ));
    }

    let baseline = capture_pane_stripped(&target).unwrap_or_default();
    match poll_claude_pane_diff(&target, &baseline, Duration::from_secs(175)) {
        PollOutcome::Complete(output) => Ok((output, 0)),
        PollOutcome::Permission { summary, .. } => Ok((
            format!(
                "{summary}\n\n_(nouvelle permission requise — relancez /bunny do ou approuvez dans le terminal Web UI)_"
            ),
            0,
        )),
        PollOutcome::Timeout(partial) => Ok((
            if partial.is_empty() {
                "_(timeout après approbation — voir terminal Web UI)_".into()
            } else {
                format!("{partial}\n\n_(timeout)_")
            },
            1,
        )),
    }
}

pub fn encode_claude_pane_reason(ctx: &ClaudePaneApprovalCtx) -> String {
    format!(
        "{}{}",
        CLAUDE_PANE_REASON_PREFIX,
        serde_json::to_string(ctx).unwrap_or_default()
    )
}

pub fn decode_claude_pane_reason(reason: &str) -> Option<ClaudePaneApprovalCtx> {
    reason
        .strip_prefix(CLAUDE_PANE_REASON_PREFIX)
        .and_then(|json| serde_json::from_str(json).ok())
}

enum PollOutcome {
    Complete(String),
    Permission { summary: String, pane_text: String },
    Timeout(String),
}

fn capture_pane_stripped(target: &str) -> Result<String, ApiError> {
    tmux::capture_pane(target)
        .map(|s| terminals::strip_ansi_escapes(&s))
        .map_err(|e| ApiError::validation(&e.to_string()))
}

fn poll_claude_pane_diff(target: &str, baseline: &str, timeout: Duration) -> PollOutcome {
    let started = Instant::now();
    let mut last_delta = String::new();
    let mut stable_ticks = 0u32;
    let mut saw_busy = false;

    while started.elapsed() < timeout {
        thread::sleep(Duration::from_millis(500));
        let Ok(cap) = tmux::capture_pane(target) else {
            continue;
        };
        let text = terminals::strip_ansi_escapes(&cap);
        if pane_shows_permission(&text) {
            return PollOutcome::Permission {
                summary: "Claude demande une permission (outil / écriture fichier).".into(),
                pane_text: text,
            };
        }
        if claude_is_busy(&text) {
            saw_busy = true;
            stable_ticks = 0;
        }
        let delta = pane_diff_since(baseline, &text);
        if delta.is_empty() || is_claude_welcome_only(&delta) {
            continue;
        }
        if !saw_busy {
            continue;
        }
        if delta == last_delta {
            stable_ticks += 1;
            if stable_ticks >= 8 && claude_looks_idle(&text) {
                return PollOutcome::Complete(sanitize_claude_output(&delta));
            }
        } else {
            stable_ticks = 0;
            last_delta = delta;
        }
    }
    PollOutcome::Timeout(sanitize_claude_output(&last_delta))
}

fn pane_diff_since(before: &str, after: &str) -> String {
    let b: Vec<char> = before.chars().collect();
    let a: Vec<char> = after.chars().collect();
    let mut i = 0;
    while i < b.len() && i < a.len() && b[i] == a[i] {
        i += 1;
    }
    a[i..].iter().collect::<String>().trim().to_string()
}

fn is_claude_welcome_only(s: &str) -> bool {
    let lower = s.to_lowercase();
    let has_splash = lower.contains("try \"write a test")
        || lower.contains("? for shortcuts")
        || lower.contains("opus 4.");
    let has_substance = lower.contains("landing-page")
        || lower.contains("created")
        || lower.contains("wrote")
        || lower.contains("fichier")
        || s.lines().count() > 25;
    has_splash && !has_substance && s.len() < 1200
}

fn claude_is_busy(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("esc to interrupt")
        || lower.contains("thinking")
        || lower.contains("working")
        || lower.contains("spinner")
}

fn sanitize_claude_output(s: &str) -> String {
    let skip: &[&str] = &[
        "Try \"write a test",
        "? for shortcuts",
        "Opus 4.",
        "Auto mode is now",
        "Plugins in .claude",
        "/release-notes",
    ];
    s.lines()
        .filter(|line| !skip.iter().any(|pat| line.contains(pat)))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn wait_claude_input_ready(target: &str, timeout: Duration) -> Result<(), ApiError> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        thread::sleep(Duration::from_millis(500));
        let Ok(cap) = tmux::capture_pane(target) else {
            continue;
        };
        let text = terminals::strip_ansi_escapes(&cap);
        if claude_input_ready(&text) {
            return Ok(());
        }
    }
    Err(ApiError::validation(
        "Claude Code did not become ready in the shell — check the Web UI terminal",
    ))
}

fn claude_input_ready(text: &str) -> bool {
    let has_prompt = text.contains('❯')
        || text
            .lines()
            .last()
            .is_some_and(|l| l.trim_start().starts_with('>'));
    let splash = text.to_lowercase();
    let blocked = splash.contains("try \"write a test") && !splash.contains("esc to interrupt");
    has_prompt && !blocked
}

fn pane_shows_permission(text: &str) -> bool {
    let lower = text.to_lowercase();
    let tail: String = lower.chars().rev().take(1200).collect::<String>().chars().rev().collect();
    (tail.contains("allow") && (tail.contains("[y/n]") || tail.contains("(y/n)")))
        || tail.contains("do you want to allow")
        || tail.contains("permission to")
        || (tail.contains("write") && tail.contains("allow?"))
        || tail.contains("approuver")
        || tail.contains("autorisation")
}

fn claude_looks_idle(text: &str) -> bool {
    !claude_is_busy(text)
        && text
            .lines()
            .rev()
            .take(6)
            .any(|l| l.contains('❯') || l.trim_start().starts_with('>'))
}

fn extract_permission_summary(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let l = line.to_lowercase();
        if l.contains("allow") || l.contains("permission") || l.contains("autorisation") {
            let chunk: String = lines[i..].iter().take(12).copied().collect::<Vec<_>>().join("\n");
            return Some(chunk.chars().take(400).collect());
        }
    }
    None
}

fn ensure_claude_running(target: &str) -> Result<(), ApiError> {
    if claude_input_ready(
        &capture_pane_stripped(target).unwrap_or_default(),
    ) {
        return Ok(());
    }
    if !claude_already_running(target)? {
        tmux::send_keys_literal(target, "claude", true)
            .map_err(|e| ApiError::validation(&e.to_string()))?;
    }
    wait_claude_input_ready(target, Duration::from_secs(45))
}

fn claude_already_running(target: &str) -> Result<bool, ApiError> {
    let cap = tmux::capture_pane_visible(target).map_err(|e| ApiError::validation(&e.to_string()))?;
    let text = terminals::strip_ansi_escapes(&cap).to_lowercase();
    Ok(text.contains("claude code") || text.contains("claude ") || text.contains('❯'))
}
