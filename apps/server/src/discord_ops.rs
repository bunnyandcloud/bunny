use crate::api::ApiError;
use crate::compositor::{capture_snapshot, SnapshotTarget};
use crate::state::AppState;
use crate::task_runner;
use crate::watch;
use axum::{
    extract::{Extension, Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use bunny_auth::AuthenticatedSession;
use bunny_core::permissions::{role_can, Action};
use bunny_discord::{db::hash_token, AgentTaskMode, DiscordAuditEntry, DiscordSessionLink};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

pub fn human_router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/sessions/:id/discord/link-codes",
            post(create_discord_link_code),
        )
        .route(
            "/sessions/:id/discord/links",
            get(list_discord_links).delete(revoke_discord_links),
        )
        .route("/auth/discord/link", post(link_discord_user))
        .with_state(state)
}

pub fn internal_router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/link", post(internal_link))
        .route("/unlink", post(internal_unlink))
        .route("/status", get(internal_status))
        .route("/shell/list", get(internal_shell_list))
        .route("/shell/run", post(internal_shell_run))
        .route("/shell/new", post(internal_shell_new))
        .route("/shell/close", post(internal_shell_close))
        .route("/browser/open", post(internal_browser_open))
        .route("/browser/status", get(internal_browser_status))
        .route("/snapshot", post(internal_snapshot))
        .route("/stream/start", post(internal_stream_start))
        .route("/stream/stop", post(internal_stream_stop))
        .route("/stream/status", get(internal_stream_status))
        .route("/agent/ask", post(internal_agent_ask))
        .route("/agent/plan", post(internal_agent_plan))
        .route("/agent/do", post(internal_agent_do))
        .route("/task/stop", post(internal_task_stop))
        .route("/approval/resolve", post(internal_approval_resolve))
        .route("/follow/start", post(internal_follow_start))
        .route("/follow/stop", post(internal_follow_stop))
        .route("/audit", post(internal_audit))
        .with_state(state)
}

pub fn public_watch_router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/watch/:token", get(watch::get_watch_meta))
        .route("/watch/:token/access", post(watch::grant_watch_access))
        .route("/watch/:token/vnc/ws", get(watch::watch_novnc_ws))
        .route("/watch/:token/vnc", get(watch::watch_novnc_http_root))
        .route("/watch/:token/vnc/*path", get(watch::watch_novnc_http))
        .with_state(state)
}

pub fn verify_bridge_token(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    if !state.config.discord.enabled {
        return Err(ApiError::forbidden("discord integration disabled"));
    }
    let hash = state
        .config
        .discord
        .bridge_token_hash
        .as_deref()
        .ok_or_else(|| ApiError::forbidden("discord bridge not configured"))?;
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or_else(|| ApiError::forbidden("bridge token required"))?;
    if state.discord.lock().verify_bridge_token(token, hash) {
        Ok(())
    } else {
        Err(ApiError::forbidden("invalid bridge token"))
    }
}

#[derive(Deserialize)]
pub struct BridgeContext {
    pub guild_id: String,
    pub channel_id: String,
    pub thread_id: Option<String>,
    pub discord_user_id: String,
}

#[derive(Deserialize)]
pub struct InternalLinkRequest {
    #[serde(flatten)]
    pub ctx: BridgeContext,
    pub code: String,
}

#[derive(Serialize)]
pub struct InternalLinkResponse {
    pub session_id: String,
    pub session_name: Option<String>,
}

async fn internal_link(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<InternalLinkRequest>,
) -> Result<Json<InternalLinkResponse>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    let (session_id, created_by_user_id) = state
        .discord
        .lock()
        .consume_link_code(&body.code)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    state.discord.lock().record_installation(&body.ctx.guild_id, Some(&body.ctx.discord_user_id)).ok();
    state
        .discord
        .lock()
        .link_discord_user(&body.ctx.discord_user_id, created_by_user_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    state
        .discord
        .lock()
        .upsert_session_link(
            &body.ctx.guild_id,
            &body.ctx.channel_id,
            session_id,
            Some(created_by_user_id),
        )
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    audit(
        &state,
        &body.ctx,
        session_id,
        "/bunny link",
        "linked",
        "ok",
        Some(created_by_user_id),
        None,
        None,
    );
    Ok(Json(InternalLinkResponse {
        session_id: session_id.to_string(),
        session_name: None,
    }))
}

