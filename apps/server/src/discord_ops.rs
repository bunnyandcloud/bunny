use crate::api::ApiError;
use crate::compositor::{capture_browser_snapshot, capture_snapshot, SnapshotTarget};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use crate::state::AppState;
use crate::task_runner;
use crate::watch;
use axum::{
    extract::{Extension, Path, Query, State},
    http::{header, HeaderMap},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use bunny_auth::AuthenticatedSession;
use bunny_core::permissions::{role_can, Action};
use bunny_i18n::Locale;
use bunny_discord::{AgentTaskMode, DiscordAuditEntry, DiscordSessionLink};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
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
        .route("/auth/discord/start", get(discord_oauth_start))
        .route(
            "/auth/discord/link",
            post(link_discord_user).delete(unlink_discord_user),
        )
        .route("/discord/setup", get(discord_setup_status))
        .route("/discord/setup/bot", post(discord_setup_bot))
        .route("/discord/setup/oauth", post(discord_setup_oauth))
        .route("/discord/setup/reload", post(discord_setup_reload))
        .with_state(state)
}

pub fn internal_router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/link", post(internal_link))
        .route("/unlink", post(internal_unlink))
        .route("/status", get(internal_status))
        .route("/locale", post(internal_set_locale))
        .route("/user-locale", get(internal_user_locale))
        .route("/shell/list", get(internal_shell_list))
        .route("/shell/run", post(internal_shell_run))
        .route("/shell/run/stop", post(internal_shell_run_stop))
        .route("/shell/file", post(internal_shell_file))
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
        .route("/claude/reset", post(internal_claude_reset))
        .route("/thread/bind", post(internal_thread_bind_route))
        .route("/thread/input", post(internal_thread_input_route))
        .route("/thread/answer", post(internal_thread_answer_route))
        .route("/thread/permission", post(internal_thread_permission_route))
        .route("/thread/discussion", post(internal_thread_discussion_route))
        .route("/thread/stop", post(internal_thread_stop_route))
        .route("/thread/finalize", post(internal_thread_finalize_route))
        .route("/thread/merge", post(internal_thread_merge_route))
        .route("/thread/status", post(internal_thread_status_route))
        .route("/thread/attachment", post(internal_thread_attachment_route))
        .route("/project/set", post(internal_project_set_route))
        .route("/project", get(internal_project_get_route))
        .route("/git", post(internal_git_route))
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
    let cfg = effective_discord_config(state);
    if !cfg.enabled {
        return Err(ApiError::forbidden("discord integration disabled"));
    }
    let hash = cfg
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
    let locale = discord_user_locale(&state, &ctx);
    Ok(Json(serde_json::json!({
        "linked": true,
        "session_id": link.session_id.to_string(),
        "guild_id": link.guild_id,
        "channel_id": link.channel_id,
        "locale": locale.as_str(),
        "locale_source": "user",
    })))
}

#[derive(Deserialize)]
pub struct InternalSetLocaleRequest {
    #[serde(flatten)]
    pub ctx: BridgeContext,
    pub locale: String,
}

async fn internal_set_locale(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<InternalSetLocaleRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    if !bunny_i18n::is_valid_locale_code(&body.locale) {
        let loc = discord_user_locale(&state, &body.ctx);
        return Err(ApiError::validation(
            &bunny_i18n::t(loc, "api.error.invalid_locale", &[]),
        ));
    }
    let user_id = resolve_bunny_user(&state, &body.ctx)?;
    state
        .auth
        .set_user_locale(user_id, &body.locale)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    let loc = Locale::from_db(&body.locale);
    Ok(Json(serde_json::json!({
        "ok": true,
        "locale": loc.as_str(),
        "message": bunny_i18n::t(loc, "discord.language.updated", &[("locale", loc.as_str())]),
    })))
}

async fn internal_user_locale(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(ctx): Query<BridgeContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    let locale = discord_user_locale(&state, &ctx);
    Ok(Json(serde_json::json!({
        "locale": locale.as_str(),
        "locale_source": if resolve_bunny_user(&state, &ctx).is_ok() { "user" } else { "default" },
    })))
}

/// Preferred UI locale for the Discord user behind `ctx`, defaulting to English.
pub(crate) fn discord_user_locale(state: &AppState, ctx: &BridgeContext) -> Locale {
    if let Ok(user_id) = resolve_bunny_user(state, ctx) {
        if let Ok(loc) = state.auth.get_user_locale(user_id) {
            return Locale::from_db(&loc);
        }
    }
    Locale::En
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
    let resolved_term_id = resolve_discord_shell(
        &state,
        link.session_id,
        &body.ctx.guild_id,
        &body.ctx.channel_id,
        body.ctx.thread_id.as_deref(),
        body.shell_name.as_deref(),
    )?;
    let previous_shell_name = terminal_name(&state, resolved_term_id);
    let mut term_id = resolved_term_id;
    let mut shell_auto_created = false;
    crate::terminals::ensure_session_terminals_live(&state, link.session_id);
    if crate::terminals::discord_shell_pane_busy(&state, term_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?
    {
        let cwd = crate::terminals::discord_shell_working_directory(&state, term_id)
            .map_err(|e| ApiError::validation(&e.to_string()))?;
        term_id = create_discord_shell_at_cwd(&state, link.session_id, &cwd)?;
        shell_auto_created = true;
    }
    if bunny_discord::risk::is_interactive_discord_command(&body.command) {
        return Err(ApiError::validation(
            "commande interactive non supportée depuis Discord — utilisez le terminal Web UI, ou par ex. `head -n 80 landing-page.html`",
        ));
    }
    let run = capture_shell_run_output(
        Arc::clone(&state),
        link.session_id,
        term_id,
        &body.command,
        Some(bunny_user),
    )
    .await?;
    let output = truncate_discord_shell_output(&run.output);
    let exit_code = run.exit_code;
    remember_discord_shell(&state, &body.ctx.guild_id, &body.ctx.channel_id, term_id);
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
        "persistent": run.persistent,
        "shell": shell_name,
        "shell_auto_created": shell_auto_created,
        "previous_shell": if shell_auto_created {
            previous_shell_name
        } else {
            None::<String>
        },
    })))
}

