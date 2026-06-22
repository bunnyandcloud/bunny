//! Discord ↔ Claude Code: persistent sessions and interactive approvals.

use crate::api::ApiError;
use crate::discord_ops::{remember_discord_shell, resolve_discord_shell};
use crate::state::AppState;
use crate::task_runner::{shell_single_quote, wrap_prompt};
use crate::terminals;
use bunny_discord::{
    AgentTaskMode, AskUserQuestionItem, DiscordThreadMessage, DiscordThreadMessageRole,
    DiscordThreadPendingPermission, DiscordThreadPendingQuestions,
};
use std::collections::HashMap;
use bunny_pty::tmux;
use chrono::Utc;
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
    let term_id = resolve_discord_shell(
        &state,
        session_id,
        guild_id,
        channel_id,
        None,
        shell_name,
    )?;
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
        terminals::exec_discord_shell_command(state, term_id, session_id, &cmd, None)?;

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
    let parsed = parse_claude_json_for_discord(raw);
    (parsed.display_text, parsed.session_id)
}

/// Parsed `claude -p --output-format json` for Discord threads and slash commands.
#[derive(Debug, Clone)]
pub struct ClaudeJsonParse {
    pub display_text: String,
    pub session_id: Option<String>,
    pub ask_user_questions: Option<Vec<AskUserQuestionItem>>,
    pub bash_permission: Option<BashPermissionRequest>,
}

/// Bash command blocked by Claude Code in headless mode (`permission_denials`).
#[derive(Debug, Clone)]
pub struct BashPermissionRequest {
    pub command: String,
    pub allowed_tools: Vec<String>,
}

pub fn parse_claude_json_for_discord(raw: &str) -> ClaudeJsonParse {
    let trimmed = raw.trim();
    let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return ClaudeJsonParse {
            display_text: trimmed.to_string(),
            session_id: None,
            ask_user_questions: None,
            bash_permission: None,
        };
    };
    let session_id = v
        .get("session_id")
        .and_then(|s| s.as_str())
        .map(str::to_string);
    let ask_user_questions = extract_ask_user_questions_from_claude_json(&v);
    if let Some(ref questions) = ask_user_questions {
        let intro = format_pending_questions_intro(questions);
        return ClaudeJsonParse {
            display_text: intro,
            session_id,
            ask_user_questions: Some(questions.clone()),
            bash_permission: None,
        };
    }
    if let Some(bash) = extract_bash_permission_from_claude_json(&v) {
        return ClaudeJsonParse {
            display_text: format_pending_permission_intro(&bash.command),
            session_id,
            ask_user_questions: None,
            bash_permission: Some(bash),
        };
    }
    if claude_json_is_error(&v) {
        return ClaudeJsonParse {
            display_text: format_claude_error_message(&v, trimmed),
            session_id,
            ask_user_questions: None,
            bash_permission: None,
        };
    }
    let result = claude_json_result_text(&v).unwrap_or_else(|| trimmed.to_string());
    let bash_permission = extract_bash_permission_from_result_text(&result);
    ClaudeJsonParse {
        display_text: if bash_permission.is_some() {
            format_pending_permission_intro(&bash_permission.as_ref().unwrap().command)
        } else {
            result
        },
        session_id,
        ask_user_questions: None,
        bash_permission,
    }
}

fn format_pending_questions_intro(questions: &[AskUserQuestionItem]) -> String {
    let n = questions.len();
    if n > 1 {
        format!(
            "❓ **Claude a besoin de {n} choix** — répondez via les boutons (une question à la fois)."
        )
    } else {
        "❓ **Claude a besoin de votre choix** — utilisez les boutons ci-dessous.".to_string()
    }
}

fn format_pending_permission_intro(command: &str) -> String {
    format!(
        "🔐 **Claude demande l'autorisation d'exécuter une commande shell** — utilisez les boutons ci-dessous.\n```\n{command}\n```"
    )
}

/// Extract blocked Bash tool calls from Claude JSON (`permission_denials`).
pub fn extract_bash_permission_from_claude_json(
    v: &serde_json::Value,
) -> Option<BashPermissionRequest> {
    extract_bash_from_permission_denials(v)
}

fn extract_bash_from_permission_denials(v: &serde_json::Value) -> Option<BashPermissionRequest> {
    let denials = v.get("permission_denials")?.as_array()?;
    for d in denials {
        if d.get("tool_name").and_then(|t| t.as_str()) != Some("Bash") {
            continue;
        }
        let cmd = bash_command_from_denial(d)?;
        let allowed_tools = bash_allowed_tool_specs(&cmd);
        // One Discord prompt per turn — never join multiple denials (avoids giant `&&` chains).
        return Some(BashPermissionRequest {
            command: cmd,
            allowed_tools,
        });
    }
    None
}