#[derive(Deserialize)]
pub struct InternalUnlinkRequest {
    #[serde(flatten)]
    pub ctx: BridgeContext,
}

async fn internal_unlink(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<InternalUnlinkRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    let removed = state
        .discord
        .lock()
        .remove_session_link(&body.ctx.guild_id, &body.ctx.channel_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": removed })))
}

async fn internal_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(ctx): Query<BridgeContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    let link = resolve_link(&state, &ctx)?;
    Ok(Json(serde_json::json!({
        "linked": true,
        "session_id": link.session_id.to_string(),
        "guild_id": link.guild_id,
        "channel_id": link.channel_id,
    })))
}

#[derive(Deserialize)]
pub struct ShellRunRequest {
    #[serde(flatten)]
    pub ctx: BridgeContext,
    pub command: String,
    pub shell_name: Option<String>,
}

async fn internal_shell_run(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<ShellRunRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    let link = resolve_link(&state, &body.ctx)?;
    let bunny_user = resolve_bunny_user(&state, &body.ctx)?;
    ensure_discord_control(&state, bunny_user, link.session_id)?;
    if bunny_discord::risk::requires_approval(&body.command) {
        return Ok(Json(serde_json::json!({
            "needs_approval": true,
            "command": body.command,
        })));
    }
    let term_id = resolve_shell_terminal(&state, link.session_id, body.shell_name.as_deref())?;
    crate::terminals::ensure_session_terminals_live(&state, link.session_id);
    let (output, exit_code) = capture_shell_run_output(
        Arc::clone(&state),
        link.session_id,
        term_id,
        &body.command,
    )
    .await?;
    let result = if exit_code == 0 { "ok" } else { "error" };
    audit(
        &state,
        &body.ctx,
        link.session_id,
        "/bunny shell run",
        &body.command,
        result,
        Some(bunny_user),
        Some(term_id),
        None,
    );
    let shell_name = state
        .auth
        .db()
        .lock()
        .get_terminal(term_id)
        .ok()
        .flatten()
        .map(|row| row.2);
    Ok(Json(serde_json::json!({
        "ok": exit_code == 0,
        "output": output,
        "exit_code": exit_code,
        "shell": shell_name,
    })))
}

#[derive(Deserialize)]
pub struct ShellNewRequest {
    #[serde(flatten)]
    pub ctx: BridgeContext,
    pub name: Option<String>,
}

async fn internal_shell_new(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<ShellNewRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    let link = resolve_link(&state, &body.ctx)?;
    let bunny_user = resolve_bunny_user(&state, &body.ctx)?;
    ensure_discord_control(&state, bunny_user, link.session_id)?;
    let session_id = link.session_id;
    let name = body
        .name
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| crate::terminals::next_shell_name(&state, session_id));
    {
        let rows = state
            .auth
            .db()
            .lock()
            .list_terminals_for_session(session_id)
            .unwrap_or_default();
        if rows.iter().any(|(_, _, existing, ..)| existing == &name) {
            return Err(ApiError::validation(&format!(
                "shell name already exists: {name}"
            )));
        }
    }
    let cwd = crate::terminals::default_shell_cwd();
    let secret_env = state.secret_env_for_session(session_id);
    let (term_id, tmux_target) = state
        .terminals
        .create(session_id, &name, &cwd, None, 80, 24, secret_env)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    state.terminal_sessions.write().insert(term_id, session_id);
    crate::terminals::persist_terminal(
        &state,
        term_id,
        session_id,
        &name,
        &state.config.terminal.shell,
        None,
        &cwd,
        80,
        24,
        tmux_target.as_deref(),
    )
    .map_err(|e| ApiError::validation(&e.to_string()))?;
    audit(
        &state,
        &body.ctx,
        session_id,
        "/bunny shell_new",
        &format!("create {name}"),
        "ok",
        Some(bunny_user),
        Some(term_id),
        None,
    );
    Ok(Json(serde_json::json!({
        "ok": true,
        "terminal_id": term_id.to_string(),
        "name": name,
    })))
}

#[derive(Deserialize)]
pub struct ShellCloseRequest {
    #[serde(flatten)]
    pub ctx: BridgeContext,
    pub shell_name: Option<String>,
}