#[derive(Deserialize)]
pub struct ShellRunStopRequest {
    #[serde(flatten)]
    pub ctx: BridgeContext,
    pub shell_name: Option<String>,
}

async fn internal_shell_run_stop(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<ShellRunStopRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    let link = resolve_link(&state, &body.ctx)?;
    let bunny_user = resolve_bunny_user(&state, &body.ctx)?;
    ensure_discord_control(&state, bunny_user, link.session_id)?;
    let term_id = resolve_discord_shell(
        &state,
        link.session_id,
        &body.ctx.guild_id,
        &body.ctx.channel_id,
        body.ctx.thread_id.as_deref(),
        body.shell_name.as_deref(),
    )?;
    crate::terminals::ensure_session_terminals_live(&state, link.session_id);
    let state_bg = Arc::clone(&state);
    let message = tokio::task::spawn_blocking(move || {
        crate::terminals::exec_discord_shell_interrupt(&state_bg, term_id)
            .map_err(|e| ApiError::validation(&e.to_string()))
    })
    .await
    .map_err(|e| ApiError::validation(&e.to_string()))??;
    remember_discord_shell(&state, &body.ctx.guild_id, &body.ctx.channel_id, term_id);
    let shell_name = state
        .auth
        .db()
        .lock()
        .get_terminal(term_id)
        .ok()
        .flatten()
        .map(|row| row.2);
    audit(
        &state,
        &body.ctx,
        link.session_id,
        "/bunny run_stop",
        "Ctrl+C",
        "ok",
        Some(bunny_user),
        Some(term_id),
        None,
    );
    Ok(Json(serde_json::json!({
        "ok": true,
        "message": message,
        "shell": shell_name,
    })))
}

#[derive(Deserialize)]
pub struct ShellFileRequest {
    #[serde(flatten)]
    pub ctx: BridgeContext,
    pub path: String,
    pub shell_name: Option<String>,
}

async fn internal_shell_file(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<ShellFileRequest>,
) -> Result<impl IntoResponse, ApiError> {
    verify_bridge_token(&state, &headers)?;
    let link = resolve_link(&state, &body.ctx)?;
    let bunny_user = resolve_bunny_user(&state, &body.ctx)?;
    ensure_discord_control(&state, bunny_user, link.session_id)?;
    let term_id = resolve_discord_shell(
        &state,
        link.session_id,
        &body.ctx.guild_id,
        &body.ctx.channel_id,
        body.ctx.thread_id.as_deref(),
        body.shell_name.as_deref(),
    )?;
    crate::terminals::ensure_session_terminals_live(&state, link.session_id);
    let (filename, bytes) = crate::terminals::read_discord_shell_file(
        &state,
        term_id,
        &body.path,
        crate::terminals::DISCORD_FILE_ATTACHMENT_MAX,
    )
    .map_err(|e| ApiError::validation(&e.to_string()))?;
    remember_discord_shell(&state, &body.ctx.guild_id, &body.ctx.channel_id, term_id);
    let shell_name = state
        .auth
        .db()
        .lock()
        .get_terminal(term_id)
        .ok()
        .flatten()
        .map(|row| row.2)
        .unwrap_or_else(|| "shell".into());
    let size = bytes.len();
    let caption = format!(
        "File `{filename}` from shell `{shell_name}` ({size} bytes) — open the attachment to view the full content."
    );
    audit(
        &state,
        &body.ctx,
        link.session_id,
        "/bunny file",
        &body.path,
        "ok",
        Some(bunny_user),
        Some(term_id),
        None,
    );
    let caption_header = axum::http::HeaderValue::from_str(&caption).unwrap_or_else(|_| {
        axum::http::HeaderValue::from_static("File attachment")
    });
    let name_header = axum::http::HeaderValue::from_str(&filename).unwrap_or_else(|_| {
        axum::http::HeaderValue::from_static("file.txt")
    });
    let mut response = axum::response::Response::new(axum::body::Body::from(bytes));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/octet-stream"),
    );
    response.headers_mut().insert(
        axum::http::HeaderName::from_static("x-bunny-file-caption"),
        caption_header,
    );
    response.headers_mut().insert(
        axum::http::HeaderName::from_static("x-bunny-file-name"),
        name_header,
    );
    Ok(response)
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
    let locale = discord_user_locale(&state, &body.ctx);
    ensure_shell_may_close(&state, session_id, term_id, locale)?;
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
    let default_name = default_discord_shell_name(
        &state,
        link.session_id,
        &ctx.guild_id,
        &ctx.channel_id,
        ctx.thread_id.as_deref(),
    );
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

fn resolve_browser_url(
    state: &AppState,
    session_id: Uuid,
    browser_url: Option<String>,
    browser_port: Option<u16>,
) -> String {
    browser_url.unwrap_or_else(|| {
        browser_port
            .map(|p| format!("http://127.0.0.1:{p}"))
            .unwrap_or_else(|| default_browser_url(state, session_id))
    })
}

