use crate::state::AppState;
use bunny_discord::{
    AgentTask, AgentTaskMode, AgentTaskStatus, ApprovalRequest, DiscordThreadBinding,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn start_task(
    state: Arc<AppState>,
    session_id: Uuid,
    mode: AgentTaskMode,
    agent: &str,
    prompt: &str,
    discord_user_id: String,
    discord_thread_id: Option<String>,
    bunny_user_id: Uuid,
) -> Result<Uuid, anyhow::Error> {
    let task_id = Uuid::new_v4();
    let now = Utc::now();
    let task = AgentTask {
        id: task_id,
        session_id,
        source: "discord".into(),
        discord_thread_id: discord_thread_id.clone(),
        requested_by_discord_id: Some(discord_user_id),
        requested_by_user_id: Some(bunny_user_id),
        agent: agent.to_string(),
        mode,
        status: AgentTaskStatus::Queued,
        prompt: prompt.to_string(),
        created_at: now,
        updated_at: now,
    };
    state.discord.lock().create_task(&task)?;

    if let Some(thread_id) = discord_thread_id {
        let binding = DiscordThreadBinding {
            guild_id: String::new(),
            channel_id: String::new(),
            thread_id,
            session_id,
            task_id,
            default_shell_id: None,
            created_at: now,
        };
        state.discord.lock().bind_thread(&binding).ok();
    }

    let prompt_owned = prompt.to_string();
    let agent_owned = agent.to_string();
    tokio::spawn(async move {
        if let Err(e) = run_task(state.clone(), task_id, session_id, mode, &agent_owned, &prompt_owned).await {
            tracing::warn!(%task_id, error = %e, "discord agent task failed");
            let _ = state
                .discord
                .lock()
                .update_task_status(task_id, AgentTaskStatus::Failed);
        }
    });

    Ok(task_id)
}

async fn run_task(
    state: Arc<AppState>,
    task_id: Uuid,
    session_id: Uuid,
    mode: AgentTaskMode,
    agent: &str,
    prompt: &str,
) -> Result<(), anyhow::Error> {
    state
        .discord
        .lock()
        .update_task_status(task_id, AgentTaskStatus::Running)?;

    let wrapped = match mode {
        AgentTaskMode::Ask => format!("# Ask mode (read-only guidance)\n{prompt}"),
        AgentTaskMode::Plan => format!("# Plan mode (do not execute)\n{prompt}"),
        AgentTaskMode::Do => format!("# Agent mode\n{prompt}"),
        _ => prompt.to_string(),
    };

    let term_id = pick_or_create_shell(&state, session_id, "discord-agent")?;
    let cmd = format!(
        "claude --print \"{}\"\n",
        wrapped.replace('\\', "\\\\").replace('"', "\\\"")
    );

    if bunny_discord::risk::requires_approval(&cmd) {
        let approval_id = Uuid::new_v4();
        let req = ApprovalRequest {
            id: approval_id,
            task_id,
            session_id,
            action_summary: cmd.chars().take(200).collect(),
            reason: "Agent command requires approval".into(),
            status: "pending".into(),
            discord_message_id: None,
            created_at: Utc::now(),
            resolved_at: None,
        };
        state.discord.lock().create_approval(&req)?;
        state
            .discord
            .lock()
            .update_task_status(task_id, AgentTaskStatus::WaitingApproval)?;
        return Ok(());
    }

    state.terminals.write(term_id, &cmd)?;
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    state
        .discord
        .lock()
        .update_task_status(task_id, AgentTaskStatus::Done)?;
    let _ = agent;
    Ok(())
}

fn pick_or_create_shell(state: &AppState, session_id: Uuid, name: &str) -> Result<Uuid, anyhow::Error> {
    let auth_db = state.auth.db();
    let db = auth_db.lock();
    for (tid, sid) in state.terminal_sessions.read().iter() {
        if *sid != session_id {
            continue;
        }
        if let Ok(Some(row)) = db.get_terminal(*tid) {
            if row.2 == name {
                return Ok(*tid);
            }
        }
    }
    drop(db);
    let cwd = crate::terminals::default_shell_cwd();
    let secret_env = state.secret_env_for_session(session_id);
    let (term_id, tmux_target) = state.terminals.create(
        session_id,
        name,
        &cwd,
        None,
        80,
        24,
        secret_env,
    )?;
    state.terminal_sessions.write().insert(term_id, session_id);
    crate::terminals::persist_terminal(
        state,
        term_id,
        session_id,
        name,
        &state.config.terminal.shell,
        None,
        &cwd,
        80,
        24,
        tmux_target.as_deref(),
    )?;
    Ok(term_id)
}

pub fn cancel_task(state: &AppState, task_id: Uuid) -> Result<(), anyhow::Error> {
    state
        .discord
        .lock()
        .update_task_status(task_id, AgentTaskStatus::Cancelled)?;
    Ok(())
}

pub fn resolve_approval(
    state: &AppState,
    approval_id: Uuid,
    approve: bool,
    bunny_user_id: Uuid,
) -> Result<(), anyhow::Error> {
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
        let term_id = pick_or_create_shell(state, approval.session_id, "discord-agent")?;
        state.terminals.write(term_id, &format!("{}\n", task.prompt))?;
        state
            .discord
            .lock()
            .update_task_status(approval.task_id, AgentTaskStatus::Done)?;
    } else {
        state.discord.lock().resolve_approval(approval_id, "denied")?;
        state
            .discord
            .lock()
            .update_task_status(approval.task_id, AgentTaskStatus::Cancelled)?;
    }
    Ok(())
}