async fn internal_shell_close(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<ShellCloseRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    let link = resolve_link(&state, &body.ctx)?;
    let bunny_user = resolve_bunny_user(&state, &body.ctx)?;
    ensure_discord_control(&state, bunny_user, link.session_id)?;
    let session_id = link.session_id;
    let rows = state
        .auth
        .db()
        .lock()
        .list_terminals_for_session(session_id)
        .unwrap_or_default();
    if rows.is_empty() {
        return Err(ApiError::not_found("no shell in this session"));
    }
    let shell_name = if let Some(name) = body
        .shell_name
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
    {
        name
    } else if rows.len() == 1 {
        rows[0].2.clone()
    } else {
        return Err(ApiError::validation(
            "multiple shells — specify shell: <name> (see shell_list)",
        ));
    };
    let term_id = resolve_shell_terminal(&state, session_id, Some(&shell_name))?;
    let display_name = state
        .auth
        .db()
        .lock()
        .get_terminal(term_id)
        .ok()
        .flatten()
        .map(|row| row.2)
        .unwrap_or(shell_name);
    state.terminals.remove(term_id);
    state.terminal_sessions.write().remove(&term_id);
    crate::terminals::remove_terminal_record(&state, term_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    audit(
        &state,
        &body.ctx,
        session_id,
        "/bunny shell_close",
        &format!("close {display_name}"),
        "ok",
        Some(bunny_user),
        Some(term_id),
        None,
    );
    Ok(Json(serde_json::json!({
        "ok": true,
        "terminal_id": term_id.to_string(),
        "name": display_name,
    })))
}

async fn internal_shell_list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(ctx): Query<BridgeContext>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    let link = resolve_link(&state, &ctx)?;
    let rows = state
        .auth
        .db()
        .lock()
        .list_terminals_for_session(link.session_id)
        .unwrap_or_default();
    let default_name = rows.first().map(|(_, _, name, ..)| name.clone());
    let items: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|row| {
            let (id, _, name, _, _, _, status, _, _, _) = row;
            serde_json::json!({
                "id": id.to_string(),
                "name": name,
                "status": status,
                "default": default_name.as_deref() == Some(name.as_str()),
            })
        })
        .collect();
    Ok(Json(items))
}

#[derive(Deserialize)]
pub struct BrowserOpenRequest {
    #[serde(flatten)]
    pub ctx: BridgeContext,
    pub url: String,
}

async fn internal_browser_open(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<BrowserOpenRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    let link = resolve_link(&state, &body.ctx)?;
    let bunny_user = resolve_bunny_user(&state, &body.ctx)?;
    ensure_discord_control(&state, bunny_user, link.session_id)?;
    let browser_id =
        crate::browser_ops::find_or_create_browser(Arc::clone(&state), link.session_id, &body.url).await?;
    Ok(Json(serde_json::json!({ "browser_id": browser_id.to_string() })))
}

async fn internal_browser_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(ctx): Query<BridgeContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    let link = resolve_link(&state, &ctx)?;
    let browsers: Vec<_> = state
        .browser_sessions
        .read()
        .iter()
        .filter(|(_, sid)| **sid == link.session_id)
        .map(|(id, _)| id.to_string())
        .collect();
    Ok(Json(serde_json::json!({ "browser_ids": browsers })))
}

#[derive(Deserialize)]
pub struct SnapshotRequest {
    #[serde(flatten)]
    pub ctx: BridgeContext,
    pub target: Option<String>,
    pub shell_name: Option<String>,
    /// Start headless Chromium before capture (for browser / full snapshots).
    #[serde(default)]
    pub ensure_browser: bool,
    pub browser_url: Option<String>,
}

fn default_browser_url(state: &AppState, session_id: Uuid) -> String {
    state
        .previews
        .read()
        .values()
        .find(|p| p.session_id == session_id)
        .map(|p| format!("http://127.0.0.1:{}", p.local_port))
        .unwrap_or_else(|| "http://127.0.0.1:3000".into())
}

fn resolve_shell_label(
    state: &AppState,
    session_id: Uuid,
    shell_name: Option<&str>,
) -> Result<String, ApiError> {
    let term_id = resolve_shell_terminal(state, session_id, shell_name)?;
    Ok(shell_name
        .map(str::to_string)
        .or_else(|| state.terminals.name(term_id))
        .unwrap_or_else(|| term_id.to_string()))
}