fn resolve_shell_label(
    state: &AppState,
    session_id: Uuid,
    channel_id: &str,
    thread_id: Option<&str>,
    shell_name: Option<&str>,
) -> Result<String, ApiError> {
    let term_id = if shell_name.is_some() {
        resolve_shell_terminal(state, session_id, shell_name)?
    } else {
        resolve_discord_shell(state, session_id, "", channel_id, thread_id, None)?
    };
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
) -> Result<axum::response::Response, ApiError> {
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

    let shell_label = resolve_shell_label(
        &state,
        link.session_id,
        &body.ctx.channel_id,
        body.ctx.thread_id.as_deref(),
        body.shell_name.as_deref(),
    )?;

    if matches!(target, SnapshotTarget::Shell) {
        let term_id = resolve_discord_shell(
            &state,
            link.session_id,
            &body.ctx.guild_id,
            &body.ctx.channel_id,
            body.ctx.thread_id.as_deref(),
            body.shell_name.as_deref(),
        )?;
        let lines = crate::terminals::DISCORD_SNAPSHOT_MAX_LINES;
        let state_bg = Arc::clone(&state);
        let text = tokio::task::spawn_blocking(move || {
            crate::terminals::discord_shell_snapshot_text(&state_bg, term_id, lines)
        })
        .await
        .map_err(|e| ApiError::validation(&e.to_string()))?;
        let discord_caption = format!("Shell — {shell_label} (last {lines} lines)");
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
        return Ok(Json(serde_json::json!({
            "format": "text",
            "text": text,
            "caption": discord_caption,
            "lines": lines,
        }))
        .into_response());
    }

    if matches!(target, SnapshotTarget::All) {
        let term_id = resolve_discord_shell(
            &state,
            link.session_id,
            &body.ctx.guild_id,
            &body.ctx.channel_id,
            body.ctx.thread_id.as_deref(),
            body.shell_name.as_deref(),
        )?;
        let lines = crate::terminals::DISCORD_SNAPSHOT_MAX_LINES;
        let state_shell = Arc::clone(&state);
        let shell_text = tokio::task::spawn_blocking(move || {
            crate::terminals::discord_shell_snapshot_text(&state_shell, term_id, lines)
        })
        .await
        .map_err(|e| ApiError::validation(&e.to_string()))?;
        let shell_caption = format!("Shell — {shell_label} (last {lines} lines)");

        let browser = capture_browser_snapshot(&state, link.session_id).await;
        let (browser_png_base64, browser_unavailable) = match browser {
            Ok(snap) => (BASE64.encode(&snap.png), None),
            Err(e) => {
                tracing::warn!("full_snapshot browser capture failed: {e}");
                (String::new(), Some(e.to_string()))
            }
        };

        let discord_caption = if browser_unavailable.is_some() {
            format!("Full snapshot — {shell_label} (last {lines} lines, browser unavailable)")
        } else {
            format!("Full snapshot — {shell_label} (last {lines} lines) + browser")
        };
        audit(
            &state,
            &body.ctx,
            link.session_id,
            "/bunny full_snapshot",
            &discord_caption,
            "ok",
            None,
            None,
            None,
        );
        return Ok(Json(serde_json::json!({
            "format": "shell_text_and_browser",
            "text": shell_text,
            "caption": discord_caption,
            "shell_caption": shell_caption,
            "lines": lines,
            "browser_png_base64": browser_png_base64,
            "browser_unavailable": browser_unavailable,
        }))
        .into_response());
    }

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
    /// Local dev server port when `browser_url` is omitted (e.g. 5173).
    pub browser_port: Option<u16>,
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
    let url = resolve_browser_url(
        &state,
        link.session_id,
        body.browser_url.clone(),
        body.browser_port,
    );
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

#[derive(Deserialize)]
pub struct StreamStopRequest {
    #[serde(flatten)]
    pub ctx: BridgeContext,
    /// Full watch URL (`.../watch/<token>`) or path; stops only that link when set.
    pub url: Option<String>,
}

async fn internal_stream_stop(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<StreamStopRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    let stopped = watch::stop_browser_streams(
        &state,
        &body.ctx.guild_id,
        &body.ctx.channel_id,
        body.url.as_deref(),
    )?;
    Ok(Json(serde_json::json!({ "stopped": stopped })))
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
    pub shell_name: Option<String>,
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
    let task_id = task_runner::create_task_record(
        state,
        link.session_id,
        mode,
        &body.agent,
        &body.prompt,
        &body.ctx.discord_user_id,
        bunny_user,
    )
    .map_err(|e| ApiError::validation(&e.to_string()))?;

    let shell_name = body.shell_name.as_deref();
    let mode_label = match mode {
        AgentTaskMode::Ask => "ask",
        AgentTaskMode::Plan => "plan",
        AgentTaskMode::Do => "do",
        _ => "agent",
    };

    let result = task_runner::run_discord_agent(
        Arc::clone(state),
        link.session_id,
        &body.ctx.guild_id,
        &body.ctx.channel_id,
        mode,
        &body.prompt,
        shell_name,
    )
    .await;

    match result {
        Ok(r) if r.needs_approval => {
            let approval_id = Uuid::new_v4();
            let summary = r.approval_summary.clone().unwrap_or_default();
            let reason = if let Some(ref ctx) = r.claude_pane_ctx {
                crate::discord_claude::encode_claude_pane_reason(ctx)
            } else {
                "shell_risk".into()
            };
            let req = bunny_discord::ApprovalRequest {
                id: approval_id,
                task_id,
                session_id: link.session_id,
                action_summary: summary.clone(),
                reason,
                status: "pending".into(),
                discord_message_id: None,
                created_at: chrono::Utc::now(),
                resolved_at: None,
            };
            state
                .discord
                .lock()
                .create_approval(&req)
                .map_err(|e| ApiError::validation(&e.to_string()))?;
            let policy = bunny_policy::ApproverPolicy::default_for_risk(
                bunny_policy::classify_shell_risk(&summary),
            );
            crate::integrations_ops::store_approval_policy(state, approval_id, &policy);
            crate::approval_service::ApprovalService::notify_session_channels(
                state,
                link.session_id,
                approval_id,
            );
            state
                .discord
                .lock()
                .update_task_status(task_id, bunny_discord::AgentTaskStatus::WaitingApproval)
                .map_err(|e| ApiError::validation(&e.to_string()))?;
            audit(
                state,
                &body.ctx,
                link.session_id,
                &format!("/bunny {mode_label}"),
                &body.prompt,
                "needs_approval",
                Some(bunny_user),
                None,
                None,
            );
            Ok(Json(serde_json::json!({
                "needs_approval": true,
                "approval_id": approval_id.to_string(),
                "task_id": task_id.to_string(),
                "summary": summary,
                "shell": r.shell,
                "mode": mode_label,
                "claude_pane": r.claude_pane_ctx.is_some(),
            })))
        }
        Ok(r) => {
            let status = if r.exit_code == 0 {
                bunny_discord::AgentTaskStatus::Done
            } else {
                bunny_discord::AgentTaskStatus::Failed
            };
            state
                .discord
                .lock()
                .update_task_status(task_id, status)
                .map_err(|e| ApiError::validation(&e.to_string()))?;
            audit(
                state,
                &body.ctx,
                link.session_id,
                &format!("/bunny {mode_label}"),
                &body.prompt,
                if r.exit_code == 0 { "ok" } else { "error" },
                Some(bunny_user),
                None,
                None,
            );
            Ok(Json(serde_json::json!({
                "ok": r.exit_code == 0,
                "task_id": task_id.to_string(),
                "output": r.output,
                "exit_code": r.exit_code,
                "shell": r.shell,
                "mode": mode_label,
            })))
        }
        Err(e) => {
            let _ = state
                .discord
                .lock()
                .update_task_status(task_id, bunny_discord::AgentTaskStatus::Failed);
            Err(e)
        }
    }
}

#[derive(Deserialize)]
pub struct TaskStopRequest {
    #[serde(flatten)]
    #[allow(dead_code)]
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
    let outcome = crate::approval_service::ApprovalService::resolve(&state, approval_id, body.approve, bunny_user)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "output": outcome.output,
        "exit_code": outcome.exit_code,
        "mode": outcome.mode,
        "shell": outcome.shell,
    })))
}

