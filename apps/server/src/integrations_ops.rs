//! Integrations API — approvals, activity feed, GitHub OAuth, unified chat internal routes.

use crate::api::{ensure_session_access, ApiError};
use crate::approval_service::ApprovalService;
use crate::discord_ops;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use bunny_core::permissions::Action;
use bunny_integrations::{LinkContext, SessionActivityEntry};
use bunny_policy::ApproverPolicy;
use chrono::Utc;
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

pub fn human_router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/sessions/:id/approvals", get(list_session_approvals))
        .route("/sessions/:id/activity", get(list_session_activity))
        .route("/approvals/:id/resolve", post(web_resolve_approval))
        .route("/integrations/providers", get(list_providers))
        .route("/integrations/github/oauth/start", get(github_oauth_start))
        .route("/integrations/github/oauth/callback", get(github_oauth_callback))
        .route("/integrations/github/repos", get(list_github_repos))
        .route("/sessions/:id/integrations/github/bind", post(bind_github_repo))
        .with_state(state)
}

pub fn internal_chat_router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .nest("/discord", discord_ops::internal_router(state.clone()))
        .nest("/slack", internal_slack_router(state.clone()))
        .nest("/teams", internal_teams_router(state))
}

fn internal_slack_router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/health", get(slack_health))
        .route("/approval/resolve", post(slack_approval_resolve))
        .with_state(state)
}

fn internal_teams_router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/health", get(teams_health))
        .route("/approval/resolve", post(teams_approval_resolve))
        .with_state(state)
}

async fn slack_health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "bridge": "slack", "ok": true }))
}

async fn teams_health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "bridge": "teams", "ok": true }))
}

#[derive(Deserialize)]
struct ChatApprovalResolve {
    approval_id: String,
    approve: bool,
    external_user_id: String,
}

async fn slack_approval_resolve(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(body): Json<ChatApprovalResolve>,
) -> Result<Json<serde_json::Value>, ApiError> {
    resolve_chat_approval(&state, &headers, "slack", body).await
}

async fn teams_approval_resolve(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(body): Json<ChatApprovalResolve>,
) -> Result<Json<serde_json::Value>, ApiError> {
    resolve_chat_approval(&state, &headers, "teams", body).await
}

async fn resolve_chat_approval(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    bridge_id: &str,
    body: ChatApprovalResolve,
) -> Result<Json<serde_json::Value>, ApiError> {
    discord_ops::verify_bridge_token(state, headers)?;
    let bunny_user = state
        .integrations
        .lock()
        .get_chat_account_link(bridge_id, &body.external_user_id)?
        .ok_or_else(|| ApiError::forbidden("account not linked"))?;
    let approval_id =
        Uuid::parse_str(&body.approval_id).map_err(|_| ApiError::validation("approval_id"))?;
    let outcome = ApprovalService::resolve(state, approval_id, body.approve, bunny_user)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "bridge": bridge_id,
        "output": outcome.output,
        "exit_code": outcome.exit_code,
    })))
}

async fn list_session_approvals(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<Uuid>,
    user: axum::Extension<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ensure_session_access(&state, *user, session_id, Action::SessionRead)?;
    let rows = state
        .integrations
        .lock()
        .list_pending_approvals(session_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    let items: Vec<_> = rows
        .into_iter()
        .map(|(id, summary, reason, status)| {
            serde_json::json!({
                "id": id.to_string(),
                "actionSummary": summary,
                "reason": reason,
                "status": status,
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "approvals": items })))
}

#[derive(Deserialize)]
struct ActivityQuery {
    limit: Option<usize>,
}

async fn list_session_activity(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<Uuid>,
    user: axum::Extension<Uuid>,
    Query(q): Query<ActivityQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ensure_session_access(&state, *user, session_id, Action::SessionRead)?;
    let limit = q.limit.unwrap_or(50);
    let entries = state
        .integrations
        .lock()
        .list_activity(session_id, limit)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(serde_json::json!({ "activity": entries })))
}

#[derive(Deserialize)]
struct WebApprovalResolve {
    approve: bool,
}

async fn web_resolve_approval(
    State(state): State<Arc<AppState>>,
    Path(approval_id): Path<Uuid>,
    user: axum::Extension<Uuid>,
    Json(body): Json<WebApprovalResolve>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let approval = state
        .discord
        .lock()
        .get_approval(approval_id)?
        .ok_or_else(|| ApiError::not_found("approval not found"))?;
    ensure_session_access(
        &state,
        *user,
        approval.session_id,
        Action::ActionApprove,
    )?;
    let outcome = ApprovalService::resolve(&state, approval_id, body.approve, *user)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "output": outcome.output,
        "exitCode": outcome.exit_code,
    })))
}