fn discord_snapshot_caption(
    target: SnapshotTarget,
    shell_label: &str,
    snap: &crate::compositor::SnapshotResult,
) -> String {
    match target {
        SnapshotTarget::Shell => format!("Shell snapshot - {shell_label}"),
        SnapshotTarget::Browser => "Browser snapshot".into(),
        SnapshotTarget::All => {
            if snap.caption.contains("browser unavailable") {
                format!("Full snapshot - shell: {shell_label} (browser unavailable)")
            } else {
                format!("Full snapshot - shell: {shell_label} + browser")
            }
        }
    }
}

async fn internal_snapshot(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<SnapshotRequest>,
) -> Result<impl IntoResponse, ApiError> {
    verify_bridge_token(&state, &headers)?;
    let link = resolve_link(&state, &body.ctx)?;
    let target = match body.target.as_deref() {
        Some("browser") => SnapshotTarget::Browser,
        Some("shell") => SnapshotTarget::Shell,
        Some("all") => SnapshotTarget::All,
        _ => SnapshotTarget::Shell,
    };

    let needs_browser = body.ensure_browser
        || matches!(target, SnapshotTarget::Browser | SnapshotTarget::All);
    if needs_browser {
        let url = body
            .browser_url
            .clone()
            .unwrap_or_else(|| default_browser_url(&state, link.session_id));
        crate::browser_ops::find_or_create_browser(Arc::clone(&state), link.session_id, &url).await?;
        // Let Chromium paint before CDP screenshot.
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }

    let shell_label = resolve_shell_label(&state, link.session_id, body.shell_name.as_deref())?;

    let snap = capture_snapshot(
        &state,
        link.session_id,
        target,
        body.shell_name.as_deref(),
    )
    .await
    .map_err(|e| ApiError::validation(&e.to_string()))?;
    let discord_caption = discord_snapshot_caption(target, &shell_label, &snap);
    audit(
        &state,
        &body.ctx,
        link.session_id,
        "/bunny snapshot",
        &discord_caption,
        "ok",
        None,
        None,
        None,
    );
    let caption_header = axum::http::HeaderValue::from_str(&discord_caption)
        .unwrap_or_else(|_| {
            axum::http::HeaderValue::from_str(&format!("Shell snapshot - {shell_label}"))
                .unwrap_or_else(|_| axum::http::HeaderValue::from_static("Snapshot"))
        });
    let mut response = axum::response::Response::new(axum::body::Body::from(snap.png));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("image/png"),
    );
    response.headers_mut().insert(
        axum::http::HeaderName::from_static("x-bunny-snapshot-caption"),
        caption_header,
    );
    Ok(response)
}

#[derive(Deserialize)]
pub struct StreamStartRequest {
    #[serde(flatten)]
    pub ctx: BridgeContext,
    pub layout: Option<String>,
    pub visibility: Option<String>,
    pub ttl_hours: Option<u64>,
    pub browser_url: Option<String>,
    /// When true, watch link allows mouse/keyboard (noVNC interactive). Default: read-only.
    #[serde(default)]
    pub interactive: bool,
}

async fn internal_stream_start(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<StreamStartRequest>,
) -> Result<Json<watch::WatchLinkResponse>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    let link = resolve_link(&state, &body.ctx)?;
    let url = body
        .browser_url
        .clone()
        .unwrap_or_else(|| default_browser_url(&state, link.session_id));
    let browser_id =
        crate::browser_ops::find_or_create_browser(Arc::clone(&state), link.session_id, &url).await?;
    audit(
        &state,
        &body.ctx,
        link.session_id,
        "/bunny stream_browser_start",
        &format!(
            "browser stream at {url} ({})",
            if body.interactive {
                "interactive"
            } else {
                "read-only"
            }
        ),
        "ok",
        None,
        None,
        Some(browser_id),
    );
    watch::create_watch_link(
        &state,
        &body.ctx,
        link.session_id,
        browser_id,
        body.layout,
        body.visibility,
        body.ttl_hours,
        body.interactive,
    )
    .await
}