#[derive(Deserialize)]
pub struct ClaudeResetRequest {
    #[serde(flatten)]
    pub ctx: BridgeContext,
}

async fn internal_claude_reset(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<ClaudeResetRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    let link = resolve_link(&state, &body.ctx)?;
    crate::discord_claude::clear_claude_session(&state, &body.ctx.guild_id, &body.ctx.channel_id)?;
    audit(
        &state,
        &body.ctx,
        link.session_id,
        "/bunny claude_reset",
        "",
        "ok",
        None,
        None,
        None,
    );
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

pub fn discord_bridge_configured(state: &AppState) -> bool {
    let cfg = effective_discord_config(state);
    (cfg.enabled && cfg.bridge_token_hash.is_some()) || bridge_configured_on_disk()
}

pub fn discord_oauth_configured(state: &AppState) -> bool {
    let cfg = effective_discord_config(state);
    cfg.oauth_client_id.is_some() && cfg.oauth_client_secret.is_some()
}

fn setup_public_url(state: &AppState) -> String {
    effective_discord_config(state)
        .public_url
        .clone()
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| format!("http://127.0.0.1:{}", state.config.server.port))
}

fn setup_oauth_redirect_uri(state: &AppState) -> String {
    let cfg = effective_discord_config(state);
    cfg.oauth_redirect_uri
        .clone()
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| {
            format!(
                "{}/api/v1/auth/discord/callback",
                setup_public_url(state).trim_end_matches('/')
            )
        })
}

fn effective_discord_config(state: &AppState) -> bunny_core::config::DiscordConfig {
    let path = crate::config_init::config_path();
    if path.is_file() {
        let paths = vec![path.to_str().unwrap(), ".bunny.yaml"];
        if let Ok(cfg) = bunny_core::config::BunnyConfig::load(&paths) {
            return cfg.discord;
        }
    }
    state.config.discord.clone()
}

fn bridge_configured_on_disk() -> bool {
    default_bridge_path().is_file()
}

fn looks_like_discord_application_id(token: &str) -> bool {
    token.chars().all(|c| c.is_ascii_digit()) && token.len() >= 15
}

pub fn default_bridge_path() -> std::path::PathBuf {
    if let Ok(env) = std::env::var("BUNNY_DISCORD_BRIDGE_CONFIG") {
        if !env.trim().is_empty() {
            return std::path::PathBuf::from(env);
        }
    }
    if let Some(root) = crate::web_ui::find_repo_root() {
        return root.join(".discord/bridge.yaml");
    }
    std::env::current_dir()
        .unwrap_or_default()
        .join(".discord/bridge.yaml")
}

fn ensure_discord_setup_owner(state: &AppState, user_id: Uuid) -> Result<(), ApiError> {
    let owner = state
        .auth
        .owner_id()
        .map_err(|_| ApiError::forbidden("permission denied"))?;
    if user_id == owner {
        Ok(())
    } else {
        Err(ApiError::forbidden("permission denied"))
    }
}

#[derive(Serialize)]
pub struct DiscordSetupStatus {
    pub bridge_configured: bool,
    pub oauth_configured: bool,
    pub public_url: String,
    pub oauth_redirect_uri: String,
    pub application_id: Option<String>,
    pub guild_id: Option<String>,
    pub bridge_path: String,
}

async fn discord_setup_status(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
) -> Result<Json<DiscordSetupStatus>, ApiError> {
    ensure_discord_setup_owner(&state, user)?;
    let bridge_path = default_bridge_path();
    let application_id = crate::config_init::read_bridge_application_id(&bridge_path)
        .map(|id| id.to_string());
    let guild_id = crate::config_init::read_bridge_guild_id(&bridge_path).map(|id| id.to_string());
    Ok(Json(DiscordSetupStatus {
        bridge_configured: {
            let cfg = effective_discord_config(&state);
            (cfg.enabled && cfg.bridge_token_hash.is_some()) || bridge_configured_on_disk()
        },
        oauth_configured: {
            let cfg = effective_discord_config(&state);
            cfg.oauth_client_id.is_some() && cfg.oauth_client_secret.is_some()
        },
        public_url: setup_public_url(&state),
        oauth_redirect_uri: setup_oauth_redirect_uri(&state),
        application_id,
        guild_id,
        bridge_path: bridge_path.display().to_string(),
    }))
}

