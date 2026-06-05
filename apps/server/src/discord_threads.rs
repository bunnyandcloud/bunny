//! Discord thread ↔ shell ↔ Claude workflows (headless `claude -p`).

use crate::api::ApiError;
use crate::discord_claude::{run_thread_claude, run_thread_claude_with_answers};
use bunny_discord::AskUserQuestionItem;
use crate::discord_git::{init_thread_branch, probe_git_repo, reset_to_commit, run_git, sanitize_branch_token};
use crate::discord_ops::{audit, resolve_bunny_user, resolve_link, BridgeContext};
use crate::state::AppState;
use crate::terminals::{self, default_shell_cwd};
use axum::Json;
use bunny_discord::{
    DiscordThreadBinding, DiscordThreadDiscussion, DiscordThreadMessage, DiscordThreadMessageRole,
    DiscordThreadStatus,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

const THREAD_TRANSCRIPT_FETCH_LIMIT: usize = 50;

#[derive(Deserialize)]
pub struct ThreadBindRequest {
    #[serde(flatten)]
    pub ctx: BridgeContext,
    pub thread_id: String,
    pub goal_text: String,
    pub shell_name: Option<String>,
}

#[derive(Serialize)]
pub struct ThreadClaudeApiFields {
    pub response_text: String,
    pub needs_approval: bool,
    pub approval_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_question_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_questions: Option<Vec<AskUserQuestionItem>>,
}

fn thread_response_from_run(run: &crate::discord_claude::ThreadClaudeResult) -> ThreadClaudeApiFields {
    ThreadClaudeApiFields {
        response_text: run.output.clone(),
        needs_approval: run.needs_approval,
        approval_summary: run.approval_summary.clone(),
        pending_question_id: run.pending_question_id.map(|u| u.to_string()),
        pending_questions: run.pending_questions.clone(),
    }
}

#[derive(Serialize)]
pub struct ThreadBindResponse {
    pub thread_id: String,
    pub term_id: String,
    pub shell_name: String,
    pub project_cwd: String,
    pub git_enabled: bool,
    pub thread_branch: Option<String>,
    pub base_branch: Option<String>,
    #[serde(flatten)]
    pub claude: ThreadClaudeApiFields,
}

#[derive(Serialize)]
pub struct ThreadInputResponse {
    #[serde(flatten)]
    pub claude: ThreadClaudeApiFields,
}

#[derive(Deserialize)]
pub struct ThreadIdRequest {
    #[serde(flatten)]
    #[allow(dead_code)]
    pub ctx: BridgeContext,
    pub thread_id: String,
}

#[derive(Deserialize)]
pub struct ThreadInputRequest {
    #[serde(flatten)]
    pub ctx: BridgeContext,
    pub thread_id: String,
    pub text: String,
    pub discord_message_id: Option<String>,
    pub author_name: Option<String>,
    #[serde(default)]
    pub include_discussion_context: bool,
}

#[derive(Deserialize)]
pub struct ThreadAnswerRequest {
    #[serde(flatten)]
    pub ctx: BridgeContext,
    pub thread_id: String,
    pub pending_id: String,
    pub question_index: usize,
    pub answer_label: String,
}

#[derive(Serialize)]
pub struct ThreadAnswerResponse {
    pub complete: bool,
    pub answered_count: usize,
    pub total: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_questions: Option<Vec<AskUserQuestionItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_question_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claude: Option<ThreadClaudeApiFields>,
}

#[derive(Deserialize)]
pub struct ThreadDiscussionRequest {
    #[serde(flatten)]
    pub ctx: BridgeContext,
    pub thread_id: String,
    pub content: String,
    pub author_name: Option<String>,
}

#[derive(Deserialize)]
pub struct ThreadFinalizeRequest {
    #[serde(flatten)]
    pub ctx: BridgeContext,
    pub thread_id: String,
    pub outcome: String,
}

#[derive(Serialize)]
pub struct ThreadFinalizeResponse {
    pub status: String,
    pub git_instructions: Option<String>,
}

#[derive(Deserialize)]
pub struct ProjectPathRequest {
    #[serde(flatten)]
    pub ctx: BridgeContext,
    pub path: Option<String>,
}

#[derive(Deserialize)]
pub struct GitCommandRequest {
    #[serde(flatten)]
    pub ctx: BridgeContext,
    pub subcommand: String,
    pub branch: Option<String>,
    pub path: Option<String>,
    pub thread_id: Option<String>,
}

const THREAD_ATTACHMENTS_MAX_BYTES: usize = 12 * 1024 * 1024;

pub fn resolve_project_cwd(
    state: &AppState,
    guild_id: &str,
    channel_id: &str,
    session_id: Uuid,
) -> Result<PathBuf, ApiError> {
    let link = state
        .discord
        .lock()
        .get_session_link(guild_id, channel_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?
        .ok_or_else(|| ApiError::not_found("channel not linked"))?;

    if let Some(ref override_path) = link.project_cwd_override {
        return validate_project_dir(Path::new(override_path));
    }

    let session_path = state
        .auth
        .db()
        .lock()
        .get_stream_session_project_path(session_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?
        .filter(|s| !s.is_empty());

    if let Some(p) = session_path {
        return validate_project_dir(Path::new(&p));
    }

    validate_project_dir(&default_shell_cwd())
}

fn validate_project_dir(path: &Path) -> Result<PathBuf, ApiError> {
    let canon = std::fs::canonicalize(path)
        .map_err(|_| ApiError::validation(&format!("project path not found: {}", path.display())))?;
    if !canon.is_dir() {
        return Err(ApiError::validation("project path is not a directory"));
    }
    Ok(canon)
}

pub async fn internal_thread_bind(
    state: Arc<AppState>,
    body: ThreadBindRequest,
) -> Result<Json<ThreadBindResponse>, ApiError> {
    let link = resolve_link(&state, &body.ctx)?;
    let bunny_user = resolve_bunny_user(&state, &body.ctx)?;
    crate::discord_ops::ensure_discord_control(&state, bunny_user, link.session_id)?;

    if !crate::claude::is_installed() {
        return Err(ApiError::validation(
            "Claude Code is not installed on the agent — use Web UI ?claude=setup first",
        ));
    }

    let project_cwd = resolve_project_cwd(
        &state,
        &body.ctx.guild_id,
        &body.ctx.channel_id,
        link.session_id,
    )?;
    let project_cwd_str = project_cwd.to_string_lossy().into_owned();

    let git_probe = probe_git_repo(&project_cwd);
    let mut thread_branch = None;
    let mut base_branch = git_probe.base_branch.clone();
    let mut start_commit = git_probe.start_commit.clone();

    if git_probe.enabled {
        let short = &body.thread_id[body.thread_id.len().saturating_sub(6)..];
        let branch_name = format!(
            "bunny/{}-{}",
            sanitize_branch_token(&body.ctx.channel_id),
            sanitize_branch_token(short)
        );
        match init_thread_branch(&project_cwd, &branch_name) {
            Ok((base, commit)) => {
                base_branch = Some(base);
                start_commit = Some(commit);
                thread_branch = Some(branch_name);
            }
            Err(e) => {
                tracing::warn!("thread branch init failed: {e}");
            }
        }
    }

    let shell_name = body
        .shell_name
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| format!("discord-{}", &body.thread_id[..body.thread_id.len().min(8)]));

    let term_id = create_shell_in_cwd(&state, link.session_id, &shell_name, &project_cwd)?;

    let task_id = Uuid::new_v4();
    let binding = DiscordThreadBinding {
        guild_id: body.ctx.guild_id.clone(),
        channel_id: body.ctx.channel_id.clone(),
        thread_id: body.thread_id.clone(),
        session_id: link.session_id,
        task_id,
        term_id,
        project_cwd: project_cwd_str.clone(),
        status: DiscordThreadStatus::Active,
        goal_text: Some(body.goal_text.clone()),
        git_enabled: git_probe.enabled && thread_branch.is_some(),
        base_branch: base_branch.clone(),
        thread_branch: thread_branch.clone(),
        start_commit,
        last_pane_marker: 0,
        last_pane_snapshot: String::new(),
        last_input_discord_message_id: None,
        claude_session_id: None,
        created_at: Utc::now(),
    };
    state
        .discord
        .lock()
        .bind_thread(&binding)
        .map_err(|e| ApiError::validation(&e.to_string()))?;

    insert_thread_message(
        &state,
        &body.thread_id,
        DiscordThreadMessageRole::User,
        Some(&body.ctx.discord_user_id),
        None,
        &body.goal_text,
    )?;

    let run = execute_thread_claude(
        &state,
        &binding,
        &body.goal_text,
        "User",
        &[],
    )
    .await?;

    record_thread_claude_output(&state, &body.thread_id, &run)?;

    audit(
        &state,
        &body.ctx,
        link.session_id,
        "/discord/thread/bind",
        &body.goal_text,
        "ok",
        Some(bunny_user),
        Some(term_id),
        None,
    );

    Ok(Json(ThreadBindResponse {
        thread_id: body.thread_id,
        term_id: term_id.to_string(),
        shell_name,
        project_cwd: project_cwd_str,
        git_enabled: binding.git_enabled,
        thread_branch,
        base_branch,
        claude: thread_response_from_run(&run),
    }))
}

fn record_thread_claude_output(
    state: &AppState,
    thread_id: &str,
    run: &crate::discord_claude::ThreadClaudeResult,
) -> Result<(), ApiError> {
    if run.pending_question_id.is_some() {
        return Ok(());
    }
    if !run.needs_approval && !run.output.trim().is_empty() {
        insert_thread_message(
            state,
            thread_id,
            DiscordThreadMessageRole::Assistant,
            None,
            Some("bunny"),
            &run.output,
        )?;
    }
    Ok(())
}

async fn execute_thread_claude(
    state: &Arc<AppState>,
    binding: &DiscordThreadBinding,
    user_message: &str,
    author_label: &str,
    extra_transcript: &[DiscordThreadMessage],
) -> Result<crate::discord_claude::ThreadClaudeResult, ApiError> {
    let goal = binding
        .goal_text
        .as_deref()
        .unwrap_or(user_message)
        .to_string();
    let mut transcript = state
        .discord
        .lock()
        .list_thread_messages(&binding.thread_id, THREAD_TRANSCRIPT_FETCH_LIMIT)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    if !extra_transcript.is_empty() {
        transcript.extend_from_slice(extra_transcript);
    }
    let session_id = binding.session_id;
    let term_id = binding.term_id;
    let thread_id = binding.thread_id.clone();
    let project_cwd = binding.project_cwd.clone();
    let user_msg = user_message.to_string();
    let label = author_label.to_string();
    let state_bg = state.clone();
    tokio::task::spawn_blocking(move || {
        run_thread_claude(
            &state_bg,
            session_id,
            term_id,
            &thread_id,
            &goal,
            &project_cwd,
            &user_msg,
            &label,
            &transcript,
        )
    })
    .await
    .map_err(|e| ApiError::validation(&e.to_string()))?
    .map_err(|e| ApiError::validation(&e.to_string()))
}

fn insert_thread_message(
    state: &AppState,
    thread_id: &str,
    role: DiscordThreadMessageRole,
    discord_user_id: Option<&str>,
    author_name: Option<&str>,
    content: &str,
) -> Result<(), ApiError> {
    let msg = DiscordThreadMessage {
        id: Uuid::new_v4(),
        thread_id: thread_id.to_string(),
        role,
        discord_user_id: discord_user_id.map(str::to_string),
        author_name: author_name.map(str::to_string),
        content: content.to_string(),
        created_at: Utc::now(),
    };
    state
        .discord
        .lock()
        .insert_thread_message(&msg)
        .map_err(|e| ApiError::validation(&e.to_string()))
}

fn create_shell_in_cwd(
    state: &AppState,
    session_id: Uuid,
    name: &str,
    cwd: &Path,
) -> Result<Uuid, ApiError> {
    let rows = state
        .auth
        .db()
        .lock()
        .list_terminals_for_session(session_id)
        .unwrap_or_default();
    if rows.iter().any(|(_, _, existing, ..)| existing == name) {
        return Err(ApiError::validation(&format!(
            "shell name already exists: {name}"
        )));
    }
    let secret_env = state.secret_env_for_session(session_id);
    let (term_id, tmux_target) = state
        .terminals
        .create(session_id, name, cwd, None, 80, 24, secret_env)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    state.terminal_sessions.write().insert(term_id, session_id);
    crate::terminals::persist_terminal(
        state,
        term_id,
        session_id,
        name,
        &state.config.terminal.shell,
        None,
        cwd,
        80,
        24,
        tmux_target.as_deref(),
    )
    .map_err(|e| ApiError::validation(&e.to_string()))?;
    crate::terminals::notify_terminal_created(state, session_id, term_id, name);
    Ok(term_id)
}

pub async fn internal_thread_input(
    state: Arc<AppState>,
    body: ThreadInputRequest,
) -> Result<Json<ThreadInputResponse>, ApiError> {
    let binding = require_active_thread(&state, &body.thread_id)?;
    let link = resolve_link(&state, &body.ctx)?;
    let bunny_user = resolve_bunny_user(&state, &body.ctx)?;
    crate::discord_ops::ensure_discord_control(&state, bunny_user, link.session_id)?;

    if !crate::claude::is_installed() {
        return Err(ApiError::validation(
            "Claude Code is not installed on the agent — use Web UI ?claude=setup first",
        ));
    }

    let author_label = body
        .author_name
        .as_deref()
        .map(|n| format!("User ({n})"))
        .unwrap_or_else(|| "User".into());

    insert_thread_message(
        &state,
        &body.thread_id,
        DiscordThreadMessageRole::User,
        Some(&body.ctx.discord_user_id),
        body.author_name.as_deref(),
        &body.text,
    )?;

    let _ = state
        .discord
        .lock()
        .cancel_thread_pending_questions(&body.thread_id);

    let mut discussion_msgs = Vec::new();
    if body.include_discussion_context {
        let discussion = state
            .discord
            .lock()
            .list_thread_discussion(&body.thread_id, 20)
            .map_err(|e| ApiError::validation(&e.to_string()))?;
        for d in discussion {
            discussion_msgs.push(DiscordThreadMessage {
                id: d.id,
                thread_id: d.thread_id,
                role: DiscordThreadMessageRole::Discussion,
                discord_user_id: Some(d.discord_user_id),
                author_name: d.author_name,
                content: d.content,
                created_at: d.created_at,
            });
        }
    }

    let run = execute_thread_claude(
        &state,
        &binding,
        &body.text,
        &author_label,
        &discussion_msgs,
    )
    .await?;

    record_thread_claude_output(&state, &body.thread_id, &run)?;

    state
        .discord
        .lock()
        .set_thread_last_input_message(&body.thread_id, body.discord_message_id.as_deref())
        .map_err(|e| ApiError::validation(&e.to_string()))?;

    Ok(Json(ThreadInputResponse {
        claude: thread_response_from_run(&run),
    }))
}

pub async fn internal_thread_answer(
    state: Arc<AppState>,
    body: ThreadAnswerRequest,
) -> Result<Json<ThreadAnswerResponse>, ApiError> {
    let binding = require_active_thread(&state, &body.thread_id)?;
    let link = resolve_link(&state, &body.ctx)?;
    let bunny_user = resolve_bunny_user(&state, &body.ctx)?;
    crate::discord_ops::ensure_discord_control(&state, bunny_user, link.session_id)?;

    let pending_id = Uuid::parse_str(&body.pending_id)
        .map_err(|_| ApiError::validation("invalid pending_id"))?;

    let mut pending = state
        .discord
        .lock()
        .get_thread_pending_questions(pending_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?
        .ok_or_else(|| ApiError::not_found("pending questions not found"))?;

    if pending.thread_id != body.thread_id {
        return Err(ApiError::validation("pending_id does not match thread"));
    }

    let q = pending
        .questions
        .get(body.question_index)
        .ok_or_else(|| ApiError::validation("invalid question_index"))?;

    if !q
        .options
        .iter()
        .any(|o| o.label == body.answer_label)
    {
        return Err(ApiError::validation("invalid answer_label"));
    }

    pending
        .answers
        .insert(q.question.clone(), body.answer_label.clone());
    state
        .discord
        .lock()
        .update_thread_pending_answers(pending_id, &pending.answers)
        .map_err(|e| ApiError::validation(&e.to_string()))?;

    let total = pending.questions.len();
    let answered_count = pending.questions.iter().filter(|item| {
        pending.answers.contains_key(&item.question)
    }).count();

    if answered_count < total {
        let next_index = pending
            .questions
            .iter()
            .position(|item| !pending.answers.contains_key(&item.question))
            .unwrap_or(answered_count);
        return Ok(Json(ThreadAnswerResponse {
            complete: false,
            answered_count,
            total,
            pending_questions: Some(pending.questions.clone()),
            next_question_index: Some(next_index),
            claude: None,
        }));
    }

    let answers = pending.answers.clone();
    state
        .discord
        .lock()
        .delete_thread_pending_questions(pending_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?;

    let answer_note = pending
        .questions
        .iter()
        .map(|item| {
            format!(
                "Q: {}\nR: {}",
                item.question,
                answers.get(&item.question).unwrap_or(&body.answer_label)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    insert_thread_message(
        &state,
        &body.thread_id,
        DiscordThreadMessageRole::User,
        Some(&body.ctx.discord_user_id),
        None,
        &format!("(choix Discord)\n{answer_note}"),
    )?;

    let session_id = binding.session_id;
    let term_id = binding.term_id;
    let thread_id = binding.thread_id.clone();
    let state_bg = state.clone();
    let run = tokio::task::spawn_blocking(move || {
        run_thread_claude_with_answers(&state_bg, session_id, term_id, &thread_id, &answers)
    })
    .await
    .map_err(|e| ApiError::validation(&e.to_string()))??;

    record_thread_claude_output(&state, &body.thread_id, &run)?;

    Ok(Json(ThreadAnswerResponse {
        complete: true,
        answered_count: total,
        total,
        pending_questions: None,
        next_question_index: None,
        claude: Some(thread_response_from_run(&run)),
    }))
}

pub async fn internal_thread_discussion(
    state: Arc<AppState>,
    body: ThreadDiscussionRequest,
) -> Result<Json<serde_json::Value>, ApiError> {
    let _binding = require_active_thread(&state, &body.thread_id)?;
    let entry = DiscordThreadDiscussion {
        id: Uuid::new_v4(),
        thread_id: body.thread_id.clone(),
        discord_user_id: body.ctx.discord_user_id.clone(),
        author_name: body.author_name.clone(),
        content: body.content.clone(),
        created_at: Utc::now(),
    };
    state
        .discord
        .lock()
        .insert_thread_discussion(&entry)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    insert_thread_message(
        &state,
        &body.thread_id,
        DiscordThreadMessageRole::Discussion,
        Some(&body.ctx.discord_user_id),
        body.author_name.as_deref(),
        &body.content,
    )?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn internal_thread_stop(
    state: Arc<AppState>,
    body: ThreadIdRequest,
) -> Result<Json<serde_json::Value>, ApiError> {
    let _binding = require_active_thread(&state, &body.thread_id)?;
    let stopped = terminals::cancel_thread_claude_run(&state, &body.thread_id);
    Ok(Json(serde_json::json!({ "ok": true, "stopped": stopped })))
}

pub async fn internal_thread_finalize(
    state: Arc<AppState>,
    body: ThreadFinalizeRequest,
) -> Result<Json<ThreadFinalizeResponse>, ApiError> {
    let binding = state
        .discord
        .lock()
        .get_thread_binding(&body.thread_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?
        .ok_or_else(|| ApiError::not_found("thread not found"))?;

    let link = resolve_link(&state, &body.ctx)?;
    let bunny_user = resolve_bunny_user(&state, &body.ctx)?;
    crate::discord_ops::ensure_discord_control(&state, bunny_user, link.session_id)?;

    let outcome = body.outcome.to_lowercase();
    let cwd = PathBuf::from(&binding.project_cwd);

    if outcome == "cancel" && binding.git_enabled {
        if let Some(ref commit) = binding.start_commit {
            reset_to_commit(&cwd, commit)?;
        }
    }

    let _ = terminals::cancel_thread_claude_run(&state, &body.thread_id);
    close_thread_shell(&state, binding.term_id, link.session_id)?;

    let status = if outcome == "goal" {
        DiscordThreadStatus::Goal
    } else {
        DiscordThreadStatus::Cancelled
    };
    state
        .discord
        .lock()
        .update_thread_status(&body.thread_id, status)
        .map_err(|e| ApiError::validation(&e.to_string()))?;

    let git_instructions = if outcome == "goal" && binding.git_enabled {
        Some(format_goal_git_instructions(&binding))
    } else {
        None
    };

    Ok(Json(ThreadFinalizeResponse {
        status: status.as_str().into(),
        git_instructions,
    }))
}

fn format_goal_git_instructions(binding: &DiscordThreadBinding) -> String {
    let base = binding.base_branch.as_deref().unwrap_or("main");
    let branch = binding
        .thread_branch
        .as_deref()
        .unwrap_or("bunny/thread");
    format!(
        "**Git**\n- Base: `{base}`\n- Branch: `{branch}`\n```bash\ngit push -u origin {branch}\n```\nOuvre une PR de `{branch}` vers `{base}` sur ton forge (GitHub/GitLab)."
    )
}

fn close_thread_shell(state: &AppState, term_id: Uuid, session_id: Uuid) -> Result<(), ApiError> {
    state.terminals.remove(term_id);
    state.terminal_sessions.write().remove(&term_id);
    crate::terminals::remove_terminal_record(state, term_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    let _ = session_id;
    Ok(())
}

pub async fn internal_thread_status(
    state: Arc<AppState>,
    body: ThreadIdRequest,
) -> Result<Json<serde_json::Value>, ApiError> {
    let binding = state
        .discord
        .lock()
        .get_thread_binding(&body.thread_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?
        .ok_or_else(|| ApiError::not_found("thread not found"))?;
    Ok(Json(serde_json::json!({
        "thread_id": binding.thread_id,
        "status": binding.status.as_str(),
        "term_id": binding.term_id.to_string(),
        "project_cwd": binding.project_cwd,
        "git_enabled": binding.git_enabled,
        "goal_text": binding.goal_text,
        "thread_branch": binding.thread_branch,
        "base_branch": binding.base_branch,
        "last_input_discord_message_id": binding.last_input_discord_message_id,
    })))
}

pub async fn internal_project_set(
    state: Arc<AppState>,
    body: ProjectPathRequest,
) -> Result<Json<serde_json::Value>, ApiError> {
    let link = resolve_link(&state, &body.ctx)?;
    let bunny_user = resolve_bunny_user(&state, &body.ctx)?;
    crate::discord_ops::ensure_discord_control(&state, bunny_user, link.session_id)?;

    let path_opt = if let Some(ref p) = body.path {
        let validated = validate_project_dir(Path::new(p))?;
        Some(validated.to_string_lossy().into_owned())
    } else {
        None
    };

    state
        .discord
        .lock()
        .set_project_cwd_override(&body.ctx.guild_id, &body.ctx.channel_id, path_opt.as_deref())
        .map_err(|e| ApiError::validation(&e.to_string()))?;

    let cwd = resolve_project_cwd(
        &state,
        &body.ctx.guild_id,
        &body.ctx.channel_id,
        link.session_id,
    )?;
    let git = probe_git_repo(&cwd);

    Ok(Json(serde_json::json!({
        "project_cwd": cwd.to_string_lossy(),
        "git_enabled": git.enabled,
        "base_branch": git.base_branch,
    })))
}

pub async fn internal_project_get(
    state: Arc<AppState>,
    body: BridgeContext,
) -> Result<Json<serde_json::Value>, ApiError> {
    let link = resolve_link(&state, &body)?;
    let cwd = resolve_project_cwd(
        &state,
        &body.guild_id,
        &body.channel_id,
        link.session_id,
    )?;
    let git = probe_git_repo(&cwd);
    Ok(Json(serde_json::json!({
        "project_cwd": cwd.to_string_lossy(),
        "git_enabled": git.enabled,
        "base_branch": git.base_branch,
    })))
}

pub async fn internal_git_command(
    state: Arc<AppState>,
    body: GitCommandRequest,
) -> Result<Json<serde_json::Value>, ApiError> {
    let link = resolve_link(&state, &body.ctx)?;
    let bunny_user = resolve_bunny_user(&state, &body.ctx)?;
    crate::discord_ops::ensure_discord_control(&state, bunny_user, link.session_id)?;

    let cwd = if let Some(ref tid) = body.thread_id {
        let binding = state
            .discord
            .lock()
            .get_thread_binding(tid)
            .map_err(|e| ApiError::validation(&e.to_string()))?
            .ok_or_else(|| ApiError::not_found("thread not found"))?;
        PathBuf::from(binding.project_cwd)
    } else {
        resolve_project_cwd(
            &state,
            &body.ctx.guild_id,
            &body.ctx.channel_id,
            link.session_id,
        )?
    };

    let git = probe_git_repo(&cwd);
    if !git.enabled {
        return Err(ApiError::validation(&format!(
            "Aucun dépôt git dans `{}` — utilisez `/bunny project path:…` ou créez la session avec le bon project_path.",
            cwd.display()
        )));
    }

    let sub = body.subcommand.to_lowercase();
    let output = match sub.as_str() {
        "status" => run_git(&cwd, &["status", "-sb"])?,
        "diff" => {
            let mut args = vec!["diff"];
            if let Some(ref p) = body.path {
                args.push(p.as_str());
            }
            run_git(&cwd, &args)?
        }
        "log" => run_git(&cwd, &["log", "--oneline", "-n", "15"])?,
        "branch" => {
            let name = body.branch.as_deref().ok_or_else(|| ApiError::validation("branch required"))?;
            run_git(&cwd, &["checkout", "-b", name])?
        }
        "checkout" => {
            let name = body.branch.as_deref().ok_or_else(|| ApiError::validation("branch required"))?;
            run_git(&cwd, &["checkout", name])?
        }
        "merge" => {
            let name = body.branch.as_deref().ok_or_else(|| ApiError::validation("branch required"))?;
            let cmd = format!("git merge {name}");
            if bunny_discord::risk::requires_approval(&cmd) {
                return Ok(Json(serde_json::json!({
                    "needs_approval": true,
                    "command": cmd,
                })));
            }
            run_git(&cwd, &["merge", name])?
        }
        "reset_hard" => {
            let cmd = "git reset --hard";
            if bunny_discord::risk::requires_approval(cmd) {
                return Ok(Json(serde_json::json!({
                    "needs_approval": true,
                    "command": cmd,
                })));
            }
            run_git(&cwd, &["reset", "--hard"])?
        }
        _ => return Err(ApiError::validation(&format!("unknown git subcommand: {sub}"))),
    };

    Ok(Json(serde_json::json!({ "ok": true, "output": output })))
}

pub async fn internal_thread_attachment(
    state: Arc<AppState>,
    body: serde_json::Value,
) -> Result<Json<serde_json::Value>, ApiError> {
    let thread_id = body
        .get("thread_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::validation("thread_id"))?;
    let filename = body
        .get("filename")
        .and_then(|v| v.as_str())
        .unwrap_or("attachment");
    let content_b64 = body
        .get("content_base64")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::validation("content_base64"))?;

    let _binding = require_active_thread(&state, thread_id)?;
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(content_b64)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    if bytes.len() > THREAD_ATTACHMENTS_MAX_BYTES {
        return Err(ApiError::validation("attachment too large"));
    }

    let dir = PathBuf::from(state.config.expand_data_dir())
        .join("discord-attachments")
        .join(thread_id);
    std::fs::create_dir_all(&dir).map_err(|e| ApiError::validation(&e.to_string()))?;
    let safe_name = sanitize_branch_token(filename);
    let path = dir.join(&safe_name);
    std::fs::write(&path, &bytes).map_err(|e| ApiError::validation(&e.to_string()))?;

    Ok(Json(serde_json::json!({
        "ok": true,
        "path": path.to_string_lossy(),
    })))
}

fn require_active_thread(state: &AppState, thread_id: &str) -> Result<DiscordThreadBinding, ApiError> {
    let binding = state
        .discord
        .lock()
        .get_thread_binding(thread_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?
        .ok_or_else(|| ApiError::not_found("thread not bound"))?;
    if binding.status != DiscordThreadStatus::Active {
        return Err(ApiError::validation("thread is no longer active"));
    }
    Ok(binding)
}