async fn internal_stream_stop(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<BridgeContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    let stopped = watch::stop_watch_for_channel(&state, &body.guild_id, &body.channel_id)?;
    Ok(Json(serde_json::json!({ "ok": stopped })))
}

async fn internal_stream_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(ctx): Query<BridgeContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    let watch = state
        .discord
        .lock()
        .active_watch_for_channel(&ctx.guild_id, &ctx.channel_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(serde_json::json!({
        "active": watch.is_some(),
        "watch": watch,
    })))
}

#[derive(Deserialize)]
pub struct AgentRequest {
    #[serde(flatten)]
    pub ctx: BridgeContext,
    pub agent: String,
    pub prompt: String,
}

async fn internal_agent_ask(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<AgentRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    run_agent(&state, &headers, body, AgentTaskMode::Ask).await
}

async fn internal_agent_plan(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<AgentRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    run_agent(&state, &headers, body, AgentTaskMode::Plan).await
}

async fn internal_agent_do(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<AgentRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    run_agent(&state, &headers, body, AgentTaskMode::Do).await
}

async fn run_agent(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    body: AgentRequest,
    mode: AgentTaskMode,
) -> Result<Json<serde_json::Value>, ApiError> {
    let _ = headers;
    let link = resolve_link(state, &body.ctx)?;
    let bunny_user = resolve_bunny_user(state, &body.ctx)?;
    ensure_discord_agent(state, bunny_user, link.session_id)?;
    let task_id = task_runner::start_task(
        state.clone(),
        link.session_id,
        mode,
        &body.agent,
        &body.prompt,
        body.ctx.discord_user_id.clone(),
        body.ctx.thread_id.clone(),
        bunny_user,
    )
    .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(serde_json::json!({ "task_id": task_id.to_string() })))
}

#[derive(Deserialize)]
pub struct TaskStopRequest {
    #[serde(flatten)]
    pub ctx: BridgeContext,
    pub task_id: String,
}

