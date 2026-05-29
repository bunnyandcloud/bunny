use crate::middleware;
use crate::novnc_proxy;
use crate::preview;
use crate::secrets_ops::{
    self, ensure_secrets_access, init_vault, list_secrets, lock_vault, reveal_secret, remove_secret,
    unlock_vault, upsert_secret, SecretMetaResponse, SecretRevealResponse, VaultStatusResponse,
};
use crate::state::AppState;
use crate::terminals::{
    default_session_path_label, default_shell_cwd, ensure_session_terminals_live, persist_terminal,
    remove_terminal_record, teardown_session,
};
use crate::webrtc::{IceCandidatePayload, SdpPayload, WebRtcConfigResponse};
use crate::ws;
use axum::{
    extract::{Extension, Path, Query, State, WebSocketUpgrade},
    http::StatusCode,
    middleware::from_fn_with_state,
    response::{IntoResponse, Redirect, Response},
    routing::{delete, get, get_service, patch, post},
    Json, Router,
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use bunny_auth::{AuthenticatedSession, LoginStep};
use bunny_core::permissions::{role_can, Action};
use bunny_core::types::{ApiErrorResponse, Role};
use bunny_core::API_VERSION;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use axum::http::HeaderValue;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use uuid::Uuid;

pub fn router(state: Arc<AppState>, web_dist: Option<std::path::PathBuf>) -> Router {
    let public = Router::new()
        .route("/auth/bootstrap", post(auth_bootstrap))
        .route("/auth/login", post(auth_login))
        .route("/auth/mfa/verify", post(auth_mfa_verify))
        .route("/invitations/accept", post(invitation_accept))
        .route("/agent/info", get(agent_info))
        .route("/claude/oauth/redirect/:token", get(claude_oauth_redirect));

    let protected = Router::new()
        .route("/auth/logout", post(auth_logout))
        .route("/auth/me", get(auth_me))
        .route("/auth/mfa/status", get(auth_mfa_status))
        .route("/auth/mfa/setup", post(auth_mfa_setup))
        .route("/auth/mfa/enable", post(auth_mfa_enable))
        .route("/auth/mfa/disable", post(auth_mfa_disable))
        .route("/auth/mfa/recovery/regenerate", post(auth_mfa_recovery_regenerate))
        .route("/sessions", post(create_session).get(list_sessions))
        .route(
            "/sessions/:id",
            get(get_session).patch(patch_session).delete(delete_session),
        )
        .route("/sessions/:id/join", post(join_session))
        .route("/sessions/:id/invitations", post(create_invitation))
        .route("/sessions/:id/members", get(list_session_members))
        .route(
            "/sessions/:id/members/:user_id",
            patch(update_session_member).delete(remove_session_member),
        )
        .route("/sessions/:id/stop", post(stop_session))
        .route("/sessions/:id/reset", post(reset_session))
        .route("/sessions/:id/realtime", get(session_realtime))
        .route("/terminals", post(create_terminal).get(list_terminals))
        .route(
            "/terminals/:id",
            get(get_terminal).patch(patch_terminal).delete(delete_terminal),
        )
        .route("/terminals/:id/input", post(terminal_input))
        .route("/terminals/:id/resize", post(terminal_resize))
        .route("/terminals/:id/restart", post(terminal_restart))
        .route("/terminals/:id/ws", get(terminal_ws))
        .route("/previews", post(create_preview).get(list_previews))
        .route("/previews/:id", delete(delete_preview))
        .route("/browser-sessions", post(create_browser))
        .route("/browser-sessions/:id", get(get_browser))
        .route("/browser-sessions/:id/control", post(browser_control))
        .route("/browser-sessions/:id/restart", post(browser_restart))
        .route("/browser-sessions/:id/reset", post(browser_reset))
        .route("/browser-sessions/:id/events", get(browser_events_ws))
        .route("/browser-sessions/:id/webrtc/offer", post(browser_webrtc_offer))
        .route("/browser-sessions/:id/webrtc/candidate", post(browser_webrtc_candidate))
        .route("/browser-sessions/:id/webrtc/stop", post(browser_webrtc_stop))
        .route("/browser-sessions/:id/vnc/ws", get(browser_novnc_ws))
        .route("/browser-sessions/:id/vnc/*path", get(novnc_proxy::http_proxy))
        .route("/browser-sessions/:id/vnc", get(novnc_proxy::http_proxy_root))
        .route("/timeline", get(get_timeline))
        .route("/audit-logs", get(get_audit_logs))
        .route("/secrets/status", get(secrets_status))
        .route("/secrets/init", post(secrets_init))
        .route("/secrets/unlock", post(secrets_unlock))
        .route("/secrets/lock", post(secrets_lock))
        .route("/secrets", get(secrets_list).post(secrets_upsert))
        .route("/secrets/:name/reveal", get(secrets_reveal))
        .route("/secrets/:name", delete(secrets_delete))
        .route("/voice/intent", post(voice_intent))
        .route("/voice/confirm", post(voice_confirm))
        .route("/push/register", post(push_register))
        .route("/push/register/:device_id", delete(push_unregister))
        .route("/claude/status", get(claude_status))
        .route("/claude/install", post(claude_install))
        .route("/claude/auth/start", post(claude_auth_start))
        .route("/claude/auth/code", post(claude_auth_code))
        .route("/claude/auth/detect-code", post(claude_auth_detect_code))
        .route("/users", get(list_users).post(create_user).patch(update_user))
        .route("/users/:user_id", delete(revoke_user))
        .route("/webrtc/config", get(webrtc_config))
        .route("/sessions/:id/webrtc/offer", post(webrtc_offer))
        .route("/sessions/:id/webrtc/candidate", post(webrtc_candidate))
        .layer(from_fn_with_state(state.clone(), middleware::require_auth));

    let api = public.merge(protected).with_state(state.clone());

    let preview_routes = preview::router(state.clone())
        .merge(preview::root_dev_assets_router(state.clone()))
        .merge(preview::root_public_assets_router(state.clone()))
        .layer(from_fn_with_state(state.clone(), middleware::require_auth));

    let static_files = if let Some(dist) = web_dist.filter(|d| d.join("index.html").is_file()) {
        let index_html = dist.join("index.html");
        Router::new()
            // Serve SPA directly (preserve ?invite=…&email=… query string).
            .route("/login", get_service(ServeFile::new(index_html.clone())))
            .nest_service("/assets", ServeDir::new(dist.join("assets")))
            .fallback_service(ServeFile::new(index_html))
    } else {
        Router::new().route(
            "/",
            get(|| async {
                "bunny API running. Run: bunny run  (or: cd apps/web && npm run build)"
            }),
        )
    };

    Router::new()
        .nest(&format!("/api/{API_VERSION}"), api)
        .merge(preview_routes)
        .merge(static_files)
        .layer(cors_layer())
        .with_state(state)
}

/// Dev origins for Vite (5173) and same-host UI (7681). Credentials cannot use `*`.
fn cors_layer() -> CorsLayer {
    const ORIGINS: &[&str] = &[
        "http://127.0.0.1:5173",
        "http://localhost:5173",
        "http://127.0.0.1:7681",
        "http://localhost:7681",
    ];
    let origins: Vec<HeaderValue> = ORIGINS.iter().filter_map(|o| o.parse().ok()).collect();
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_credentials(true)
}

async fn auth_bootstrap(
    State(state): State<Arc<AppState>>,
    Json(body): Json<BootstrapRequest>,
) -> Result<Json<BootstrapResponse>, ApiError> {
    if !state.auth.needs_bootstrap()? {
        return Err(ApiError::conflict("owner already exists"));
    }
    let id = state.auth.bootstrap_owner(&body.email, &body.password)?;
    Ok(Json(BootstrapResponse {
        user_id: id.to_string(),
        message: "owner account created".into(),
    }))
}

fn session_cookie(token: &str) -> Cookie<'static> {
    let mut cookie = Cookie::new("bunny_session", token.to_string());
    cookie.set_http_only(true);
    cookie.set_path("/");
    cookie.set_same_site(SameSite::Lax);
    cookie
}