/// Collapse whitespace for denied-command deduplication.
pub fn normalize_shell_command_key(cmd: &str) -> String {
    cmd.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Keys used to remember refused shell commands for a thread.
pub fn shell_command_denial_keys(command: &str) -> Vec<String> {
    let mut keys: Vec<String> = split_bash_compound(command)
        .into_iter()
        .map(|p| normalize_shell_command_key(&p))
        .filter(|k| !k.is_empty())
        .collect();
    let full = normalize_shell_command_key(command);
    if !full.is_empty() && !keys.iter().any(|k| k == &full) {
        keys.push(full);
    }
    keys.sort();
    keys.dedup();
    keys
}

pub fn format_denied_shell_command_message(command: &str) -> String {
    let preview = truncate_permission_command_display(command, 480);
    format!(
        "⛔ **Commande shell déjà refusée** — je ne redemanderai pas l'autorisation.\n\n\
         ```bash\n{preview}\n```\n\
         Répondez dans le fil (@bunny) pour indiquer comment continuer."
    )
}

pub fn truncate_permission_command_display(command: &str, max_chars: usize) -> String {
    let cmd = command.trim();
    let n = cmd.chars().count();
    if n <= max_chars {
        return cmd.to_string();
    }
    let truncated: String = cmd.chars().take(max_chars).collect();
    format!("{truncated}\n… ({n} caractères)")
}

fn bash_command_from_denial(d: &serde_json::Value) -> Option<String> {
    bash_command_from_tool_input(d.get("tool_input"))
}

fn bash_command_from_tool_input(input: Option<&serde_json::Value>) -> Option<String> {
    let input = input?;
    if let Some(cmd) = input.get("command").and_then(|c| c.as_str()) {
        let trimmed = cmd.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    if let Some(s) = input.as_str() {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

/// Build `--allowedTools` patterns for a compound Bash command.
pub fn bash_allowed_tool_specs(command: &str) -> Vec<String> {
    split_bash_compound(command)
        .into_iter()
        .map(|part| format!("Bash({part})"))
        .collect()
}

/// Fallback when Claude explains a blocked shell command in prose (no `permission_denials`).
fn extract_bash_permission_from_result_text(text: &str) -> Option<BashPermissionRequest> {
    let lower = text.to_lowercase();
    let permission_hint = lower.contains("autorisation")
        || lower.contains("approbation")
        || lower.contains("permission")
        || lower.contains("bloqu");
    let shell_hint = lower.contains("git add")
        || lower.contains("git commit")
        || lower.contains("bash");
    if !permission_hint || !shell_hint {
        return None;
    }
    let command = extract_bash_codeblock(text)?;
    let allowed_tools = bash_allowed_tool_specs(&command);
    if allowed_tools.is_empty() {
        return None;
    }
    Some(BashPermissionRequest {
        command,
        allowed_tools,
    })
}

fn extract_bash_codeblock(text: &str) -> Option<String> {
    for fence in ["```bash\n", "```sh\n", "```\n"] {
        if let Some(start) = text.find(fence) {
            let rest = &text[start + fence.len()..];
            if let Some(end) = rest.find("```") {
                let cmd = rest[..end].trim();
                if !cmd.is_empty() {
                    return Some(cmd.to_string());
                }
            }
        }
    }
    None
}

fn strip_outer_parens(s: &str) -> Option<&str> {
    let s = s.trim();
    if !s.starts_with('(') || !s.ends_with(')') {
        return None;
    }
    let inner = &s[1..s.len() - 1];
    let mut depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;
    for c in inner.chars() {
        match c {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '(' if !in_single && !in_double => depth += 1,
            ')' if !in_single && !in_double => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            _ => {}
        }
    }
    if depth != 0 || in_single || in_double {
        return None;
    }
    Some(inner)
}

fn split_bash_top_level<'a>(command: &'a str, sep: &str) -> Vec<&'a str> {
    if sep.is_empty() {
        return vec![command];
    }
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0usize;
    while i < command.len() {
        if !in_single && !in_double && depth == 0 && command[i..].starts_with(sep) {
            let part = command[start..i].trim();
            if !part.is_empty() {
                parts.push(part);
            }
            i += sep.len();
            start = i;
            continue;
        }
        let c = command[i..].chars().next().unwrap();
        match c {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '(' if !in_single && !in_double => depth += 1,
            ')' if !in_single && !in_double => depth -= 1,
            _ => {}
        }
        i += c.len_utf8();
    }
    let part = command[start..].trim();
    if !part.is_empty() {
        parts.push(part);
    }
    parts
}

fn split_bash_compound(command: &str) -> Vec<String> {
    let mut parts = vec![command.trim().to_string()];
    for sep in ["&&", "||", ";", "|"] {
        let mut next = Vec::new();
        for p in parts {
            for segment in split_bash_top_level(&p, sep) {
                let trimmed = segment.trim();
                if !trimmed.is_empty() {
                    next.push(trimmed.to_string());
                }
            }
        }
        parts = next;
    }
    parts
}

const MAX_SHELL_AUTO_CHAIN: u32 = 6;

/// Read-only / diagnostic shell snippets — auto-run without a Discord permission prompt.
pub fn is_auto_approved_shell_command(command: &str) -> bool {
    let c = command.trim();
    if c.is_empty() {
        return false;
    }
    if let Some(inner) = strip_outer_parens(c) {
        return is_auto_approved_shell_command(inner);
    }
    let alts = split_bash_top_level(c, "||");
    if alts.len() > 1 {
        return alts.iter().all(|alt| is_auto_approved_shell_command(alt));
    }
    let parts = split_bash_top_level(c, "&&");
    if parts.len() > 1 {
        return parts.iter().all(|p| is_auto_approved_shell_command(p));
    }
    is_shell_probe_part(c)
}

fn is_shell_probe_part(part: &str) -> bool {
    let p = part.trim();
    if p.is_empty() {
        return false;
    }
    if let Some(inner) = strip_outer_parens(p) {
        return is_auto_approved_shell_command(inner);
    }
    if p.starts_with("command -v ")
        || p.starts_with("which ")
        || p.starts_with("type ")
        || p == "pwd"
        || p == "ls"
        || p.starts_with("ls ")
        || p.starts_with("test -f ")
        || p.starts_with("test -d ")
        || p.starts_with("test -e ")
        || p.starts_with("python3 --version")
        || p.starts_with("node --version")
        || p.starts_with("git status")
        || p.starts_with("git log")
        || p.starts_with("git rev-parse")
    {
        return true;
    }
    if p.starts_with("echo ") && !p.contains('>') && !p.contains(">>") {
        return true;
    }
    false
}

fn build_thread_already_executed_resume_prompt(command: &str) -> String {
    let preview = truncate_permission_command_display(command, 400);
    format!(
        "# Rappel : commande déjà exécutée\n\n\
         ```bash\n{preview}\n```\n\n\
         Cette commande a **déjà été exécutée** après autorisation Discord. \
         **Ne la redemande pas.** Passe à l'étape suivante.\n"
    )
}

fn resume_claude_with_prompt(
    state: &AppState,
    thread_id: &str,
    term_id: Uuid,
    session_id: Uuid,
    prompt: &str,
    acting_user_id: Option<Uuid>,
    auto_depth: u32,
) -> Result<ThreadClaudeResult, ApiError> {
    if auto_depth >= MAX_SHELL_AUTO_CHAIN {
        return Ok(ThreadClaudeResult {
            output: "_(Trop d'étapes shell automatiques — répondez dans le fil pour continuer.)_".into(),
            exit_code: 0,
            needs_approval: false,
            approval_summary: None,
            pending_question_id: None,
            pending_questions: None,
            pending_permission_id: None,
            permission_command: None,
        });
    }
    let resume = state
        .discord
        .lock()
        .get_thread_claude_session_id(thread_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    let max_turns = thread_claude_max_turns(state);
    let cmd = build_thread_claude_cmd(prompt, resume.as_deref(), max_turns, &[]);
    let (raw, exit_code) = terminals::exec_discord_shell_command_for_thread(
        state, term_id, session_id, thread_id, &cmd, acting_user_id,
    )?;
    thread_claude_result_from_raw(
        state,
        thread_id,
        term_id,
        session_id,
        &raw,
        exit_code,
        || build_thread_claude_cmd(prompt, None, max_turns, &[]),
        auto_depth,
        acting_user_id,
    )
}

fn execute_thread_bash_and_resume(
    state: &AppState,
    thread_id: &str,
    term_id: Uuid,
    session_id: Uuid,
    command: &str,
    acting_user_id: Option<Uuid>,
    auto_depth: u32,
) -> Result<ThreadClaudeResult, ApiError> {
    let (shell_out, shell_code) = terminals::exec_discord_shell_command_for_thread(
        state, term_id, session_id, thread_id, command, acting_user_id,
    )
    .map_err(|e| ApiError::validation(&e.to_string()))?;
    if shell_code == 0 {
        let keys = shell_command_denial_keys(command);
        let _ = state
            .discord
            .lock()
            .record_thread_granted_shell_keys(thread_id, &keys);
    }
    let prompt = build_thread_permission_executed_prompt(command, shell_code, &shell_out);
    resume_claude_with_prompt(
        state,
        thread_id,
        term_id,
        session_id,
        &prompt,
        acting_user_id,
        auto_depth + 1,
    )
}

/// Extract `AskUserQuestion` items from Claude JSON (`permission_denials` or tool blocks).
pub fn extract_ask_user_questions_from_claude_json(
    v: &serde_json::Value,
) -> Option<Vec<AskUserQuestionItem>> {
    if let Some(q) = extract_ask_from_permission_denials(v) {
        return Some(q);
    }
    extract_ask_from_messages_array(v)
}

fn extract_ask_from_permission_denials(v: &serde_json::Value) -> Option<Vec<AskUserQuestionItem>> {
    let denials = v.get("permission_denials")?.as_array()?;
    let mut all = Vec::new();
    for d in denials {
        if d.get("tool_name").and_then(|t| t.as_str()) != Some("AskUserQuestion") {
            continue;
        }
        if let Some(q) = parse_ask_user_question_tool_input(d.get("tool_input")) {
            all.extend(q);
        }
    }
    if all.is_empty() {
        None
    } else {
        Some(dedupe_ask_questions(all))
    }
}

fn extract_ask_from_messages_array(v: &serde_json::Value) -> Option<Vec<AskUserQuestionItem>> {
    let messages = v.get("messages")?.as_array()?;
    let mut all = Vec::new();
    for msg in messages {
        let content = msg.get("content")?.as_array()?;
        for block in content {
            let tool_name = block
                .get("name")
                .or_else(|| block.get("tool_name"))
                .and_then(|t| t.as_str());
            if tool_name != Some("AskUserQuestion") {
                continue;
            }
            let input = block.get("input").or_else(|| block.get("tool_input"));
            if let Some(q) = parse_ask_user_question_tool_input(input) {
                all.extend(q);
            }
        }
    }
    if all.is_empty() {
        None
    } else {
        Some(dedupe_ask_questions(all))
    }
}

fn parse_ask_user_question_tool_input(input: Option<&serde_json::Value>) -> Option<Vec<AskUserQuestionItem>> {
    let input = input?;
    let arr = input.get("questions")?.as_array()?;
    let mut out = Vec::new();
    for qv in arr {
        if let Ok(item) = serde_json::from_value::<AskUserQuestionItem>(qv.clone()) {
            if !item.question.trim().is_empty() && !item.options.is_empty() {
                out.push(item);
            }
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn dedupe_ask_questions(items: Vec<AskUserQuestionItem>) -> Vec<AskUserQuestionItem> {
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for item in items {
        if seen.contains(&item.question) {
            continue;
        }
        seen.push(item.question.clone());
        out.push(item);
    }
    out
}

fn claude_json_is_error(v: &serde_json::Value) -> bool {
    if v.get("is_error").and_then(|b| b.as_bool()) == Some(true) {
        return true;
    }
    v.get("subtype")
        .and_then(|s| s.as_str())
        .is_some_and(|s| s.starts_with("error_"))
}

fn claude_json_result_text(v: &serde_json::Value) -> Option<String> {
    v.get("result").and_then(|r| match r {
        serde_json::Value::String(s) => Some(s.clone()),
        _ => Some(r.to_string()),
    })
}

fn format_claude_error_message(v: &serde_json::Value, raw_fallback: &str) -> String {
    let subtype = v
        .get("subtype")
        .and_then(|s| s.as_str())
        .unwrap_or("");
    if subtype == "error_max_turns" {
        if let Some(plan) = extract_plan_from_claude_error_json(v) {
            return format!(
                "⚠️ **Limite d'étapes agent atteinte** — plan partiel ci-dessous.\n\
                 _Relance avec un message plus court ou découpé si besoin._\n\n{plan}"
            );
        }
        let detail = v
            .get("errors")
            .and_then(|e| e.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "nombre max de tours dépassé".into());
        return format!(
            "⚠️ **Claude n'a pas terminé** ({detail}).\n\
             _Essayez une demande plus courte ou sans « mode plan » interactif._"
        );
    }
    if let Some(text) = claude_json_result_text(v) {
        if !text.trim().is_empty() {
            return text;
        }
    }
    if raw_fallback.len() > 800 {
        format!("⚠️ Erreur Claude (réponse tronquée) : {}…", &raw_fallback[..800])
    } else {
        format!("⚠️ Erreur Claude : {raw_fallback}")
    }
}

fn extract_plan_from_claude_error_json(v: &serde_json::Value) -> Option<String> {
    let denials = v.get("permission_denials")?.as_array()?;
    for d in denials {
        let tool = d.get("tool_name")?.as_str()?;
        if tool == "ExitPlanMode" {
            if let Some(plan) = d.get("tool_input")?.get("plan")?.as_str() {
                return Some(plan.to_string());
            }
        }
    }
    None
}

const THREAD_TRANSCRIPT_MAX_CHARS: usize = 10_000;
const THREAD_TRANSCRIPT_MAX_MESSAGES: usize = 50;

pub struct ThreadClaudeResult {
    pub output: String,
    #[allow(dead_code)]
    pub exit_code: i32,
    pub needs_approval: bool,
    pub approval_summary: Option<String>,
    pub pending_question_id: Option<Uuid>,
    pub pending_questions: Option<Vec<AskUserQuestionItem>>,
    pub pending_permission_id: Option<Uuid>,
    pub permission_command: Option<String>,
}

pub fn build_thread_claude_prompt(
    goal_text: &str,
    project_cwd: &str,
    transcript: &[DiscordThreadMessage],
    user_message: &str,
    author_label: &str,
) -> String {
    let history = truncate_thread_transcript(transcript);
    let mut prompt = String::from("# Agent mode (Discord thread)\n\n");
    prompt.push_str("# Goal du thread (contexte — référence pour orienter ton travail)\n");
    prompt.push_str(goal_text.trim());
    prompt.push_str("\n\n");
    if !history.is_empty() {
        prompt.push_str("# Contexte thread Discord (messages précédents)\n");
        prompt.push_str(&history);
        prompt.push_str("\n\n");
    }
    prompt.push_str("# Instruction\n");
    prompt.push_str(
        "Contraintes Discord (mode headless) :\n\
         - Réponds en texte markdown dans ta réponse finale.\n\
         - Pour un choix utilisateur (stack, options, préférences), utilise l'outil **AskUserQuestion** (boutons Discord).\n\
         - N'utilise PAS ExitPlanMode : si on demande un plan, écris-le en markdown dans la réponse.\n\
         - Ne pose pas de questions en texte libre sans AskUserQuestion.\n\n",
    );
    prompt.push_str("Réponds uniquement au dernier message utilisateur ci-dessous.\n");
    prompt.push_str(&format!("Travaille dans le cwd du projet: {project_cwd}\n"));
    prompt.push_str(
        "Ne déclare pas si le Goal est atteint ou non (pas de « Objectif atteint », « Goal reached », « pas encore atteint », etc.) — \
         c’est l’utilisateur qui clôture le thread via le bouton **Goal!** sur Discord quand il est satisfait.\n\n",
    );
    prompt.push_str("Dernier message:\n");
    prompt.push_str(&format!("{author_label}: {user_message}"));
    prompt
}

fn truncate_thread_transcript(messages: &[DiscordThreadMessage]) -> String {
    let start = messages.len().saturating_sub(THREAD_TRANSCRIPT_MAX_MESSAGES);
    let slice = &messages[start..];
    let mut lines = Vec::new();
    let mut total = 0usize;
    for msg in slice {
        let who = match msg.role {
            DiscordThreadMessageRole::Assistant => "Assistant (bunny)".to_string(),
            DiscordThreadMessageRole::Discussion => format!(
                "Discussion ({})",
                msg.author_name
                    .as_deref()
                    .unwrap_or(msg.discord_user_id.as_deref().unwrap_or("?"))
            ),
            DiscordThreadMessageRole::User => format!(
                "User ({})",
                msg.author_name
                    .as_deref()
                    .unwrap_or(msg.discord_user_id.as_deref().unwrap_or("?"))
            ),
        };
        let line = format!("{who}: {}", msg.content.trim());
        if total + line.len() > THREAD_TRANSCRIPT_MAX_CHARS {
            break;
        }
        total += line.len() + 1;
        lines.push(line);
    }
    lines.join("\n")
}

fn thread_claude_max_turns(state: &AppState) -> u32 {
    state.config.discord.claude_max_turns.max(1)
}

fn build_thread_claude_cmd(
    prompt: &str,
    resume: Option<&str>,
    max_turns: u32,
    allowed_tools: &[String],
) -> String {
    let mut cmd = format!(
        "claude -p --output-format json --permission-mode acceptEdits --max-turns {max_turns} ",
    );
    for tool in allowed_tools {
        cmd.push_str("--allowedTools ");
        cmd.push_str(&shell_single_quote(tool));
        cmd.push(' ');
    }
    if let Some(sid) = resume.filter(|s| !s.is_empty()) {
        cmd.push_str("--resume ");
        cmd.push_str(sid);
        cmd.push(' ');
    }
    cmd.push_str(&shell_single_quote(prompt));
    cmd
}

pub fn run_thread_claude(
    state: &AppState,
    session_id: Uuid,
    term_id: Uuid,
    thread_id: &str,
    goal_text: &str,
    project_cwd: &str,
    user_message: &str,
    author_label: &str,
    transcript: &[DiscordThreadMessage],
) -> Result<ThreadClaudeResult, ApiError> {
    let prompt = build_thread_claude_prompt(
        goal_text,
        project_cwd,
        transcript,
        user_message,
        author_label,
    );
    let cmd_probe = format!("claude -p {}", shell_single_quote(&prompt));
    if bunny_discord::risk::requires_approval(&cmd_probe) {
        return Ok(ThreadClaudeResult {
            output: String::new(),
            exit_code: 0,
            needs_approval: true,
            approval_summary: Some(cmd_probe.chars().take(200).collect()),
            pending_question_id: None,
            pending_questions: None,
            pending_permission_id: None,
            permission_command: None,
        });
    }

    let resume = state
        .discord
        .lock()
        .get_thread_claude_session_id(thread_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?;

    let max_turns = thread_claude_max_turns(state);
    let cmd = build_thread_claude_cmd(&prompt, resume.as_deref(), max_turns, &[]);
    let (raw, exit_code) =
        terminals::exec_discord_shell_command_for_thread(state, term_id, session_id, thread_id, &cmd, None)?;
    thread_claude_result_from_raw(
        state,
        thread_id,
        term_id,
        session_id,
        &raw,
        exit_code,
        || build_thread_claude_cmd(&prompt, None, max_turns, &[]),
        0,
        None,
    )
}

fn thread_claude_result_from_raw(
    state: &AppState,
    thread_id: &str,
    term_id: Uuid,
    session_id: Uuid,
    raw: &str,
    exit_code: i32,
    retry_without_resume: impl FnOnce() -> String,
    auto_depth: u32,
    acting_user_id: Option<Uuid>,
) -> Result<ThreadClaudeResult, ApiError> {
    let mut parsed = parse_claude_json_for_discord(raw);
    let mut effective_exit = exit_code;

    if parsed.ask_user_questions.is_none()
        && parsed.bash_permission.is_none()
        && (exit_code != 0 || parsed.display_text.trim().is_empty())
        && state
            .discord
            .lock()
            .get_thread_claude_session_id(thread_id)
            .map_err(|e| ApiError::validation(&e.to_string()))?
            .is_some()
    {
        let _ = state
            .discord
            .lock()
            .set_thread_claude_session_id(thread_id, None);
        let retry_cmd = retry_without_resume();
        let (raw2, exit2) = terminals::exec_discord_shell_command_for_thread(
            state, term_id, session_id, thread_id, &retry_cmd, None,
        )?;
        parsed = parse_claude_json_for_discord(&raw2);
        effective_exit = exit2;
    }

    if let Some(sid) = parsed.session_id.as_deref() {
        let _ = state
            .discord
            .lock()
            .set_thread_claude_session_id(thread_id, Some(sid));
    }

    if let Some(questions) = parsed.ask_user_questions.clone() {
        let pending_id = Uuid::new_v4();
        let pending = DiscordThreadPendingQuestions {
            id: pending_id,
            thread_id: thread_id.to_string(),
            questions,
            answers: HashMap::new(),
            created_at: Utc::now(),
        };
        state
            .discord
            .lock()
            .insert_thread_pending_questions(&pending)
            .map_err(|e| ApiError::validation(&e.to_string()))?;
        return Ok(ThreadClaudeResult {
            output: parsed.display_text,
            exit_code: effective_exit,
            needs_approval: false,
            approval_summary: None,
            pending_question_id: Some(pending_id),
            pending_questions: Some(pending.questions),
            pending_permission_id: None,
            permission_command: None,
        });
    }

    if let Some(bash) = parsed.bash_permission.clone() {
        let denial_keys = shell_command_denial_keys(&bash.command);
        let granted = state
            .discord
            .lock()
            .is_thread_shell_command_granted(thread_id, &denial_keys)
            .unwrap_or(false);
        if granted {
            return resume_claude_with_prompt(
                state,
                thread_id,
                term_id,
                session_id,
                &build_thread_already_executed_resume_prompt(&bash.command),
                acting_user_id,
                auto_depth + 1,
            );
        }
        let denied = state
            .discord
            .lock()
            .is_thread_shell_command_denied(thread_id, &denial_keys)
            .unwrap_or(false);
        if denied {
            return Ok(ThreadClaudeResult {
                output: format_denied_shell_command_message(&bash.command),
                exit_code: effective_exit,
                needs_approval: false,
                approval_summary: None,
                pending_question_id: None,
                pending_questions: None,
                pending_permission_id: None,
                permission_command: None,
            });
        }
        if is_auto_approved_shell_command(&bash.command) {
            return execute_thread_bash_and_resume(
                state,
                thread_id,
                term_id,
                session_id,
                &bash.command,
                acting_user_id,
                auto_depth,
            );
        }
        let pending_id = Uuid::new_v4();
        let pending = DiscordThreadPendingPermission {
            id: pending_id,
            thread_id: thread_id.to_string(),
            claude_session_id: parsed.session_id.clone(),
            command: bash.command.clone(),
            allowed_tools: bash.allowed_tools,
            created_at: Utc::now(),
        };
        state
            .discord
            .lock()
            .insert_thread_pending_permission(&pending)
            .map_err(|e| ApiError::validation(&e.to_string()))?;
        return Ok(ThreadClaudeResult {
            output: parsed.display_text,
            exit_code: effective_exit,
            needs_approval: false,
            approval_summary: None,
            pending_question_id: None,
            pending_questions: None,
            pending_permission_id: Some(pending_id),
            permission_command: Some(bash.command),
        });
    }

    let mut output = parsed.display_text;
    if output.trim().is_empty() && effective_exit != 0 {
        output = format!(
            "_(Claude a quitté avec le code {effective_exit} — voir le transcript du shell `discord-*` dans la Web UI)_"
        );
    }

    Ok(ThreadClaudeResult {
        output,
        exit_code: effective_exit,
        needs_approval: false,
        approval_summary: None,
        pending_question_id: None,
        pending_questions: None,
        pending_permission_id: None,
        permission_command: None,
    })
}

pub fn build_thread_permission_approved_prompt(command: &str) -> String {
    format!(
        "# Permission shell accordée (Discord)\n\n\
         L'utilisateur a **autorisé** l'exécution de la commande suivante. \
         Exécute-la maintenant, puis poursuis la tâche en cours.\n\n\
         ```bash\n{command}\n```\n"
    )
}

pub fn build_thread_permission_executed_prompt(command: &str, exit_code: i32, output: &str) -> String {
    let preview = truncate_permission_command_display(command, 600);
    let out = truncate_permission_command_display(output, 3000);
    if exit_code == 0 && output.contains(terminals::BUNNY_BACKGROUND_PID_MARKER) {
        format!(
            "# Serveur / processus lancé en arrière-plan (autorisé via Discord)\n\n\
             ```bash\n{preview}\n```\n\n\
             ```\n{out}\n```\n\n\
             Le processus **tourne en arrière-plan** — ne le relance pas. \
             Indique à l'utilisateur comment y accéder (URL, port) et confirme que c'est prêt.\n"
        )
    } else if exit_code == 0 {
        format!(
            "# Commande shell exécutée (autorisée via Discord)\n\n\
             ```bash\n{preview}\n```\n\n\
             Sortie (code 0) :\n```\n{out}\n```\n\n\
             La commande est **terminée** — ne la réexécute pas. Poursuis la tâche et réponds à l'utilisateur.\n"
        )
    } else {
        format!(
            "# Commande shell échouée (autorisée via Discord)\n\n\
             ```bash\n{preview}\n```\n\n\
             Code sortie : {exit_code}\n```\n{out}\n```\n\n\
             Explique l'erreur à l'utilisateur et propose la suite **sans relancer la même commande**.\n"
        )
    }
}

pub fn format_granted_shell_command_message(command: &str) -> String {
    let preview = truncate_permission_command_display(command, 480);
    format!(
        "✓ **Commande déjà exécutée** après votre autorisation.\n\n\
         ```bash\n{preview}\n```\n\
         Répondez dans le fil si vous voulez autre chose."
    )
}

pub fn build_thread_permission_denied_prompt(command: &str) -> String {
    let preview = truncate_permission_command_display(command, 600);
    format!(
        "# Permission shell refusée (Discord)\n\n\
         L'utilisateur a **refusé** l'exécution de :\n\n\
         ```bash\n{preview}\n```\n\n\
         **Ne réessaie pas** cette commande ni une variante équivalente (`git add` / `git commit` inclus). \
         Explique brièvement ce qui bloque et demande à l'utilisateur comment continuer.\n"
    )
}

/// Resume a thread Claude session after the user approved/denied a Bash permission on Discord.
pub fn run_thread_claude_after_permission(
    state: &AppState,
    session_id: Uuid,
    term_id: Uuid,
    thread_id: &str,
    approve: bool,
    command: &str,
    _allowed_tools: &[String],
    claude_session_id: Option<&str>,
    acting_user_id: Option<Uuid>,
) -> Result<ThreadClaudeResult, ApiError> {
    let _ = claude_session_id;
    let _ = _allowed_tools;

    if approve {
        return execute_thread_bash_and_resume(
            state,
            thread_id,
            term_id,
            session_id,
            command,
            acting_user_id,
            0,
        );
    }

    let prompt = build_thread_permission_denied_prompt(command);
    resume_claude_with_prompt(
        state,
        thread_id,
        term_id,
        session_id,
        &prompt,
        acting_user_id,
        0,
    )
}

pub fn build_thread_claude_answers_prompt(answers: &HashMap<String, String>) -> String {
    let mut prompt = String::from(
        "# Réponses utilisateur (AskUserQuestion)\n\n\
         L'utilisateur a répondu via Discord. Applique ces choix et poursuis la tâche.\n\n",
    );
    for (question, answer) in answers {
        prompt.push_str(&format!("**{question}**\n→ {answer}\n\n"));
    }
    prompt.push_str(
        "Ne repose pas les mêmes questions. Continue jusqu'à une réponse finale en markdown.\n",
    );
    prompt
}

/// Resume a thread Claude session after the user answered AskUserQuestion on Discord.
pub fn run_thread_claude_with_answers(
    state: &AppState,
    session_id: Uuid,
    term_id: Uuid,
    thread_id: &str,
    answers: &HashMap<String, String>,
) -> Result<ThreadClaudeResult, ApiError> {
    let resume = state
        .discord
        .lock()
        .get_thread_claude_session_id(thread_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    let prompt = build_thread_claude_answers_prompt(answers);
    let max_turns = thread_claude_max_turns(state);
    let cmd = build_thread_claude_cmd(&prompt, resume.as_deref(), max_turns, &[]);
    let (raw, exit_code) =
        terminals::exec_discord_shell_command_for_thread(state, term_id, session_id, thread_id, &cmd, None)?;
    thread_claude_result_from_raw(
        state,
        thread_id,
        term_id,
        session_id,
        &raw,
        exit_code,
        || build_thread_claude_cmd(&prompt, None, max_turns, &[]),
        0,
        None,
    )
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

pub(crate) fn pane_diff_since(before: &str, after: &str) -> String {
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

fn claude_pane_tail(text: &str, max_lines: usize) -> String {
    text.lines()
        .rev()
        .take(max_lines)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n")
}

fn claude_is_busy(text: &str) -> bool {
    // Only inspect the bottom of the pane — scrollback often contains "thinking"/"working" in help text.
    let tail = claude_pane_tail(text, 25).to_lowercase();
    tail.contains("esc to interrupt")
        || tail.contains("esc to cancel")
        || tail.contains("esc to stop")
}

fn claude_looks_idle(text: &str) -> bool {
    if claude_is_busy(text) {
        return false;
    }
    claude_pane_tail(text, 12).lines().any(|l| {
        let t = l.trim();
        t == ">" || t == "❯" || t.starts_with('>') || t.starts_with('❯')
    })
}

fn sanitize_claude_output(s: &str) -> String {
    let skip: &[&str] = &[
        "Try \"write a test",
        "? for shortcuts",
        "Opus 4.",
        "Auto mode is now",
        "Plugins in .claude",
        "/release-notes",
        "esc to interrupt",
        "/effort",
    ];
    s.lines()
        .filter(|line| {
            !skip.iter().any(|pat| line.contains(pat))
                && !is_claude_status_line(line)
                && !is_discord_noise_line(line)
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn is_decorative_separator(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 8
        && t.chars()
            .all(|c| matches!(c, '-' | '─' | '—' | '=' | '·' | '•' | '_' | ' '))
}

fn is_claude_status_line(line: &str) -> bool {
    let l = line.to_lowercase();
    let t = line.trim();
    l.contains("coalescing")
        || l.contains("slithering")
        || l.contains("esc to interrupt")
        || l.contains("thinking")
        || l.contains("working")
        || l.contains("spinner")
        || l.contains("/effort")
        || l.contains("tokens")
        || l.contains(" high - ")
        || l.contains(" · high ")
        || (t.starts_with('*') && t.len() < 80)
        || l.contains("sauté")
        || l.contains("sauteed")
        || l.contains("brewed")
        || l.contains("cogitated")
        || l.contains("cooked")
        || is_decorative_separator(t)
}

fn is_discord_noise_line(line: &str) -> bool {
    let l = line.to_lowercase();
    l.contains("focus-events")
        || l.contains("~/.tmux.conf")
        || l.contains("reloaded configuration")
        || l.contains("tmux version")
        || l.starts_with("[tmux]")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_ask_user_question_from_permission_denials() {
        let raw = r#"{
            "type": "result",
            "subtype": "error_max_turns",
            "session_id": "abc-session",
            "permission_denials": [{
                "tool_name": "AskUserQuestion",
                "tool_input": {
                    "questions": [{
                        "question": "Which stack?",
                        "header": "Stack",
                        "multiSelect": false,
                        "options": [
                            {"label": "Vite", "description": "Fast"},
                            {"label": "Next.js", "description": "Full"}
                        ]
                    }]
                }
            }]
        }"#;
        let v: serde_json::Value = serde_json::from_str(raw).unwrap();
        let q = extract_ask_user_questions_from_claude_json(&v).expect("questions");
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].question, "Which stack?");
        assert_eq!(q[0].options.len(), 2);
        assert_eq!(q[0].options[0].label, "Vite");

        let parsed = parse_claude_json_for_discord(raw);
        assert!(parsed.ask_user_questions.is_some());
        assert_eq!(parsed.session_id.as_deref(), Some("abc-session"));
        assert!(parsed.display_text.contains("Claude a besoin"));
    }

    #[test]
    fn builds_answers_resume_prompt() {
        let mut answers = HashMap::new();
        answers.insert("Which stack?".into(), "Vite".into());
        let p = build_thread_claude_answers_prompt(&answers);
        assert!(p.contains("Which stack?"));
        assert!(p.contains("Vite"));
    }

    #[test]
    fn build_thread_claude_cmd_includes_configurable_max_turns() {
        let cmd = build_thread_claude_cmd("hello", None, 30, &[]);
        assert!(cmd.contains("--max-turns 30"));
        assert!(cmd.ends_with("'hello'"));

        let resumed = build_thread_claude_cmd("go", Some("sess-1"), 25, &[]);
        assert!(resumed.contains("--max-turns 25"));
        assert!(resumed.contains("--resume sess-1"));

        let allowed = build_thread_claude_cmd(
            "go",
            Some("sess-1"),
            25,
            &["Bash(git add *)".into()],
        );
        assert!(allowed.contains("--allowedTools 'Bash(git add *)'"));
    }

    #[test]
    fn extracts_bash_permission_from_permission_denials() {
        let raw = r#"{
            "type": "result",
            "subtype": "error_during_execution",
            "session_id": "abc-session",
            "permission_denials": [{
                "tool_name": "Bash",
                "tool_input": { "command": "git add test.md && git commit -m \"essai\"" }
            }]
        }"#;
        let v: serde_json::Value = serde_json::from_str(raw).unwrap();
        let bash = extract_bash_permission_from_claude_json(&v).expect("bash");
        assert!(bash.command.contains("git add test.md"));
        assert!(bash.allowed_tools.iter().any(|t| t.contains("git add")));
        assert!(bash.allowed_tools.iter().any(|t| t.contains("git commit")));

        let parsed = parse_claude_json_for_discord(raw);
        assert!(parsed.bash_permission.is_some());
        assert!(parsed.display_text.contains("autorisation"));
    }

    #[test]
    fn bash_permission_uses_first_denial_only() {
        let raw = r#"{
            "permission_denials": [
                {"tool_name": "Bash", "tool_input": { "command": "git add side.md" }},
                {"tool_name": "Bash", "tool_input": { "command": "git commit -m \"add\"" }},
                {"tool_name": "Bash", "tool_input": { "command": "git commit -m \"again\"" }}
            ]
        }"#;
        let v: serde_json::Value = serde_json::from_str(raw).unwrap();
        let bash = extract_bash_permission_from_claude_json(&v).expect("bash");
        assert_eq!(bash.command, "git add side.md");
        assert!(!bash.command.contains("&&"));
    }

    #[test]
    fn shell_command_denial_keys_include_parts_and_full() {
        let keys = shell_command_denial_keys("git add a.md && git commit -m x");
        assert!(keys.contains(&"git add a.md".to_string()));
        assert!(keys.contains(&"git commit -m x".to_string()));
    }

    #[test]
    fn auto_approves_shell_probes() {
        assert!(is_auto_approved_shell_command(
            r#"(command -v python3 && echo "python3 ok") || echo "no python3""#
        ));
        assert!(is_auto_approved_shell_command("git status"));
        assert!(is_auto_approved_shell_command("ls -la"));
        assert!(!is_auto_approved_shell_command("python3 -m http.server 3000 --bind 0.0.0.0"));
        assert!(!is_auto_approved_shell_command("git add test.md"));
    }

    #[test]
    fn extracts_bash_permission_from_result_text_fallback() {
        let raw = r#"{
            "type": "result",
            "session_id": "sess-2",
            "result": "Le fichier est créé mais le commit nécessite une autorisation.\n\n```bash\ngit add test.md && git commit -m \"essai\"\n```"
        }"#;
        let parsed = parse_claude_json_for_discord(raw);
        let bash = parsed.bash_permission.expect("bash from prose");
        assert!(bash.command.contains("git add test.md"));
    }
}