async fn list_providers(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "chatBridges": state.chat_hub.registry().list_chat_bridges(),
        "toolProviders": state.chat_hub.registry().list_tool_providers(),
    }))
}

#[derive(Deserialize)]
struct OAuthStartQuery {
    redirect_uri: String,
}

async fn github_oauth_start(
    State(state): State<Arc<AppState>>,
    user: axum::Extension<Uuid>,
    Query(q): Query<OAuthStartQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let _ = user;
    let provider = state
        .chat_hub
        .registry()
        .tool_provider("github")
        .ok_or_else(|| ApiError::validation("github provider not registered"))?;
    let oauth = provider.oauth_config();
    let client_id = std::env::var("BUNNY_GITHUB_CLIENT_ID")
        .or_else(|_| std::env::var("GITHUB_CLIENT_ID"))
        .unwrap_or_default();
    if client_id.is_empty() {
        return Err(ApiError::validation("GitHub OAuth not configured"));
    }
    let scope = oauth.scopes.join("+");
    let url = format!(
        "{}?client_id={}&redirect_uri={}&scope={}",
        oauth.authorize_url,
        urlencoding::encode(&client_id),
        urlencoding::encode(&q.redirect_uri),
        urlencoding::encode(&scope),
    );
    Ok(Json(serde_json::json!({ "authorizeUrl": url })))
}

#[derive(Deserialize)]
struct OAuthCallbackQuery {
    code: String,
    redirect_uri: String,
}

async fn github_oauth_callback(
    State(state): State<Arc<AppState>>,
    user: axum::Extension<Uuid>,
    Query(q): Query<OAuthCallbackQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let provider = state
        .chat_hub
        .registry()
        .tool_provider("github")
        .ok_or_else(|| ApiError::validation("github provider not registered"))?;
    let identity = provider
        .link_account(&LinkContext {
            user_id: *user,
            oauth_code: q.code,
            redirect_uri: q.redirect_uri,
        })
        .await
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    state
        .integrations
        .lock()
        .insert_integration_account_link(
            "github",
            &identity.external_user_id,
            *user,
            &serde_json::to_string(&identity).unwrap_or_default(),
        )
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true, "identity": identity })))
}

async fn list_github_repos(
    State(state): State<Arc<AppState>>,
    user: axum::Extension<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let _link = state
        .integrations
        .lock()
        .get_integration_account_link("github", *user)
        .map_err(|e| ApiError::validation(&e.to_string()))?
        .ok_or_else(|| ApiError::forbidden("GitHub account not linked"))?;
    Ok(Json(serde_json::json!({ "repos": [] })))
}

#[derive(Deserialize)]
struct BindGithubRepo {
    repo: String,
}

async fn bind_github_repo(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<Uuid>,
    user: axum::Extension<Uuid>,
    Json(body): Json<BindGithubRepo>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ensure_session_access(&state, *user, session_id, Action::IntegrationManage)?;
    let binding = bunny_integrations::ResourceBinding {
        id: Uuid::new_v4(),
        session_id,
        installation_id: Uuid::nil(),
        resource_type: "repo".into(),
        resource_ref: format!("repo:{}", body.repo),
        config: serde_json::json!({}),
    };
    state
        .integrations
        .lock()
        .upsert_resource_binding(&binding)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    state
        .integrations
        .lock()
        .upsert_git_repo_binding(
            Uuid::new_v4(),
            session_id,
            "remote",
            None,
            Some(&format!("https://github.com/{}.git", body.repo)),
            "main",
            None,
        )
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    let _ = state.integrations.lock().record_activity(&SessionActivityEntry {
        id: Uuid::new_v4(),
        session_id,
        kind: "integration.bind".into(),
        summary: format!("Linked GitHub repo {}", body.repo),
        ref_type: Some("resource_binding".into()),
        ref_id: Some(binding.id.to_string()),
        bridge_id: None,
        ts: Utc::now(),
    });
    Ok(Json(serde_json::json!({ "ok": true, "bindingId": binding.id.to_string() })))
}

pub fn store_approval_policy(
    state: &AppState,
    approval_id: Uuid,
    policy: &ApproverPolicy,
) {
    if let Ok(json) = serde_json::to_string(policy) {
        let _ = state
            .integrations
            .lock()
            .set_approval_policy(approval_id, &json);
    }
}