fn mfa_challenge_cookie(token: &str) -> Cookie<'static> {
    let mut cookie = Cookie::new("bunny_mfa_challenge", token.to_string());
    cookie.set_http_only(true);
    cookie.set_path("/");
    cookie.set_same_site(SameSite::Lax);
    cookie
}

async fn auth_login(
    State(state): State<Arc<AppState>>,
    Json(body): Json<LoginRequest>,
) -> Result<(CookieJar, Json<serde_json::Value>), ApiError> {
    match state
        .auth
        .login(&body.email, &body.password, body.device_id.as_deref())
        .map_err(map_auth_error)?
    {
        LoginStep::Complete(result) => {
            let jar = CookieJar::new().add(session_cookie(&result.session_token));
            Ok((
                jar,
                Json(serde_json::json!({
                    "user_id": result.user_id.to_string(),
                    "email": result.email,
                    "session_token": result.session_token,
                    "expires_at": result.expires_at.to_rfc3339(),
                    "mfa_required": false,
                })),
            ))
        }
        LoginStep::MfaRequired {
            challenge_token,
            user_id,
            email,
        } => {
            let jar = CookieJar::new().add(mfa_challenge_cookie(&challenge_token));
            Ok((
                jar,
                Json(serde_json::json!({
                    "mfa_required": true,
                    "mfa_challenge_token": challenge_token,
                    "user_id": user_id.to_string(),
                    "email": email,
                })),
            ))
        }
    }
}

async fn auth_mfa_verify(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Json(body): Json<MfaVerifyRequest>,
) -> Result<(CookieJar, Json<serde_json::Value>), ApiError> {
    let challenge = body
        .mfa_challenge_token
        .as_deref()
        .or_else(|| jar.get("bunny_mfa_challenge").map(|c| c.value()))
        .ok_or_else(|| ApiError::validation("mfa challenge required"))?;
    let result = state
        .auth
        .verify_mfa_login_with_failure(challenge, &body.code, body.device_id.as_deref())
        .map_err(map_auth_error)?;
    let mut out_jar = CookieJar::new()
        .add(session_cookie(&result.session_token))
        .remove(Cookie::from("bunny_mfa_challenge"));
    Ok((
        out_jar,
        Json(serde_json::json!({
            "user_id": result.user_id.to_string(),
            "email": result.email,
            "session_token": result.session_token,
            "expires_at": result.expires_at.to_rfc3339(),
        })),
    ))
}

async fn auth_mfa_status(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
) -> Result<Json<MfaStatusResponse>, ApiError> {
    let status = state.auth.mfa_status(user).map_err(map_auth_error)?;
    Ok(Json(MfaStatusResponse {
        enabled: status.enabled,
        recovery_remaining: status.recovery_remaining,
    }))
}

async fn auth_mfa_setup(
    State(state): State<Arc<AppState>>,
    Extension(session): Extension<AuthenticatedSession>,
    Json(body): Json<RecentAuthRequest>,
) -> Result<Json<MfaSetupResponse>, ApiError> {
    state
        .auth
        .assert_recent_auth(&session, body.password.as_deref())
        .map_err(map_auth_error)?;
    let setup = state.auth.mfa_setup_begin(session.user_id).map_err(map_auth_error)?;
    Ok(Json(MfaSetupResponse {
        otpauth_uri: setup.otpauth_uri,
        secret_base32: setup.secret_base32,
    }))
}

async fn auth_mfa_enable(
    State(state): State<Arc<AppState>>,
    Extension(session): Extension<AuthenticatedSession>,
    Json(body): Json<MfaEnableRequest>,
) -> Result<Json<MfaEnableResponse>, ApiError> {
    state
        .auth
        .assert_recent_auth(&session, body.password.as_deref())
        .map_err(map_auth_error)?;
    let codes = state
        .auth
        .mfa_setup_confirm(session.user_id, &body.code)
        .map_err(map_auth_error)?;
    Ok(Json(MfaEnableResponse { recovery_codes: codes }))
}

async fn auth_mfa_disable(
    State(state): State<Arc<AppState>>,
    Extension(session): Extension<AuthenticatedSession>,
    Json(body): Json<MfaDisableRequest>,
) -> Result<Json<OkResponse>, ApiError> {
    state
        .auth
        .assert_recent_auth(&session, body.password.as_deref())
        .map_err(map_auth_error)?;
    state
        .auth
        .mfa_disable(session.user_id, &body.code)
        .map_err(map_auth_error)?;
    Ok(Json(OkResponse { ok: true }))
}

async fn auth_mfa_recovery_regenerate(
    State(state): State<Arc<AppState>>,
    Extension(session): Extension<AuthenticatedSession>,
    Json(body): Json<MfaDisableRequest>,
) -> Result<Json<MfaEnableResponse>, ApiError> {
    state
        .auth
        .assert_recent_auth(&session, body.password.as_deref())
        .map_err(map_auth_error)?;
    let codes = state
        .auth
        .mfa_regenerate_recovery(session.user_id, &body.code)
        .map_err(map_auth_error)?;
    Ok(Json(MfaEnableResponse { recovery_codes: codes }))
}

fn map_auth_error(e: anyhow::Error) -> ApiError {
    let msg = e.to_string();
    if msg.contains("too many attempts") {
        return ApiError::too_many_requests(&msg);
    }
    if msg.contains("recent authentication") {
        return ApiError::forbidden(&msg);
    }
    if msg.contains("already enabled") || msg.contains("not enabled") {
        return ApiError::conflict(&msg);
    }
    if msg.contains("invalid") || msg.contains("expired") || msg.contains("credentials") {
        return ApiError::unauthorized_msg(&msg);
    }
    ApiError::validation(&msg)
}

async fn auth_logout(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> Result<CookieJar, ApiError> {
    if let Some(c) = jar.get("bunny_session") {
        state.auth.logout(c.value())?;
    }
    Ok(jar
        .remove(Cookie::from("bunny_session"))
        .remove(Cookie::from("bunny_mfa_challenge")))
}

async fn auth_me(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
) -> Result<Json<MeResponse>, ApiError> {
    let (email, created_at) = state.auth.me(user).map_err(|_| ApiError::unauthorized())?;
    let is_owner = state
        .auth
        .owner_id()
        .map(|owner| owner == user)
        .unwrap_or(false);
    let mfa_enabled = state
        .auth
        .mfa_status(user)
        .map(|s| s.enabled)
        .unwrap_or(false);
    let profile = state
        .auth
        .db()
        .lock()
        .get_user_profile(user)
        .ok()
        .flatten();
    let (can_install_claude, can_manage_vault, can_create_sessions, default_session_role) =
        if is_owner {
            (true, true, true, "owner".to_string())
        } else if let Some(p) = profile {
            (
                p.can_install_claude,
                p.can_manage_vault,
                p.can_create_sessions,
                format!("{:?}", p.default_session_role).to_lowercase(),
            )
        } else {
            (false, false, false, "viewer".to_string())
        };
    Ok(Json(MeResponse {
        user_id: user.to_string(),
        email,
        created_at: created_at.to_rfc3339(),
        is_owner,
        mfa_enabled,
        can_install_claude,
        can_manage_vault,
        can_create_sessions,
        default_session_role,
    }))
}

async fn list_users(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
) -> Result<Json<Vec<UserAdminItem>>, ApiError> {
    ensure_system_owner(&state, user)?;
    let owner_id = state.auth.owner_id().map_err(|_| ApiError::forbidden("permission denied"))?;
    let users = state
        .auth
        .db()
        .lock()
        .list_users()
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(
        users
            .into_iter()
            .map(|row| UserAdminItem {
                id: row.id.to_string(),
                email: row.email,
                disabled: row.disabled_at.is_some(),
                is_system_owner: row.id == owner_id,
                can_install_claude: row.can_install_claude,
                can_manage_vault: row.can_manage_vault,
                can_create_sessions: row.can_create_sessions,
                default_session_role: format!("{:?}", row.default_session_role).to_lowercase(),
            })
            .collect(),
    ))
}