async fn internal_task_stop(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<TaskStopRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    let id = Uuid::parse_str(&body.task_id).map_err(|_| ApiError::validation("task_id"))?;
    task_runner::cancel_task(&state, id).map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ApprovalResolveRequest {
    pub approval_id: String,
    pub approve: bool,
    #[serde(flatten)]
    pub ctx: BridgeContext,
}

async fn internal_approval_resolve(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<ApprovalResolveRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    let bunny_user = resolve_bunny_user(&state, &body.ctx)?;
    let approval_id =
        Uuid::parse_str(&body.approval_id).map_err(|_| ApiError::validation("approval_id"))?;
    task_runner::resolve_approval(&state, approval_id, body.approve, bunny_user)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct FollowStartRequest {
    #[serde(flatten)]
    pub ctx: BridgeContext,
    pub target: String,
    pub shell_name: Option<String>,
    pub interval_secs: u64,
}

async fn internal_follow_start(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<FollowStartRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    let link = resolve_link(&state, &body.ctx)?;
    let follow = bunny_discord::DiscordFollow {
        id: Uuid::new_v4(),
        guild_id: body.ctx.guild_id.clone(),
        channel_id: body.ctx.channel_id.clone(),
        session_id: link.session_id,
        target: body.target,
        shell_name: body.shell_name,
        interval_secs: body.interval_secs.max(10),
        active: true,
        created_at: Utc::now(),
    };
    state
        .discord
        .lock()
        .upsert_follow(&follow)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(serde_json::json!({ "follow_id": follow.id.to_string() })))
}

async fn internal_follow_stop(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<BridgeContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    state
        .discord
        .lock()
        .deactivate_follows(&body.guild_id, &body.channel_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct AuditRequest {
    #[serde(flatten)]
    pub ctx: BridgeContext,
    pub command: String,
    pub action_executed: String,
    pub result: String,
    pub session_id: Option<String>,
}

async fn internal_audit(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<AuditRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    let session_id = body
        .session_id
        .as_ref()
        .and_then(|s| Uuid::parse_str(s).ok());
    audit(
        &state,
        &body.ctx,
        session_id.unwrap_or(Uuid::nil()),
        &body.command,
        &body.action_executed,
        &body.result,
        None,
        None,
        None,
    );
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct CreateLinkCodeBody {
    pub password: Option<String>,
}

#[derive(Deserialize)]
pub struct LinkDiscordUserBody {
    pub discord_user_id: String,
}

async fn link_discord_user(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Json(body): Json<LinkDiscordUserBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state
        .discord
        .lock()
        .link_discord_user(&body.discord_user_id, user)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn create_discord_link_code(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Extension(session): Extension<AuthenticatedSession>,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateLinkCodeBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::api::ensure_session_access(&state, user, id, Action::DiscordLink)?;
    state
        .auth
        .assert_recent_auth(&session, body.password.as_deref())
        .map_err(|_| ApiError::forbidden("recent authentication required (password)"))?;
    let ttl = state.config.discord.link_code_ttl_minutes.max(1) as i64;
    let code = state
        .discord
        .lock()
        .generate_link_code(id, user, ttl)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(serde_json::json!({
        "code": code,
        "expires_in_minutes": ttl,
        "instructions": format!("In Discord, run: /bunny link {}", code),
    })))
}

async fn list_discord_links(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<DiscordSessionLink>>, ApiError> {
    crate::api::ensure_session_access(&state, user, id, Action::SessionRead)?;
    let links = state
        .discord
        .lock()
        .get_link_status_for_session(id)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(links))
}

async fn revoke_discord_links(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::api::ensure_session_access(&state, user, id, Action::DiscordLink)?;
    let links = state
        .discord
        .lock()
        .get_link_status_for_session(id)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    for link in links {
        state
            .discord
            .lock()
            .remove_session_link(&link.guild_id, &link.channel_id)
            .ok();
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn discord_oauth_start(
    State(state): State<Arc<AppState>>,
) -> Result<RedirectResponse, ApiError> {
    let client_id = state
        .config
        .discord
        .oauth_client_id
        .as_deref()
        .ok_or_else(|| ApiError::validation("discord oauth not configured"))?;
    let redirect = state
        .config
        .discord
        .oauth_redirect_uri
        .clone()
        .unwrap_or_else(|| "/api/v1/auth/discord/callback".into());
    let url = format!(
        "https://discord.com/api/oauth2/authorize?client_id={}&redirect_uri={}&response_type=code&scope=identify%20guilds.members.read",
        client_id,
        urlencoding::encode(&redirect)
    );
    Ok(RedirectResponse(url))
}

pub struct RedirectResponse(pub String);

impl IntoResponse for RedirectResponse {
    fn into_response(self) -> axum::response::Response {
        axum::response::Redirect::temporary(&self.0).into_response()
    }
}

#[derive(Deserialize)]
pub struct OAuthCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
}

pub async fn discord_oauth_callback(
    State(state): State<Arc<AppState>>,
    Query(q): Query<OAuthCallbackQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let code = q.code.ok_or_else(|| ApiError::validation("missing code"))?;
    let token = exchange_discord_code(&state, &code).await?;
    let discord_user_id = fetch_discord_user_id(&token).await?;
    Ok(Json(serde_json::json!({
        "discord_user_id": discord_user_id,
        "message": "Link your Bunny account from profile settings with this Discord id",
    })))
}

async fn exchange_discord_code(state: &AppState, code: &str) -> Result<String, ApiError> {
    let client_id = state
        .config
        .discord
        .oauth_client_id
        .as_deref()
        .ok_or_else(|| ApiError::validation("oauth not configured"))?;
    let client_secret = state
        .config
        .discord
        .oauth_client_secret
        .as_deref()
        .ok_or_else(|| ApiError::validation("oauth not configured"))?;
    let redirect = state
        .config
        .discord
        .oauth_redirect_uri
        .clone()
        .unwrap_or_else(|| "/api/v1/auth/discord/callback".into());
    let client = reqwest::Client::new();
    let res = client
        .post("https://discord.com/api/oauth2/token")
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect.as_str()),
        ])
        .send()
        .await
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    let json: serde_json::Value = res
        .json()
        .await
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    json.get("access_token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| ApiError::validation("no access token"))
}

async fn fetch_discord_user_id(token: &str) -> Result<String, ApiError> {
    let client = reqwest::Client::new();
    let res = client
        .get("https://discord.com/api/users/@me")
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    let json: serde_json::Value = res
        .json()
        .await
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    json.get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| ApiError::validation("no user id"))
}

fn resolve_link(state: &AppState, ctx: &BridgeContext) -> Result<DiscordSessionLink, ApiError> {
    state
        .discord
        .lock()
        .get_session_link(&ctx.guild_id, &ctx.channel_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?
        .ok_or_else(|| ApiError::not_found("channel not linked to a bunny session"))
}

fn resolve_bunny_user(state: &AppState, ctx: &BridgeContext) -> Result<Uuid, ApiError> {
    let mut discord = state.discord.lock();
    if let Some(user_id) = discord
        .get_bunny_user_for_discord(&ctx.discord_user_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?
    {
        return Ok(user_id);
    }
    if let Some(user_id) = discord
        .backfill_discord_user_link(&ctx.guild_id, &ctx.channel_id, &ctx.discord_user_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?
    {
        return Ok(user_id);
    }
    Err(ApiError::forbidden(
        "discord account not linked to bunny user — run /bunny link again with a fresh code from the Web UI",
    ))
}

fn ensure_discord_control(state: &AppState, user_id: Uuid, session_id: Uuid) -> Result<(), ApiError> {
    let role = crate::api::get_role(state, user_id, session_id)?;
    if role_can(role, Action::DiscordControl) {
        Ok(())
    } else {
        Err(ApiError::forbidden("permission denied"))
    }
}

fn ensure_discord_agent(state: &AppState, user_id: Uuid, session_id: Uuid) -> Result<(), ApiError> {
    let role = crate::api::get_role(state, user_id, session_id)?;
    if role_can(role, Action::DiscordAgentRun) {
        Ok(())
    } else {
        Err(ApiError::forbidden("permission denied"))
    }
}

async fn capture_shell_run_output(
    state: Arc<AppState>,
    session_id: Uuid,
    term_id: Uuid,
    command: &str,
) -> Result<(String, i32), ApiError> {
    let command = command.to_string();
    tokio::task::spawn_blocking(move || {
        crate::terminals::exec_discord_shell_command(&state, term_id, session_id, &command)
            .map_err(|e| ApiError::validation(&e.to_string()))
    })
    .await
    .map_err(|e| ApiError::validation(&e.to_string()))?
}

fn resolve_shell_terminal(
    state: &AppState,
    session_id: Uuid,
    shell_name: Option<&str>,
) -> Result<Uuid, ApiError> {
    if let Some(name) = shell_name {
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
        let rows = db
            .list_terminals_for_session(session_id)
            .unwrap_or_default();
        for (tid, _, term_name, ..) in rows {
            if term_name == name {
                return Ok(tid);
            }
        }
        return Err(ApiError::not_found("shell"));
    }
    state
        .auth
        .db()
        .lock()
        .list_terminals_for_session(session_id)
        .unwrap_or_default()
        .into_iter()
        .next()
        .map(|(id, ..)| id)
        .ok_or_else(|| ApiError::not_found("no shell — open a shell in the Web UI first"))
}

pub fn audit(
    state: &AppState,
    ctx: &BridgeContext,
    session_id: Uuid,
    command: &str,
    action_executed: &str,
    result: &str,
    bunny_user: Option<Uuid>,
    shell_id: Option<Uuid>,
    browser_id: Option<Uuid>,
) {
    let entry = DiscordAuditEntry {
        id: Uuid::new_v4(),
        discord_user_id: Some(ctx.discord_user_id.clone()),
        bunny_user_id: bunny_user,
        guild_id: Some(ctx.guild_id.clone()),
        channel_id: Some(ctx.channel_id.clone()),
        thread_id: ctx.thread_id.clone(),
        session_id: if session_id.is_nil() {
            None
        } else {
            Some(session_id)
        },
        command: command.to_string(),
        action_executed: action_executed.to_string(),
        agent: None,
        shell_id,
        browser_id,
        approval_id: None,
        result: result.to_string(),
        created_at: Utc::now(),
    };
    state.discord.lock().insert_audit(&entry).ok();
}

pub fn generate_bridge_token() -> (String, String) {
    let token: String = (0..32)
        .map(|_| {
            let v = rand::random::<u8>() % 36;
            if v < 10 {
                (b'0' + v) as char
            } else {
                (b'a' + v - 10) as char
            }
        })
        .collect();
    (token.clone(), hash_token(&token))
}