#[derive(Deserialize)]
struct DiscordSetupBotRequest {
    application_id: u64,
    bot_token: String,
    guild_id: Option<u64>,
    public_url: Option<String>,
}

#[derive(Serialize)]
struct DiscordSetupBotResponse {
    ok: bool,
    bridge_path: String,
    public_url: String,
    oauth_redirect_uri: String,
    bridge_running: bool,
    bridge_starting: bool,
}

async fn discord_setup_bot(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Json(body): Json<DiscordSetupBotRequest>,
) -> Result<Json<DiscordSetupBotResponse>, ApiError> {
    ensure_discord_setup_owner(&state, user)?;
    let token = body.bot_token.trim();
    if token.is_empty() {
        return Err(ApiError::validation("bot token is required"));
    }
    if body.application_id == 0 {
        return Err(ApiError::validation("application id is required"));
    }
    if looks_like_discord_application_id(token) {
        return Err(ApiError::validation(
            "this looks like an Application ID — use Bot → Token in the Discord Developer Portal (Reset Token → Copy)",
        ));
    }
    let public_url = body
        .public_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.trim_end_matches('/').to_string())
        .unwrap_or_else(|| setup_public_url(&state));
    if !public_url.starts_with("http://") && !public_url.starts_with("https://") {
        return Err(ApiError::validation("public URL must start with http:// or https://"));
    }
    let (plain, hash) = crate::config_init::generate_bridge_credentials();
    crate::config_init::apply_discord_to_config(&hash, &public_url)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    let bridge_path = default_bridge_path();
    let internal_url = format!("http://127.0.0.1:{}", state.config.server.port);
    crate::config_init::write_bridge_dev_file(
        &bridge_path,
        body.application_id,
        token,
        &plain,
        &internal_url,
        &public_url,
        body.guild_id,
    )
    .map_err(|e| ApiError::validation(&e.to_string()))?;
    let oauth_redirect_uri = format!("{public_url}/api/v1/auth/discord/callback");
    crate::discord_bridge::spawn_restart_managed(state.clone());
    Ok(Json(DiscordSetupBotResponse {
        ok: true,
        bridge_path: bridge_path.display().to_string(),
        public_url,
        oauth_redirect_uri,
        bridge_running: false,
        bridge_starting: true,
    }))
}

#[derive(Deserialize)]
struct DiscordSetupOAuthRequest {
    oauth_client_secret: String,
    oauth_client_id: Option<String>,
    oauth_redirect_uri: Option<String>,
}

#[derive(Serialize)]
struct DiscordSetupOAuthResponse {
    ok: bool,
    oauth_redirect_uri: String,
}

async fn discord_setup_oauth(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Json(body): Json<DiscordSetupOAuthRequest>,
) -> Result<Json<DiscordSetupOAuthResponse>, ApiError> {
    ensure_discord_setup_owner(&state, user)?;
    let secret = body.oauth_client_secret.trim();
    if secret.is_empty() {
        return Err(ApiError::validation("oauth client secret is required"));
    }
    let bridge_path = default_bridge_path();
    let app_id_hint = crate::config_init::read_bridge_application_id(&bridge_path)
        .map(|id| id.to_string());
    let client_id = body
        .oauth_client_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or(app_id_hint)
        .ok_or_else(|| ApiError::validation("oauth client id is required (configure bot first)"))?;
    let redirect = body
        .oauth_redirect_uri
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| setup_oauth_redirect_uri(&state));
    crate::config_init::apply_oauth_to_config(&client_id, secret, &redirect)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(DiscordSetupOAuthResponse {
        ok: true,
        oauth_redirect_uri: redirect,
    }))
}

async fn discord_setup_reload(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
) -> Result<Json<crate::discord_bridge::DiscordBridgeReloadResponse>, ApiError> {
    ensure_discord_setup_owner(&state, user)?;
    if !discord_bridge_configured(&state) {
        return Err(ApiError::validation("discord bridge is not configured"));
    }
    let bridge_path = default_bridge_path();
    crate::discord_bridge::spawn_restart_managed(state);
    Ok(Json(crate::discord_bridge::DiscordBridgeReloadResponse {
        ok: true,
        bridge_running: false,
        bridge_starting: true,
        bridge_path: bridge_path.display().to_string(),
    }))
}

#[derive(Serialize)]
pub struct DiscordAccountStatus {
    pub bridge_configured: bool,
    pub oauth_configured: bool,
    pub linked: bool,
    pub discord_user_id: Option<String>,
    pub username: Option<String>,
}

pub fn discord_account_status(state: &AppState, user_id: Uuid) -> DiscordAccountStatus {
    let bridge_configured = discord_bridge_configured(state);
    let oauth_configured = discord_oauth_configured(state);
    let link = state
        .discord
        .lock()
        .get_discord_link_for_user(user_id)
        .ok()
        .flatten();
    let (linked, discord_user_id, username) = match link {
        Some(l) => {
            let display = l
                .discord_global_name
                .or(l.discord_username)
                .or_else(|| Some(l.discord_user_id.chars().take(8).collect()));
            (true, Some(l.discord_user_id), display)
        }
        None => (false, None, None),
    };
    DiscordAccountStatus {
        bridge_configured,
        oauth_configured,
        linked,
        discord_user_id,
        username,
    }
}