async fn create_user(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Json(body): Json<CreateUserRequest>,
) -> Result<Json<CreateUserResponse>, ApiError> {
    ensure_system_owner(&state, user)?;
    let role = bunny_core::permissions::parse_role(&body.default_session_role)
        .ok_or_else(|| ApiError::validation("invalid default_session_role"))?;
    if matches!(role, bunny_core::types::Role::Owner | bunny_core::types::Role::Agent) {
        return Err(ApiError::validation(
            "default_session_role must be admin, editor, or viewer",
        ));
    }
    let token = state
        .auth
        .invite_team_user(
            &body.email,
            role,
            body.can_install_claude,
            body.can_manage_vault,
            body.can_create_sessions,
            user,
        )
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(CreateUserResponse { token }))
}

async fn update_user(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Json(body): Json<UserAdminUpdateRequest>,
) -> Result<Json<OkResponse>, ApiError> {
    ensure_system_owner(&state, user)?;
    let user_id = Uuid::parse_str(&body.user_id).map_err(|_| ApiError::validation("user_id"))?;
    let owner_id = state.auth.owner_id().map_err(|_| ApiError::forbidden("permission denied"))?;
    if user_id == owner_id {
        return Err(ApiError::forbidden("cannot change system owner permissions"));
    }
    let role = bunny_core::permissions::parse_role(&body.default_session_role)
        .ok_or_else(|| ApiError::validation("invalid default_session_role"))?;
    if matches!(role, bunny_core::types::Role::Owner | bunny_core::types::Role::Agent) {
        return Err(ApiError::validation(
            "default_session_role must be admin, editor, or viewer",
        ));
    }
    state
        .auth
        .db()
        .lock()
        .set_user_team_settings(
            user_id,
            body.can_install_claude,
            body.can_manage_vault,
            body.can_create_sessions,
            role,
        )
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(OkResponse { ok: true }))
}

async fn revoke_user(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<OkResponse>, ApiError> {
    ensure_system_owner(&state, user)?;
    state
        .auth
        .revoke_user_by_id(user_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(OkResponse { ok: true }))
}

fn normalize_label(name: &str, field: &str) -> Result<String, ApiError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ApiError::validation(&format!("{field} cannot be empty")));
    }
    if trimmed.len() > 64 {
        return Err(ApiError::validation(&format!("{field} must be at most 64 characters")));
    }
    Ok(trimmed.to_string())
}

fn default_session_name(id: Uuid) -> String {
    format!("Session {}", &id.to_string()[..8])
}

fn session_display_name(id: Uuid, stored: Option<String>) -> String {
    stored
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| default_session_name(id))
}

async fn create_session(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Json(body): Json<CreateSessionRequest>,
) -> Result<Json<SessionResponse>, ApiError> {
    ensure_can_create_sessions(&state, user)?;
    let path = body
        .project_path
        .unwrap_or_else(default_session_path_label);
    let name = body
        .name
        .as_deref()
        .map(|n| normalize_label(n, "name"))
        .transpose()?;
    let id = state.auth.create_stream_session(user, &path, name.as_deref())?;
    let display = session_display_name(id, name);
    let _ = state.record_timeline(
        id,
        "session",
        "session.created",
        serde_json::json!({ "path": path, "name": display }),
    );
    Ok(Json(SessionResponse {
        id: id.to_string(),
        name: display,
        login_url: format!("/login?next=/s/{}", id),
        auth_required: true,
    }))
}

async fn list_sessions(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
) -> Result<Json<Vec<SessionListItem>>, ApiError> {
    let sessions = state.auth.db().lock().list_stream_sessions(user)?;
    Ok(Json(
        sessions
            .into_iter()
            .map(|(id, name, path, status)| SessionListItem {
                id: id.to_string(),
                name: session_display_name(id, name),
                project_path: path,
                status,
            })
            .collect(),
    ))
}

async fn get_session(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(id): Path<Uuid>,
) -> Result<Json<SessionListItem>, ApiError> {
    ensure_session_access(&state, user, id, Action::SessionRead)?;
    let sessions = state.auth.db().lock().list_stream_sessions(user)?;
    sessions
        .into_iter()
        .find(|(sid, _, _, _)| *sid == id)
        .map(|(id, name, path, status)| SessionListItem {
            id: id.to_string(),
            name: session_display_name(id, name),
            project_path: path,
            status,
        })
        .ok_or_else(|| ApiError::not_found("session"))
        .map(Json)
}

async fn patch_session(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(id): Path<Uuid>,
    Json(body): Json<RenameRequest>,
) -> Result<Json<SessionListItem>, ApiError> {
    ensure_session_access(&state, user, id, Action::SessionUpdate)?;
    let name = normalize_label(&body.name, "name")?;
    state
        .auth
        .db()
        .lock()
        .update_stream_session_name(id, &name)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    let sessions = state.auth.db().lock().list_stream_sessions(user)?;
    sessions
        .into_iter()
        .find(|(sid, _, _, _)| *sid == id)
        .map(|(id, _, path, status)| SessionListItem {
            id: id.to_string(),
            name: name.clone(),
            project_path: path,
            status,
        })
        .ok_or_else(|| ApiError::not_found("session"))
        .map(Json)
}

async fn join_session(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ensure_session_access(&state, user, id, Action::SessionRead)?;
    Ok(Json(serde_json::json!({ "joined": true, "sessionId": id })))
}

async fn create_invitation(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateInvitationRequest>,
) -> Result<Json<CreateInvitationResponse>, ApiError> {
    ensure_session_access(&state, user, id, Action::UsersManage)?;
    let role = bunny_core::permissions::parse_role(&body.role)
        .ok_or_else(|| ApiError::validation("invalid role"))?;
    let token = state
        .auth
        .invite_user(id, &body.email, role, user)
        .map_err(|e| ApiError::forbidden(&e.to_string()))?;
    Ok(Json(CreateInvitationResponse { token }))
}

async fn invitation_accept(
    State(state): State<Arc<AppState>>,
    Json(body): Json<InvitationAcceptRequest>,
) -> Result<(CookieJar, Json<InvitationAcceptResponse>), ApiError> {
    let result = state
        .auth
        .accept_invitation(
            &body.token,
            &body.email,
            &body.password,
            body.device_id.as_deref(),
        )
        .map_err(map_auth_error)?;
    let jar = CookieJar::new().add(session_cookie(&result.session_token));
    Ok((
        jar,
        Json(InvitationAcceptResponse {
            user_id: result.user_id.to_string(),
            email: result.email,
            session_id: result.session_id.map(|id| id.to_string()),
            role: format!("{:?}", result.role).to_lowercase(),
            expires_at: result.expires_at.to_rfc3339(),
        }),
    ))
}

