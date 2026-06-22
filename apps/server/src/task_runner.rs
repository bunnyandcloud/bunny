use crate::api::ApiError;
use crate::discord_claude::{self, ClaudePaneApprovalCtx};
use crate::state::AppState;
use bunny_discord::{AgentTask, AgentTaskMode, AgentTaskStatus};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub struct AgentRunResult {
    pub output: String,
    pub exit_code: i32,
    pub shell: String,
    pub needs_approval: bool,
    pub approval_summary: Option<String>,
    pub claude_pane_ctx: Option<ClaudePaneApprovalCtx>,
}

pub async fn run_discord_agent(
    state: Arc<AppState>,
    session_id: Uuid,
    guild_id: &str,
    channel_id: &str,
    mode: AgentTaskMode,
    prompt: &str,
    shell_name: Option<&str>,
) -> Result<AgentRunResult, ApiError> {
    if !crate::claude::is_installed() {
        return Err(ApiError::validation(
            "Claude Code is not installed on the agent — use Web UI ?claude=setup first",
        ));
    }

    let wrapped = wrap_prompt(mode, prompt);
    let cmd_probe = format!("claude -p {}", shell_single_quote(&wrapped));
    if bunny_discord::risk::requires_approval(&cmd_probe) {
        let shell_label = "shell".to_string();
        return Ok(AgentRunResult {
            output: String::new(),
            exit_code: 0,
            shell: shell_label,
            needs_approval: true,
            approval_summary: Some(cmd_probe.chars().take(200).collect()),
            claude_pane_ctx: None,
        });
    }

    let run = discord_claude::run_discord_claude(
        state,
        session_id,
        guild_id,
        channel_id,
        mode,
        prompt,
        shell_name,
    )
    .await?;

    Ok(AgentRunResult {
        output: run.output,
        exit_code: run.exit_code,
        shell: run.shell,
        needs_approval: run.needs_approval,
        approval_summary: run.approval_summary,
        claude_pane_ctx: run.claude_pane_ctx,
    })
}

pub(crate) fn wrap_prompt(mode: AgentTaskMode, prompt: &str) -> String {
    match mode {
        AgentTaskMode::Ask => format!("# Ask mode (read-only guidance)\n{prompt}"),
        AgentTaskMode::Plan => format!("# Plan mode (do not execute)\n{prompt}"),
        AgentTaskMode::Do => format!("# Agent mode\n{prompt}"),
        _ => prompt.to_string(),
    }
}

pub(crate) fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

pub fn create_task_record(
    state: &AppState,
    session_id: Uuid,
    mode: AgentTaskMode,
    agent: &str,
    prompt: &str,
    discord_user_id: &str,
    bunny_user_id: Uuid,
) -> Result<Uuid, anyhow::Error> {
    let task_id = Uuid::new_v4();
    let now = Utc::now();
    let task = AgentTask {
        id: task_id,
        session_id,
        source: "discord".into(),
        discord_thread_id: None,
        requested_by_discord_id: Some(discord_user_id.to_string()),
        requested_by_user_id: Some(bunny_user_id),
        agent: agent.to_string(),
        mode,
        status: AgentTaskStatus::Queued,
        prompt: prompt.to_string(),
        created_at: now,
        updated_at: now,
    };
    state.discord.lock().create_task(&task)?;
    Ok(task_id)
}

pub fn cancel_task(state: &AppState, task_id: Uuid) -> Result<(), anyhow::Error> {
    state
        .discord
        .lock()
        .update_task_status(task_id, AgentTaskStatus::Cancelled)?;
    Ok(())
}

pub struct ApprovalResolveOutcome {
    pub output: Option<String>,
    pub exit_code: Option<i32>,
    pub shell: Option<String>,
    pub mode: Option<String>,
}

pub fn resolve_approval(
    state: &AppState,
    approval_id: Uuid,
    approve: bool,
    bunny_user_id: Uuid,
) -> Result<ApprovalResolveOutcome, anyhow::Error> {
    let approval = state
        .discord
        .lock()
        .get_approval(approval_id)?
        .ok_or_else(|| anyhow::anyhow!("approval not found"))?;
    let role = state.auth.member_role(approval.session_id, bunny_user_id)?;
    let role = role.ok_or_else(|| anyhow::anyhow!("not a member"))?;
    if !bunny_core::permissions::role_can(role, bunny_core::permissions::Action::DiscordApprove) {
        return Err(anyhow::anyhow!("cannot approve"));
    }

    if let Some(ctx) = discord_claude::decode_claude_pane_reason(&approval.reason) {
        state.discord.lock().resolve_approval(
            approval_id,
            if approve { "approved" } else { "denied" },
        )?;
        let (output, exit_code) = discord_claude::continue_claude_after_approval(state, &ctx, approve)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let status = if approve && exit_code == 0 {
            AgentTaskStatus::Done
        } else if approve {
            AgentTaskStatus::Failed
        } else {
            AgentTaskStatus::Cancelled
        };
        state.discord.lock().update_task_status(approval.task_id, status)?;
        let task = state
            .discord
            .lock()
            .get_task(approval.task_id)?
            .ok_or_else(|| anyhow::anyhow!("task missing"))?;
        let mode_label = match task.mode {
            AgentTaskMode::Ask => "ask",
            AgentTaskMode::Plan => "plan",
            AgentTaskMode::Do => "do",
            _ => "agent",
        };
        return Ok(ApprovalResolveOutcome {
            output: Some(output),
            exit_code: Some(exit_code),
            shell: None,
            mode: Some(mode_label.into()),
        });
    }

    if approve {
        state.discord.lock().resolve_approval(approval_id, "approved")?;
        state
            .discord
            .lock()
            .update_task_status(approval.task_id, AgentTaskStatus::Running)?;
        let task = state
            .discord
            .lock()
            .get_task(approval.task_id)?
            .ok_or_else(|| anyhow::anyhow!("task missing"))?;
        let term_id = crate::discord_ops::resolve_shell_terminal(state, approval.session_id, None)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let wrapped = wrap_prompt(task.mode, &task.prompt);
        let cmd = format!("claude -p --output-format json {}", shell_single_quote(&wrapped));
        let (output, exit_code) =
            crate::terminals::exec_discord_shell_command(state, term_id, approval.session_id, &cmd, None)?;
        state
            .discord
            .lock()
            .update_task_status(approval.task_id, if exit_code == 0 {
                AgentTaskStatus::Done
            } else {
                AgentTaskStatus::Failed
            })?;
        let (text, _) = discord_claude::parse_claude_json_output(&output);
        return Ok(ApprovalResolveOutcome {
            output: Some(text),
            exit_code: Some(exit_code),
            shell: None,
            mode: None,
        });
    }

    state.discord.lock().resolve_approval(approval_id, "denied")?;
    state
        .discord
        .lock()
        .update_task_status(approval.task_id, AgentTaskStatus::Cancelled)?;
    Ok(ApprovalResolveOutcome {
        output: None,
        exit_code: None,
        shell: None,
        mode: None,
    })
}