pub async fn discord_oauth_start(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    headers: HeaderMap,
) -> Result<RedirectResponse, ApiError> {
    if !discord_oauth_configured(&state) {
        return Err(ApiError::validation("discord oauth not configured"));
    }
    let cfg = effective_discord_config(&state);
    let client_id = cfg
        .oauth_client_id
        .as_deref()
        .ok_or_else(|| ApiError::validation("discord oauth not configured"))?;
    let redirect = oauth_redirect_uri(&state);
    let return_origin = oauth_return_origin(&headers, &state);
    let exp = Utc::now().timestamp() + 600;
    let state_param = sign_oauth_state(&state, user, exp, &return_origin)?;
    let url = format!(
        "https://discord.com/api/oauth2/authorize?client_id={}&redirect_uri={}&response_type=code&scope=identify%20guilds.members.read&state={}",
        client_id,
        urlencoding::encode(&redirect),
        urlencoding::encode(&state_param),
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
    pub error: Option<String>,
}

pub async fn discord_oauth_callback(
    State(state): State<Arc<AppState>>,
    Query(q): Query<OAuthCallbackQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if q.error.is_some() {
        return Ok(oauth_ui_redirect("error", &setup_public_url(&state)));
    }
    let code = q.code.ok_or_else(|| ApiError::validation("missing code"))?;
    let state_param = q
        .state
        .ok_or_else(|| ApiError::validation("missing state"))?;
    let (bunny_user_id, return_origin) = verify_oauth_state(&state, &state_param)
        .map_err(|_| ApiError::validation("invalid oauth state"))?;
    let token = exchange_discord_code(&state, &code).await?;
    let profile = fetch_discord_profile(&token).await?;
    if let Some(existing) = state
        .discord
        .lock()
        .get_bunny_user_for_discord(&profile.id)
        .map_err(|e| ApiError::validation(&e.to_string()))?
    {
        if existing != bunny_user_id {
            return Ok(oauth_ui_redirect("conflict", &return_origin));
        }
    }
    state
        .discord
        .lock()
        .link_discord_user_profile(
            &profile.id,
            bunny_user_id,
            profile.username.as_deref(),
            profile.global_name.as_deref(),
        )
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(oauth_ui_redirect("success", &return_origin))
}

fn oauth_ui_redirect(outcome: &str, return_origin: &str) -> axum::response::Response {
    let base = return_origin.trim_end_matches('/');
    axum::response::Redirect::temporary(&format!("{base}/?discord_link={outcome}")).into_response()
}

fn oauth_return_origin(headers: &HeaderMap, state: &AppState) -> String {
    if let Some(host) = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(str::trim)
        .filter(|h| !h.is_empty())
    {
        let scheme = headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|s| *s == "http" || *s == "https")
            .unwrap_or("http");
        return format!("{scheme}://{host}");
    }
    setup_public_url(state)
}

fn validate_return_origin(origin: &str) -> Result<(), ApiError> {
    let origin = origin.trim();
    if origin.is_empty() || origin.len() > 256 {
        return Err(ApiError::validation("invalid return origin"));
    }
    if !origin.starts_with("http://") && !origin.starts_with("https://") {
        return Err(ApiError::validation("invalid return origin"));
    }
    if origin.contains(['\n', '\r', '@']) {
        return Err(ApiError::validation("invalid return origin"));
    }
    Ok(())
}

fn oauth_redirect_uri(state: &AppState) -> String {
    setup_oauth_redirect_uri(state)
}

fn oauth_signing_key(state: &AppState) -> String {
    effective_discord_config(state)
        .bridge_token_hash
        .unwrap_or_else(|| format!("bunny-oauth:{}", state.data_dir))
}

fn sign_oauth_state(
    state: &AppState,
    user_id: Uuid,
    exp: i64,
    return_origin: &str,
) -> Result<String, ApiError> {
    validate_return_origin(return_origin)?;
    let return_origin = return_origin.trim_end_matches('/');
    let payload = format!("{user_id}|{exp}|{return_origin}");
    let sig = oauth_signature(&oauth_signing_key(state), &payload);
    Ok(format!("{payload}.{sig}"))
}

fn verify_oauth_state(state: &AppState, token: &str) -> Result<(Uuid, String), ApiError> {
    let (payload, sig) = token
        .rsplit_once('.')
        .ok_or_else(|| ApiError::validation("malformed state"))?;
    let mut parts = payload.splitn(3, '|');
    let user_id = parts
        .next()
        .ok_or_else(|| ApiError::validation("malformed state"))?;
    let exp = parts
        .next()
        .ok_or_else(|| ApiError::validation("malformed state"))?
        .parse::<i64>()
        .map_err(|_| ApiError::validation("malformed state"))?;
    let return_origin = parts
        .next()
        .ok_or_else(|| ApiError::validation("malformed state"))?
        .to_string();
    validate_return_origin(&return_origin)?;
    if Utc::now().timestamp() > exp {
        return Err(ApiError::validation("oauth state expired"));
    }
    let expected = oauth_signature(&oauth_signing_key(state), payload);
    if sig != expected {
        return Err(ApiError::validation("invalid oauth signature"));
    }
    let user_id = Uuid::parse_str(user_id).map_err(|_| ApiError::validation("invalid user in state"))?;
    Ok((user_id, return_origin))
}

fn oauth_signature(key: &str, payload: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hasher.update(payload.as_bytes());
    format!("{:x}", hasher.finalize())
}

struct DiscordOAuthProfile {
    id: String,
    username: Option<String>,
    global_name: Option<String>,
}

async fn exchange_discord_code(state: &AppState, code: &str) -> Result<String, ApiError> {
    let cfg = effective_discord_config(state);
    let client_id = cfg
        .oauth_client_id
        .as_deref()
        .ok_or_else(|| ApiError::validation("oauth not configured"))?;
    let client_secret = cfg
        .oauth_client_secret
        .as_deref()
        .ok_or_else(|| ApiError::validation("oauth not configured"))?;
    let redirect = oauth_redirect_uri(state);
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

async fn fetch_discord_profile(token: &str) -> Result<DiscordOAuthProfile, ApiError> {
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
    let id = json
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| ApiError::validation("no user id"))?;
    Ok(DiscordOAuthProfile {
        id,
        username: json
            .get("username")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        global_name: json
            .get("global_name")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    })
}