async fn list_session_members(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<SessionMemberResponse>>, ApiError> {
    ensure_session_access(&state, user, id, Action::UsersManage)?;
    let members = state
        .auth
        .db()
        .lock()
        .list_session_members(id)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(
        members
            .into_iter()
            .map(|(user_id, email, role)| SessionMemberResponse {
                user_id: user_id.to_string(),
                email,
                role: format!("{:?}", role).to_lowercase(),
            })
            .collect(),
    ))
}

async fn update_session_member(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path((id, user_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateSessionMemberRequest>,
) -> Result<Json<OkResponse>, ApiError> {
    ensure_session_access(&state, user, id, Action::UsersManage)?;
    let role = bunny_core::permissions::parse_role(&body.role)
        .ok_or_else(|| ApiError::validation("invalid role"))?;
    state
        .auth
        .db()
        .lock()
        .add_session_member(id, user_id, role)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(OkResponse { ok: true }))
}

async fn remove_session_member(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path((id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<OkResponse>, ApiError> {
    ensure_session_access(&state, user, id, Action::UsersManage)?;
    state
        .auth
        .db()
        .lock()
        .remove_session_member(id, user_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(OkResponse { ok: true }))
}

async fn stop_session(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ensure_session_access(&state, user, id, Action::SessionStop)?;
    teardown_session(&state, id).map_err(|e| ApiError::validation(&e.to_string()))?;
    let auth_db = state.auth.db();
    auth_db
        .lock()
        .update_stream_session_status(id, "stopped")
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(serde_json::json!({ "stopped": true })))
}

async fn delete_session(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    ensure_session_access(&state, user, id, Action::SessionDelete)?;
    teardown_session(&state, id).map_err(|e| ApiError::validation(&e.to_string()))?;
    let auth_db = state.auth.db();
    auth_db
        .lock()
        .delete_stream_session(id)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn reset_session(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ensure_session_access(&state, user, id, Action::SessionReset)?;
    Ok(Json(serde_json::json!({ "resetting": true, "sessionId": id })))
}

async fn session_realtime(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    ensure_session_access(&state, user, id, Action::SessionRead)?;
    Ok(ws.on_upgrade(move |socket| ws::handle_session_realtime(socket, state, id)))
}

async fn create_terminal(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Json(body): Json<CreateTerminalRequest>,
) -> Result<Json<TerminalResponse>, ApiError> {
    let session_id = Uuid::parse_str(&body.session_id).map_err(|_| ApiError::validation("session_id"))?;
    ensure_session_access(&state, user, session_id, Action::TerminalWrite)?;
    let cwd = body
        .cwd
        .map(std::path::PathBuf::from)
        .unwrap_or_else(default_shell_cwd);
    let cols = body.cols.unwrap_or(80);
    let rows = body.rows.unwrap_or(24);
    let secret_env = state.secret_env_for_session(session_id);
    let (term_id, tmux_target) = state.terminals.create(
        session_id,
        &body.name,
        &cwd,
        body.command.as_deref(),
        cols,
        rows,
        secret_env,
    )?;
    state.terminal_sessions.write().insert(term_id, session_id);
    persist_terminal(
        &state,
        term_id,
        session_id,
        &body.name,
        &state.config.terminal.shell,
        body.command.as_deref(),
        &cwd,
        cols,
        rows,
        tmux_target.as_deref(),
    )
    .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(TerminalResponse {
        id: term_id.to_string(),
        name: body.name,
        ws_url: format!("/api/v1/terminals/{term_id}/ws"),
    }))
}

async fn list_terminals(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Query(q): Query<ListTerminalsQuery>,
) -> Result<Json<Vec<TerminalListItem>>, ApiError> {
    let filter_session = if let Some(s) = &q.session_id {
        let sid = Uuid::parse_str(s).map_err(|_| ApiError::validation("session_id"))?;
        ensure_session_access(&state, user, sid, Action::TerminalRead)?;
        ensure_session_terminals_live(&state, sid);
        Some(sid)
    } else {
        None
    };

    let db_rows = if let Some(sid) = filter_session {
        state
            .auth
            .db()
            .lock()
            .list_terminals_for_session(sid)
            .unwrap_or_default()
    } else {
        vec![]
    };

    let mut items: Vec<TerminalListItem> = db_rows
        .into_iter()
        .map(|row: bunny_auth::db::TerminalRow| {
            let (id, _, name, _, _, _, db_status, _, _, _) = row;
            TerminalListItem {
                id: id.to_string(),
                name,
                status: state
                    .terminals
                    .status(id)
                    .map(|s| format!("{:?}", s))
                    .unwrap_or(db_status),
            }
        })
        .collect();

    if filter_session.is_some() {
        let known: std::collections::HashSet<String> =
            items.iter().map(|i| i.id.clone()).collect();
        for (tid, sid) in state.terminal_sessions.read().iter() {
            if filter_session != Some(*sid) {
                continue;
            }
            if known.contains(&tid.to_string()) {
                continue;
            }
            if let (Some(name), Some(status)) = (state.terminals.name(*tid), state.terminals.status(*tid)) {
                items.push(TerminalListItem {
                    id: tid.to_string(),
                    name,
                    status: format!("{:?}", status),
                });
            }
        }
    }

    Ok(Json(items))
}

async fn get_terminal(
    State(state): State<Arc<AppState>>,
    Extension(_user): Extension<Uuid>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let status = state
        .terminals
        .status(id)
        .ok_or_else(|| ApiError::not_found("terminal"))?;
    let name = state
        .terminals
        .name(id)
        .or_else(|| {
            state
                .auth
                .db()
                .lock()
                .get_terminal(id)
                .ok()
                .flatten()
                .map(|row| row.2)
        });
    Ok(Json(serde_json::json!({
        "id": id,
        "name": name,
        "status": format!("{:?}", status)
    })))
}

async fn patch_terminal(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(id): Path<Uuid>,
    Json(body): Json<RenameRequest>,
) -> Result<Json<TerminalListItem>, ApiError> {
    let session_id = terminal_session_id(&state, id)?;
    ensure_session_access(&state, user, session_id, Action::TerminalWrite)?;
    let name = normalize_label(&body.name, "name")?;
    state.terminals.set_name(id, name.clone());
    state
        .auth
        .db()
        .lock()
        .update_terminal_name(id, &name)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    let status = state
        .terminals
        .status(id)
        .map(|s| format!("{:?}", s))
        .unwrap_or_else(|| "unknown".into());
    Ok(Json(TerminalListItem {
        id: id.to_string(),
        name,
        status,
    }))
}

fn terminal_session_id(state: &AppState, terminal_id: Uuid) -> Result<Uuid, ApiError> {
    if let Some(session_id) = state.terminal_sessions.read().get(&terminal_id) {
        return Ok(*session_id);
    }
    let row = state
        .auth
        .db()
        .lock()
        .get_terminal(terminal_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?
        .ok_or_else(|| ApiError::not_found("terminal"))?;
    Ok(row.1)
}

async fn delete_terminal(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    if let Some(session_id) = state.terminal_sessions.read().get(&id) {
        ensure_session_access(&state, user, *session_id, Action::TerminalWrite)?;
    }
    state.terminals.remove(id);
    state.terminal_sessions.write().remove(&id);
    let _ = remove_terminal_record(&state, id);
    Ok(StatusCode::NO_CONTENT)
}

async fn terminal_input(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(id): Path<Uuid>,
    Json(body): Json<TerminalInputRequest>,
) -> Result<StatusCode, ApiError> {
    let session_id = state
        .terminal_sessions
        .read()
        .get(&id)
        .copied()
        .ok_or_else(|| ApiError::not_found("terminal"))?;
    ensure_session_access(&state, user, session_id, Action::TerminalWrite)?;
    crate::terminals::prepare_terminal_connection(&state, id)
        .map_err(|_| ApiError::not_found("terminal"))?;
    state
        .terminals
        .write(id, &body.data)
        .map_err(|_| ApiError::not_found("terminal"))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn terminal_resize(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(id): Path<Uuid>,
    Json(body): Json<ResizeRequest>,
) -> Result<StatusCode, ApiError> {
    let session_id = state
        .terminal_sessions
        .read()
        .get(&id)
        .copied()
        .ok_or_else(|| ApiError::not_found("terminal"))?;
    ensure_session_access(&state, user, session_id, Action::TerminalWrite)?;
    state.terminals.resize(id, body.cols, body.rows)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn terminal_restart(
    State(_state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(Json(serde_json::json!({ "restarting": true, "id": id })))
}

async fn terminal_ws(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(id): Path<Uuid>,
    Query(q): Query<WsQuery>,
) -> Result<Response, ApiError> {
    {
        let stream_session_id = {
            let auth_db = state.auth.db();
            let db = auth_db.lock();
            match db.get_terminal(id)? {
                Some(row) => row.1,
                None => return Err(ApiError::not_found("terminal")),
            }
        };
        ensure_session_access(&state, user, stream_session_id, Action::TerminalRead)?;
        if crate::terminals::prepare_terminal_connection(&state, id).is_err() {
            return Err(ApiError::not_found("terminal"));
        }
    }

    let session_id = state
        .terminal_sessions
        .read()
        .get(&id)
        .copied()
        .ok_or_else(|| ApiError::not_found("terminal"))?;
    let role = get_role(&state, user, session_id)?;
    let can_write = role_can(role, Action::TerminalWrite);
    if !role_can(role, Action::TerminalRead) {
        return Err(ApiError::forbidden("terminal read denied"));
    }
    Ok(ws.on_upgrade(move |socket| {
        ws::handle_terminal_ws(socket, state, id, can_write, q.from_offset)
    }))
}

async fn create_preview(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Json(body): Json<CreatePreviewRequest>,
) -> Result<Json<PreviewResponse>, ApiError> {
    let session_id = Uuid::parse_str(&body.session_id).map_err(|_| ApiError::validation("session_id"))?;
    ensure_session_access(&state, user, session_id, Action::PreviewView)?;
    let id = Uuid::new_v4();
    let public_path = format!("/s/{session_id}/ports/{}/", body.local_port);
    state.previews.write().insert(
        id,
        crate::state::PreviewState {
            id,
            session_id,
            local_port: body.local_port,
            public_path: public_path.clone(),
        },
    );
    Ok(Json(PreviewResponse {
        id: id.to_string(),
        public_path,
    }))
}

async fn list_previews(
    State(state): State<Arc<AppState>>,
    Extension(_user): Extension<Uuid>,
) -> Json<Vec<PreviewResponse>> {
    Json(
        state
            .previews
            .read()
            .values()
            .map(|p| PreviewResponse {
                id: p.id.to_string(),
                public_path: p.public_path.clone(),
            })
            .collect(),
    )
}

async fn delete_preview(
    State(state): State<Arc<AppState>>,
    Extension(_user): Extension<Uuid>,
    Path(id): Path<Uuid>,
) -> StatusCode {
    state.previews.write().remove(&id);
    StatusCode::NO_CONTENT
}

async fn create_browser(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Json(body): Json<CreateBrowserRequest>,
) -> Result<Json<BrowserResponse>, ApiError> {
    let session_id = Uuid::parse_str(&body.session_id).map_err(|_| ApiError::validation("session_id"))?;
    ensure_session_access(&state, user, session_id, Action::BrowserView)?;
    let url = body.target_url.unwrap_or_else(|| "http://127.0.0.1:3000".into());
    let id = state.browsers.create(session_id, &url)?;
    state
        .clone()
        .start_browser_cdp(session_id, id)
        .await
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(BrowserResponse {
        id: id.to_string(),
        stream_path: format!("/api/v1/browser-sessions/{id}/stream"),
        events_path: format!("/api/v1/browser-sessions/{id}/events"),
        webrtc_offer_path: format!("/api/v1/browser-sessions/{id}/webrtc/offer"),
    }))
}

async fn get_browser(
    State(state): State<Arc<AppState>>,
    Extension(_user): Extension<Uuid>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let novnc = state.browsers.get_novnc_port(id);
    let cdp = state.browsers.get_cdp_port(id);
    Ok(Json(serde_json::json!({
        "id": id,
        "novncPort": novnc,
        "cdpPort": cdp,
        "webrtcOfferPath": format!("/api/v1/browser-sessions/{id}/webrtc/offer"),
    })))
}

async fn browser_control(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(id): Path<Uuid>,
    Json(body): Json<BrowserControlRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session_id = browser_session_id(&state, id)?;
    ensure_session_access(&state, user, session_id, Action::BrowserView)?;
    if let Some(url) = body.navigate.as_deref() {
        if url.starts_with("https://claude.com/")
            || url.starts_with("https://claude.ai/")
            || url.contains("/api/v1/claude/oauth/redirect/")
        {
            state.browsers.restart(id, url)?;
            state
                .clone()
                .start_browser_cdp(session_id, id)
                .await
                .map_err(|e| ApiError::validation(&e.to_string()))?;
        } else {
            return Err(ApiError::validation("only Claude OAuth URLs are allowed"));
        }
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn browser_restart(
    State(state): State<Arc<AppState>>,
    Extension(_user): Extension<Uuid>,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateBrowserRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let url = body.target_url.unwrap_or_else(|| "http://127.0.0.1:3000".into());
    state.browsers.restart(id, &url)?;
    Ok(Json(serde_json::json!({ "restarted": true })))
}

async fn browser_reset(
    State(state): State<Arc<AppState>>,
    Extension(_user): Extension<Uuid>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state.browsers.stop(id);
    Ok(Json(serde_json::json!({ "reset": true })))
}

async fn browser_webrtc_offer(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(id): Path<Uuid>,
    Json(offer): Json<SdpPayload>,
) -> Result<Json<SdpPayload>, ApiError> {
    let session_id = browser_session_id(&state, id)?;
    ensure_session_access(&state, user, session_id, Action::BrowserView)?;
    let cdp_port = state
        .browsers
        .get_cdp_port(id)
        .ok_or_else(|| ApiError::not_found("browser session"))?;
    let answer = state
        .webrtc_browser_offer(id, cdp_port, offer)
        .await
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(answer))
}

async fn browser_webrtc_candidate(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(id): Path<Uuid>,
    Json(body): Json<IceCandidatePayload>,
) -> Result<StatusCode, ApiError> {
    let session_id = browser_session_id(&state, id)?;
    ensure_session_access(&state, user, session_id, Action::BrowserView)?;
    state
        .webrtc_browser_candidate(id, body.candidate)
        .await
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn browser_novnc_ws(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let session_id = browser_session_id(&state, id)?;
    ensure_session_access(&state, user, session_id, Action::BrowserView)?;
    let novnc_port = state
        .browsers
        .get_novnc_port(id)
        .ok_or_else(|| ApiError::not_found("browser session"))?;
    Ok(ws.on_upgrade(move |socket| ws::handle_novnc_proxy(socket, novnc_port)))
}

async fn browser_webrtc_stop(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let session_id = browser_session_id(&state, id)?;
    ensure_session_access(&state, user, session_id, Action::BrowserView)?;
    state
        .webrtc_browser_stop(id)
        .await
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

fn browser_session_id(state: &AppState, browser_id: Uuid) -> Result<Uuid, ApiError> {
    state
        .browser_sessions
        .read()
        .get(&browser_id)
        .copied()
        .ok_or_else(|| ApiError::not_found("browser session"))
}

async fn browser_events_ws(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let session_id = state
        .browser_sessions
        .read()
        .get(&id)
        .copied()
        .ok_or_else(|| ApiError::not_found("browser session"))?;
    ensure_session_access(&state, user, session_id, Action::ConsoleView)?;
    Ok(ws.on_upgrade(move |socket| ws::handle_browser_events(socket, state, id)))
}

async fn claude_oauth_redirect(
    State(state): State<Arc<AppState>>,
    Path(token): Path<Uuid>,
) -> Result<Response, ApiError> {
    let url = crate::claude::take_oauth_redirect_url(&state, token)
        .ok_or_else(|| ApiError::not_found("oauth redirect link expired or not ready"))?;
    Ok(Redirect::temporary(&url).into_response())
}

async fn claude_status(
    State(state): State<Arc<AppState>>,
    Extension(_user): Extension<Uuid>,
) -> Json<crate::claude::ClaudeStatus> {
    let install = state.claude_install.lock();
    let auth = state.claude_auth.lock();
    Json(crate::claude::status_snapshot(&install, &auth))
}

async fn claude_install(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    ensure_global_access(&state, user, Action::ClaudeInstall)?;
    if crate::claude::is_installed() {
        let mut install = state.claude_install.lock();
        install.state = "ready".into();
        install.message = "Claude Code is already installed.".into();
        install.error = None;
        return Ok(Json(serde_json::json!({ "started": false, "state": "ready" })));
    }
    crate::claude::spawn_install(state);
    Ok(Json(serde_json::json!({ "started": true })))
}

async fn claude_auth_start(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Json(body): Json<ClaudeAuthStartRequest>,
) -> Result<Json<ClaudeAuthStartResponse>, ApiError> {
    if !crate::claude::is_installed() {
        return Err(ApiError::validation(
            "Claude is not installed — use Install Claude on the home page first",
        ));
    }
    let session_id = body
        .session_id
        .as_deref()
        .map(Uuid::parse_str)
        .transpose()
        .map_err(|_| ApiError::validation("session_id"))?;
    if let Some(sid) = session_id {
        ensure_session_access(&state, user, sid, Action::TerminalWrite)?;
    }
    let (session_id, terminal_id) = crate::claude::start_auth_flow(state.clone(), user, session_id)
        .await
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(ClaudeAuthStartResponse {
        session_id: session_id.to_string(),
        terminal_id: terminal_id.to_string(),
    }))
}

async fn claude_auth_code(
    State(state): State<Arc<AppState>>,
    Extension(_user): Extension<Uuid>,
    Json(body): Json<ClaudeAuthCodeRequest>,
) -> Result<Json<OkResponse>, ApiError> {
    crate::claude::apply_detected_auth_code(&state, &body.code)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(Json(OkResponse { ok: true }))
}

async fn claude_auth_detect_code(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Json(body): Json<ClaudeAuthDetectRequest>,
) -> Result<Json<ClaudeAuthDetectResponse>, ApiError> {
    let browser_id =
        Uuid::parse_str(&body.browser_id).map_err(|_| ApiError::validation("browser_id"))?;
    let session_id = browser_session_id(&state, browser_id)?;
    ensure_session_access(&state, user, session_id, Action::BrowserView)?;
    let cdp_port = state
        .browsers
        .get_cdp_port(browser_id)
        .ok_or_else(|| ApiError::not_found("browser session"))?;
    if let Some(code) = crate::claude::detect_code_from_cdp_port(cdp_port).await {
        let hint = crate::claude::oauth_code_hint(&code);
        crate::claude::apply_detected_auth_code(&state, &code)
            .map_err(|e| ApiError::validation(&e.to_string()))?;
        return Ok(Json(ClaudeAuthDetectResponse {
            found: true,
            submitted: true,
            code_hint: Some(hint),
        }));
    }
    Ok(Json(ClaudeAuthDetectResponse {
        found: false,
        submitted: false,
        code_hint: None,
    }))
}

async fn agent_info(State(state): State<Arc<AppState>>) -> Json<AgentInfoResponse> {
    Json(AgentInfoResponse {
        name: "bunny".into(),
        api_version: bunny_core::API_VERSION.into(),
        protocol_version: bunny_core::PROTOCOL_VERSION.into(),
        require_auth: state.config.security.require_auth,
        auth_modes: vec!["local".into()],
    })
}

async fn get_timeline(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Query(q): Query<TimelineQuery>,
) -> Result<Json<Vec<TimelineItem>>, ApiError> {
    let session_id = Uuid::parse_str(&q.session_id).map_err(|_| ApiError::validation("session_id"))?;
    ensure_session_access(&state, user, session_id, Action::ConsoleView)?;
    let events = state.auth.db().lock().list_timeline(session_id, q.since.unwrap_or(0), q.limit.unwrap_or(100))?;
    Ok(Json(
        events
            .into_iter()
            .map(|(id, source, etype, payload, seq, ts)| TimelineItem {
                id: id.to_string(),
                source,
                event_type: etype,
                payload: serde_json::from_str(&payload).unwrap_or(serde_json::Value::Null),
                sequence: seq,
                ts,
            })
            .collect(),
    ))
}

async fn get_audit_logs(
    State(_state): State<Arc<AppState>>,
    Extension(_user): Extension<Uuid>,
) -> Json<Vec<serde_json::Value>> {
    Json(vec![])
}

async fn voice_intent(
    State(state): State<Arc<AppState>>,
    Extension(_user): Extension<Uuid>,
    Json(body): Json<VoiceIntentRequest>,
) -> Result<Json<VoiceIntentResponse>, ApiError> {
    if !state.config.voice.enabled {
        return Err(ApiError::forbidden("voice disabled"));
    }
    let redacted = state.redactor.read().redact_text(&body.transcript);
    let risk = classify_command_risk(&redacted);
    Ok(Json(VoiceIntentResponse {
        proposal: redacted,
        risk: risk.to_string(),
        requires_confirmation: state.config.voice.require_confirmation || risk != "safe",
        actions: vec!["insert".into(), "run".into(), "cancel".into()],
    }))
}

async fn voice_confirm(
    State(state): State<Arc<AppState>>,
    Extension(_user): Extension<Uuid>,
    Json(body): Json<VoiceConfirmRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if body.action == "run" && state.config.voice.require_confirmation {
        let risk = classify_command_risk(&body.command);
        if risk == "dangerous" && !state.config.voice.allow_direct_run {
            return Err(ApiError::forbidden("dangerous command blocked"));
        }
    }
    if body.action == "insert" || body.action == "run" {
        if let Some(term_id) = body.terminal_id.as_ref() {
            let id = Uuid::parse_str(term_id).map_err(|_| ApiError::validation("terminal_id"))?;
            if body.action == "run" {
                let cmd = format!("{}\n", body.command);
                state.terminals.write(id, &cmd)?;
            } else {
                state.terminals.write(id, &body.command)?;
            }
        }
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

fn ensure_session_access(
    state: &AppState,
    user_id: Uuid,
    session_id: Uuid,
    action: Action,
) -> Result<(), ApiError> {
    let role = get_role(state, user_id, session_id)?;
    if role_can(role, action) {
        Ok(())
    } else {
        Err(ApiError::forbidden("permission denied"))
    }
}

fn ensure_system_owner(state: &AppState, user_id: Uuid) -> Result<(), ApiError> {
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

fn ensure_can_create_sessions(state: &AppState, user_id: Uuid) -> Result<(), ApiError> {
    if ensure_system_owner(state, user_id).is_ok() {
        return Ok(());
    }
    let profile = state
        .auth
        .db()
        .lock()
        .get_user_profile(user_id)
        .map_err(|_| ApiError::forbidden("permission denied"))?;
    if let Some(p) = profile {
        if p.disabled_at.is_none() && p.can_create_sessions {
            return Ok(());
        }
    }
    Err(ApiError::forbidden("permission denied"))
}

fn ensure_global_access(state: &AppState, user_id: Uuid, action: Action) -> Result<(), ApiError> {
    if ensure_system_owner(state, user_id).is_ok() {
        return Ok(());
    }
    if let Ok(profile) = state.auth.db().lock().get_user_profile(user_id) {
        if let Some(p) = profile {
            if p.disabled_at.is_none() {
                match action {
                    Action::ClaudeInstall if p.can_install_claude => return Ok(()),
                    Action::VaultManage if p.can_manage_vault => return Ok(()),
                    _ => {}
                }
            }
        }
    }
    // Global access is granted to users who are Admin in any session.
    let is_admin_anywhere = state
        .auth
        .db()
        .lock()
        .has_any_session_role(user_id, Role::Admin)
        .map_err(|_| ApiError::forbidden("permission denied"))?;
    if is_admin_anywhere && role_can(Role::Admin, action) {
        return Ok(());
    }
    Err(ApiError::forbidden("permission denied"))
}

fn get_role(state: &AppState, user_id: Uuid, session_id: Uuid) -> Result<Role, ApiError> {
    state
        .auth
        .member_role(session_id, user_id)?
        .ok_or_else(|| ApiError::forbidden("not a session member"))
}

fn classify_command_risk(cmd: &str) -> &'static str {
    let lower = cmd.to_lowercase();
    if lower.contains("rm -rf")
        || lower.contains("sudo")
        || lower.contains("chmod -r")
        || lower.contains("curl|sh")
        || lower.contains("drop database")
    {
        "dangerous"
    } else if lower.contains("rm ") || lower.contains("delete") || lower.contains("reset") {
        "medium"
    } else {
        "safe"
    }
}

async fn secrets_status(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
) -> Result<Json<VaultStatusResponse>, ApiError> {
    ensure_secrets_access(&state, user)?;
    Ok(Json(secrets_ops::vault_status(&state)))
}

async fn secrets_init(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Json(body): Json<SecretsInitRequest>,
) -> Result<Json<OkResponse>, ApiError> {
    ensure_secrets_access(&state, user)?;
    init_vault(&state, &body.passphrase, &body.confirm_passphrase)?;
    Ok(Json(OkResponse { ok: true }))
}

async fn secrets_unlock(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Json(body): Json<SecretsPassphraseRequest>,
) -> Result<Json<OkResponse>, ApiError> {
    ensure_secrets_access(&state, user)?;
    let session_id = body
        .session_id
        .as_deref()
        .map(Uuid::parse_str)
        .transpose()
        .map_err(|_| ApiError::validation("invalid session_id"))?;
    unlock_vault(&state, &body.passphrase, session_id)?;
    Ok(Json(OkResponse { ok: true }))
}

async fn secrets_lock(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
) -> Result<Json<OkResponse>, ApiError> {
    ensure_secrets_access(&state, user)?;
    lock_vault(&state);
    Ok(Json(OkResponse { ok: true }))
}

async fn secrets_list(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
) -> Result<Json<Vec<SecretMetaResponse>>, ApiError> {
    ensure_secrets_access(&state, user)?;
    Ok(Json(list_secrets(&state)?))
}

async fn secrets_upsert(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Json(body): Json<SecretUpsertRequest>,
) -> Result<Json<SecretMetaResponse>, ApiError> {
    ensure_secrets_access(&state, user)?;
    Ok(Json(upsert_secret(
        &state,
        &body.name,
        &body.scope,
        body.session_id,
        &body.value,
    )?))
}

async fn secrets_delete(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(name): Path<String>,
    Query(q): Query<SecretScopeQuery>,
) -> Result<StatusCode, ApiError> {
    ensure_secrets_access(&state, user)?;
    remove_secret(&state, &name, &q.scope, q.session_id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn secrets_reveal(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(name): Path<String>,
    Query(q): Query<SecretScopeQuery>,
) -> Result<Json<SecretRevealResponse>, ApiError> {
    ensure_secrets_access(&state, user)?;
    Ok(Json(reveal_secret(&state, &name, &q.scope, q.session_id)?))
}

// --- DTOs ---

#[derive(Serialize)]
pub struct OkResponse { pub ok: bool }
#[derive(Deserialize)]
pub struct SecretsInitRequest { pub passphrase: String, pub confirm_passphrase: String }
#[derive(Deserialize)]
pub struct SecretsPassphraseRequest {
    pub passphrase: String,
    /// When set, reload shells in this workspace session after unlock.
    pub session_id: Option<String>,
}
#[derive(Deserialize)]
pub struct SecretUpsertRequest {
    pub name: String,
    pub scope: String,
    pub session_id: Option<String>,
    pub value: String,
}
#[derive(Deserialize)]
pub struct SecretScopeQuery {
    pub scope: String,
    pub session_id: Option<String>,
}

#[derive(Deserialize)]
pub struct BrowserControlRequest {
    pub navigate: Option<String>,
}

#[derive(Deserialize)]
pub struct ClaudeAuthStartRequest {
    pub session_id: Option<String>,
}

#[derive(Serialize)]
pub struct ClaudeAuthStartResponse {
    pub session_id: String,
    pub terminal_id: String,
}

#[derive(Deserialize)]
pub struct ClaudeAuthCodeRequest {
    pub code: String,
}

#[derive(Deserialize)]
pub struct ClaudeAuthDetectRequest {
    pub browser_id: String,
}

#[derive(Serialize)]
pub struct ClaudeAuthDetectResponse {
    pub found: bool,
    pub submitted: bool,
    pub code_hint: Option<String>,
}

#[derive(Deserialize)]
pub struct BootstrapRequest { pub email: String, pub password: String }
#[derive(Serialize)]
pub struct AgentInfoResponse {
    pub name: String,
    pub api_version: String,
    pub protocol_version: String,
    pub require_auth: bool,
    pub auth_modes: Vec<String>,
}

#[derive(Serialize)]
pub struct BootstrapResponse { pub user_id: String, pub message: String }
#[derive(Deserialize)]
pub struct LoginRequest { pub email: String, pub password: String, pub device_id: Option<String> }
#[derive(Deserialize)]
pub struct MfaVerifyRequest {
    pub code: String,
    pub mfa_challenge_token: Option<String>,
    pub device_id: Option<String>,
}
#[derive(Deserialize)]
pub struct RecentAuthRequest {
    pub password: Option<String>,
}
#[derive(Deserialize)]
pub struct MfaEnableRequest {
    pub code: String,
    pub password: Option<String>,
}
#[derive(Deserialize)]
pub struct MfaDisableRequest {
    pub code: String,
    pub password: Option<String>,
}
#[derive(Serialize)]
pub struct MfaStatusResponse {
    pub enabled: bool,
    pub recovery_remaining: u64,
}
#[derive(Serialize)]
pub struct MfaSetupResponse {
    pub otpauth_uri: String,
    pub secret_base32: String,
}
#[derive(Serialize)]
pub struct MfaEnableResponse {
    pub recovery_codes: Vec<String>,
}
#[derive(Serialize)]
pub struct MeResponse {
    pub user_id: String,
    pub email: String,
    pub created_at: String,
    pub is_owner: bool,
    pub mfa_enabled: bool,
    pub can_install_claude: bool,
    pub can_manage_vault: bool,
    pub can_create_sessions: bool,
    pub default_session_role: String,
}
#[derive(Deserialize)]
pub struct CreateSessionRequest {
    pub project_path: Option<String>,
    pub name: Option<String>,
}

#[derive(Serialize)]
pub struct UserAdminItem {
    pub id: String,
    pub email: String,
    pub disabled: bool,
    pub is_system_owner: bool,
    pub can_install_claude: bool,
    pub can_manage_vault: bool,
    pub can_create_sessions: bool,
    pub default_session_role: String,
}

#[derive(Deserialize)]
pub struct CreateUserRequest {
    pub email: String,
    pub default_session_role: String,
    pub can_install_claude: bool,
    pub can_manage_vault: bool,
    pub can_create_sessions: bool,
}

#[derive(Serialize)]
pub struct CreateUserResponse {
    pub token: String,
}

#[derive(Deserialize)]
pub struct UserAdminUpdateRequest {
    pub user_id: String,
    pub can_install_claude: bool,
    pub can_manage_vault: bool,
    pub can_create_sessions: bool,
    pub default_session_role: String,
}

#[derive(Deserialize)]
pub struct CreateInvitationRequest {
    pub email: String,
    pub role: String,
}

#[derive(Serialize)]
pub struct CreateInvitationResponse {
    pub token: String,
}

#[derive(Deserialize)]
pub struct InvitationAcceptRequest {
    pub token: String,
    pub email: String,
    pub password: String,
    pub device_id: Option<String>,
}

#[derive(Serialize)]
pub struct InvitationAcceptResponse {
    pub user_id: String,
    pub email: String,
    pub session_id: Option<String>,
    pub role: String,
    pub expires_at: String,
}

#[derive(Serialize)]
pub struct SessionMemberResponse {
    pub user_id: String,
    pub email: String,
    pub role: String,
}

#[derive(Deserialize)]
pub struct UpdateSessionMemberRequest {
    pub role: String,
}
#[derive(Serialize)]
pub struct SessionResponse {
    pub id: String,
    pub name: String,
    pub login_url: String,
    pub auth_required: bool,
}
#[derive(Serialize)]
pub struct SessionListItem {
    pub id: String,
    pub name: String,
    pub project_path: String,
    pub status: String,
}
#[derive(Deserialize)]
pub struct RenameRequest { pub name: String }
#[derive(Deserialize)]
pub struct CreateTerminalRequest {
    pub session_id: String,
    pub name: String,
    pub command: Option<String>,
    pub cwd: Option<String>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}
#[derive(Serialize)]
pub struct TerminalResponse { pub id: String, pub name: String, pub ws_url: String }
#[derive(Serialize)]
pub struct TerminalListItem { pub id: String, pub name: String, pub status: String }
#[derive(Deserialize)]
pub struct ListTerminalsQuery { pub session_id: Option<String> }
#[derive(Deserialize)]
pub struct TerminalInputRequest { pub data: String }
#[derive(Deserialize)]
pub struct ResizeRequest { pub cols: u16, pub rows: u16 }
#[derive(Deserialize)]
pub struct WsQuery { pub from_offset: Option<u64> }
#[derive(Deserialize)]
pub struct CreatePreviewRequest { pub session_id: String, pub local_port: u16 }
#[derive(Serialize)]
pub struct PreviewResponse { pub id: String, pub public_path: String }
#[derive(Deserialize)]
pub struct CreateBrowserRequest {
    pub session_id: String,
    pub target_url: Option<String>,
}
#[derive(Serialize)]
pub struct BrowserResponse {
    pub id: String,
    pub stream_path: String,
    pub events_path: String,
    pub webrtc_offer_path: String,
}
#[derive(Deserialize)]
pub struct TimelineQuery {
    pub session_id: String,
    pub since: Option<u64>,
    pub limit: Option<u64>,
}
#[derive(Serialize)]
pub struct TimelineItem {
    pub id: String,
    pub source: String,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub sequence: u64,
    pub ts: String,
}
#[derive(Deserialize)]
pub struct VoiceIntentRequest { pub transcript: String, pub target: Option<String> }
#[derive(Serialize)]
pub struct VoiceIntentResponse {
    pub proposal: String,
    pub risk: String,
    pub requires_confirmation: bool,
    pub actions: Vec<String>,
}
#[derive(Deserialize)]
pub struct VoiceConfirmRequest {
    pub action: String,
    pub command: String,
    pub terminal_id: Option<String>,
}

#[derive(Deserialize)]
pub struct PushRegisterRequest {
    pub device_id: String,
    pub platform: String,
    pub provider: String,
    pub token: String,
}

#[derive(Serialize)]
pub struct PushRegisterResponse {
    pub ok: bool,
    pub fcm_configured: bool,
}

async fn push_register(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Json(body): Json<PushRegisterRequest>,
) -> Result<Json<PushRegisterResponse>, ApiError> {
    state.auth.db().lock().upsert_push_device(
        Uuid::new_v4(),
        user,
        &body.device_id,
        &body.platform,
        &body.provider,
        &body.token,
    )?;
    Ok(Json(PushRegisterResponse {
        ok: true,
        fcm_configured: state.fcm.is_configured(),
    }))
}

async fn push_unregister(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(device_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .auth
        .db()
        .lock()
        .delete_push_device(user, &device_id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn webrtc_config(State(state): State<Arc<AppState>>) -> Json<WebRtcConfigResponse> {
    Json(WebRtcConfigResponse {
        enabled: state.config.webrtc.enabled && state.webrtc_port().is_some(),
        ice_servers: state.webrtc_ice_servers(),
        sidecar_port: state.config.webrtc.sidecar_port,
    })
}

async fn webrtc_offer(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(id): Path<Uuid>,
    Json(offer): Json<SdpPayload>,
) -> Result<Json<SdpPayload>, ApiError> {
    ensure_session_access(&state, user, id, Action::TerminalRead)?;
    let answer = state.webrtc_post_offer(id, offer).await?;
    Ok(Json(answer))
}

async fn webrtc_candidate(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(id): Path<Uuid>,
    Json(body): Json<IceCandidatePayload>,
) -> Result<StatusCode, ApiError> {
    ensure_session_access(&state, user, id, Action::TerminalRead)?;
    state.webrtc_post_candidate(id, body.candidate).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub struct ApiError {
    status: StatusCode,
    code: String,
    message: String,
}

impl ApiError {
    pub(crate) fn unauthorized() -> Self { Self { status: StatusCode::UNAUTHORIZED, code: "UNAUTHORIZED".into(), message: "authentication required".into() } }
    pub(crate) fn unauthorized_msg(msg: &str) -> Self { Self { status: StatusCode::UNAUTHORIZED, code: "UNAUTHORIZED".into(), message: msg.into() } }
    pub(crate) fn too_many_requests(msg: &str) -> Self { Self { status: StatusCode::TOO_MANY_REQUESTS, code: "TOO_MANY_REQUESTS".into(), message: msg.into() } }
    pub(crate) fn forbidden(msg: &str) -> Self { Self { status: StatusCode::FORBIDDEN, code: "FORBIDDEN".into(), message: msg.into() } }
    pub(crate) fn not_found(r: &str) -> Self { Self { status: StatusCode::NOT_FOUND, code: "NOT_FOUND".into(), message: format!("{r} not found") } }
    pub(crate) fn conflict(msg: &str) -> Self { Self { status: StatusCode::CONFLICT, code: "CONFLICT".into(), message: msg.into() } }
    pub(crate) fn validation(msg: &str) -> Self { Self { status: StatusCode::BAD_REQUEST, code: "VALIDATION_ERROR".into(), message: msg.into() } }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError::validation(&e.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ApiErrorResponse {
            error: bunny_core::types::ApiErrorBody {
                code: self.code,
                message: self.message,
                details: None,
            },
        };
        (self.status, Json(body)).into_response()
    }
}