async fn unlink_discord_user(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let removed = state
        .discord
        .lock()
        .unlink_discord_user(user)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    if !removed {
        return Err(ApiError::not_found("discord account not linked"));
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub(crate) fn resolve_link(state: &AppState, ctx: &BridgeContext) -> Result<DiscordSessionLink, ApiError> {
    state
        .discord
        .lock()
        .get_session_link(&ctx.guild_id, &ctx.channel_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?
        .ok_or_else(|| ApiError::not_found("channel not linked to a bunny session"))
}

pub(crate) fn resolve_bunny_user(state: &AppState, ctx: &BridgeContext) -> Result<Uuid, ApiError> {
    let discord = state.discord.lock();
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
    let loc = Locale::En;
    Err(ApiError::forbidden(
        &bunny_i18n::t(loc, "api.error.discord_not_linked", &[]),
    ))
}

pub(crate) fn ensure_discord_control(state: &AppState, user_id: Uuid, session_id: Uuid) -> Result<(), ApiError> {
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
    acting_user_id: Option<Uuid>,
) -> Result<crate::terminals::DiscordShellRunResult, ApiError> {
    let command = command.to_string();
    tokio::task::spawn_blocking(move || {
        crate::terminals::exec_discord_shell_command_run(
            &state,
            term_id,
            session_id,
            &command,
            acting_user_id,
        )
        .map_err(|e| ApiError::validation(&e.to_string()))
    })
    .await
    .map_err(|e| ApiError::validation(&e.to_string()))?
}

/// Server-side cap; Discord bridge paginates (~10 × 1990 chars).
const DISCORD_SHELL_OUTPUT_MAX: usize = 18_000;

fn truncate_discord_shell_output(output: &str) -> String {
    if output.chars().count() <= DISCORD_SHELL_OUTPUT_MAX {
        return output.to_string();
    }
    let truncated: String = output.chars().take(DISCORD_SHELL_OUTPUT_MAX).collect();
    format!("{truncated}\n\n… _(tronqué — fichier complet dans le terminal Web UI)_")
}

pub(crate) fn resolve_shell_terminal(
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

fn session_has_active_chat_channel_links(state: &AppState, session_id: Uuid) -> bool {
    state
        .discord
        .lock()
        .get_link_status_for_session(session_id)
        .map(|links| links.iter().any(|l| l.status == "active"))
        .unwrap_or(false)
}

/// Refuse closing shells tied to active Discord threads or the last channel shell.
pub(crate) fn ensure_shell_may_close(
    state: &AppState,
    session_id: Uuid,
    term_id: Uuid,
    locale: Locale,
) -> Result<(), ApiError> {
    if let Ok(Some(binding)) = state
        .discord
        .lock()
        .get_active_thread_binding_for_term(term_id)
    {
        let mut params = vec![("thread_id", binding.thread_id.as_str())];
        if let Some(goal) = binding.goal_text.as_deref().filter(|g| !g.is_empty()) {
            params.push(("goal", goal));
        }
        return Err(ApiError::validation(&bunny_i18n::t(
            locale,
            "shell.close.blocked_active_thread",
            &params,
        )));
    }

    if session_has_active_chat_channel_links(state, session_id) && !is_thread_bound_shell(state, term_id)
    {
        let bound = thread_bound_term_ids(state, session_id);
        let unbound_count = state
            .auth
            .db()
            .lock()
            .list_terminals_for_session(session_id)
            .unwrap_or_default()
            .into_iter()
            .filter(|(tid, ..)| !bound.contains(tid))
            .count();
        if unbound_count <= 1 {
            return Err(ApiError::validation(&bunny_i18n::t(
                locale,
                "shell.close.blocked_last_channel_shell",
                &[],
            )));
        }
    }

    Ok(())
}

fn is_discord_thread_context(channel_id: &str, thread_id: Option<&str>) -> bool {
    thread_id.is_some_and(|tid| tid != channel_id)
}

fn thread_bound_term_id(
    state: &AppState,
    session_id: Uuid,
    channel_id: &str,
    thread_id: Option<&str>,
) -> Option<Uuid> {
    if !is_discord_thread_context(channel_id, thread_id) {
        return None;
    }
    let tid = thread_id?;
    let binding = state.discord.lock().get_thread_binding(tid).ok().flatten()?;
    if binding.session_id != session_id {
        return None;
    }
    Some(binding.term_id)
}

fn thread_bound_term_ids(state: &AppState, session_id: Uuid) -> HashSet<Uuid> {
    state
        .discord
        .lock()
        .list_thread_bound_term_ids_for_session(session_id)
        .unwrap_or_default()
        .into_iter()
        .collect()
}

fn is_thread_bound_shell(state: &AppState, term_id: Uuid) -> bool {
    state
        .discord
        .lock()
        .is_term_bound_to_active_thread(term_id)
        .unwrap_or(false)
}

fn terminal_name(state: &AppState, term_id: Uuid) -> Option<String> {
    state
        .auth
        .db()
        .lock()
        .get_terminal(term_id)
        .ok()
        .flatten()
        .map(|row| row.2)
}

fn first_unbound_shell(
    state: &AppState,
    session_id: Uuid,
    bound: &HashSet<Uuid>,
) -> Option<Uuid> {
    state
        .auth
        .db()
        .lock()
        .list_terminals_for_session(session_id)
        .unwrap_or_default()
        .into_iter()
        .find_map(|(tid, ..)| (!bound.contains(&tid)).then_some(tid))
}

fn ensure_discord_channel_shell(state: &AppState, session_id: Uuid) -> Result<Uuid, ApiError> {
    create_discord_shell_at_cwd(state, session_id, &crate::terminals::default_shell_cwd())
}

pub(crate) fn create_discord_shell_at_cwd(
    state: &AppState,
    session_id: Uuid,
    cwd: &std::path::Path,
) -> Result<Uuid, ApiError> {
    let name = crate::terminals::next_shell_name(state, session_id);
    let secret_env = state.secret_env_for_session(session_id);
    let (term_id, tmux_target) = state
        .terminals
        .create(session_id, &name, cwd, None, 80, 24, secret_env)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    state.terminal_sessions.write().insert(term_id, session_id);
    crate::terminals::persist_terminal(
        state,
        term_id,
        session_id,
        &name,
        &state.config.terminal.shell,
        None,
        cwd,
        80,
        24,
        tmux_target.as_deref(),
    )
    .map_err(|e| ApiError::validation(&e.to_string()))?;
    crate::terminals::notify_terminal_created(state, session_id, term_id, &name);
    Ok(term_id)
}

fn resolve_channel_default_shell(
    state: &AppState,
    session_id: Uuid,
    guild_id: &str,
    channel_id: &str,
    create_if_missing: bool,
) -> Result<Option<Uuid>, ApiError> {
    let bound = thread_bound_term_ids(state, session_id);
    if let Ok(Some(last)) = state.discord.lock().get_last_shell_name(guild_id, channel_id) {
        if let Ok(id) = resolve_shell_terminal(state, session_id, Some(&last)) {
            if !bound.contains(&id) {
                return Ok(Some(id));
            }
        }
    }
    if let Some(id) = first_unbound_shell(state, session_id, &bound) {
        return Ok(Some(id));
    }
    if create_if_missing {
        return Ok(Some(ensure_discord_channel_shell(state, session_id)?));
    }
    Ok(None)
}

fn default_discord_shell_name(
    state: &AppState,
    session_id: Uuid,
    guild_id: &str,
    channel_id: &str,
    thread_id: Option<&str>,
) -> Option<String> {
    if let Some(term_id) = thread_bound_term_id(state, session_id, channel_id, thread_id) {
        return terminal_name(state, term_id);
    }
    resolve_channel_default_shell(state, session_id, guild_id, channel_id, false)
        .ok()
        .flatten()
        .and_then(|id| terminal_name(state, id))
}

pub(crate) fn resolve_discord_shell(
    state: &AppState,
    session_id: Uuid,
    guild_id: &str,
    channel_id: &str,
    thread_id: Option<&str>,
    shell_name: Option<&str>,
) -> Result<Uuid, ApiError> {
    if let Some(name) = shell_name {
        return resolve_shell_terminal(state, session_id, Some(name));
    }
    if let Some(term_id) = thread_bound_term_id(state, session_id, channel_id, thread_id) {
        return Ok(term_id);
    }
    resolve_channel_default_shell(state, session_id, guild_id, channel_id, true)?
        .ok_or_else(|| ApiError::not_found("no shell — open a shell in the Web UI first"))
}

pub(crate) fn remember_discord_shell(
    state: &AppState,
    guild_id: &str,
    channel_id: &str,
    term_id: Uuid,
) {
    if is_thread_bound_shell(state, term_id) {
        return;
    }
    if let Some(name) = terminal_name(state, term_id) {
        let _ = state
            .discord
            .lock()
            .set_last_shell_name(guild_id, channel_id, &name);
    }
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

async fn internal_thread_bind_route(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<crate::discord_threads::ThreadBindRequest>,
) -> Result<Json<crate::discord_threads::ThreadBindResponse>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    crate::discord_threads::internal_thread_bind(state, body).await
}

async fn internal_thread_input_route(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<crate::discord_threads::ThreadInputRequest>,
) -> Result<Json<crate::discord_threads::ThreadInputResponse>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    crate::discord_threads::internal_thread_input(state, body).await
}

async fn internal_thread_answer_route(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<crate::discord_threads::ThreadAnswerRequest>,
) -> Result<Json<crate::discord_threads::ThreadAnswerResponse>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    crate::discord_threads::internal_thread_answer(state, body).await
}

async fn internal_thread_permission_route(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<crate::discord_threads::ThreadPermissionRequest>,
) -> Result<Json<crate::discord_threads::ThreadPermissionResponse>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    crate::discord_threads::internal_thread_permission(state, body).await
}

async fn internal_thread_discussion_route(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<crate::discord_threads::ThreadDiscussionRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    crate::discord_threads::internal_thread_discussion(state, body).await
}

async fn internal_thread_stop_route(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<crate::discord_threads::ThreadIdRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    crate::discord_threads::internal_thread_stop(state, body).await
}

async fn internal_thread_finalize_route(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<crate::discord_threads::ThreadFinalizeRequest>,
) -> Result<Json<crate::discord_threads::ThreadFinalizeResponse>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    crate::discord_threads::internal_thread_finalize(state, body).await
}

async fn internal_thread_merge_route(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<crate::discord_threads::ThreadIdRequest>,
) -> Result<Json<crate::discord_threads::ThreadMergeResponse>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    crate::discord_threads::internal_thread_merge(state, body).await
}

async fn internal_thread_status_route(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<crate::discord_threads::ThreadIdRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    crate::discord_threads::internal_thread_status(state, body).await
}

async fn internal_thread_attachment_route(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    crate::discord_threads::internal_thread_attachment(state, body).await
}

async fn internal_project_set_route(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<crate::discord_threads::ProjectPathRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    crate::discord_threads::internal_project_set(state, body).await
}

async fn internal_project_get_route(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(ctx): Query<BridgeContext>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    crate::discord_threads::internal_project_get(state, ctx).await
}

async fn internal_git_route(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<crate::discord_threads::GitCommandRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    verify_bridge_token(&state, &headers)?;
    crate::discord_threads::internal_git_command(state, body).await
}

