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
    routing::{delete, get, patch, post},
    Json, Router,
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
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
        .route("/agent/info", get(agent_info));

    let protected = Router::new()
        .route("/auth/logout", post(auth_logout))
        .route("/auth/me", get(auth_me))
        .route("/sessions", post(create_session).get(list_sessions))
        .route(
            "/sessions/:id",
            get(get_session).patch(patch_session).delete(delete_session),
        )
        .route("/sessions/:id/join", post(join_session))
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
        Router::new()
            .nest_service("/assets", ServeDir::new(dist.join("assets")))
            .fallback_service(ServeFile::new(dist.join("index.html")))
    } else {
        Router::new().route(
            "/",
            get(|| async {
                "bunny API running. Run: bunny run --web-ui  (or: cd apps/web && npm run build)"
            }),
        )
    };

    Router::new()
        .nest(&format!("/api/{API_VERSION}"), api)
        .merge(preview_routes)
        .merge(static_files)
        .route("/login", get(|| async { Redirect::temporary("/") }))
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

async fn auth_login(
    State(state): State<Arc<AppState>>,
    Json(body): Json<LoginRequest>,
) -> Result<(CookieJar, Json<LoginResponse>), ApiError> {
    let result = state.auth.login(&body.email, &body.password, body.device_id.as_deref())?;
    let mut cookie = Cookie::new("bunny_session", result.session_token.clone());
    cookie.set_http_only(true);
    cookie.set_path("/");
    cookie.set_same_site(SameSite::Lax);
    let jar = CookieJar::new().add(cookie);
    Ok((
        jar,
        Json(LoginResponse {
            user_id: result.user_id.to_string(),
            email: result.email,
            expires_at: result.expires_at.to_rfc3339(),
        }),
    ))
}

async fn auth_logout(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> Result<CookieJar, ApiError> {
    if let Some(c) = jar.get("bunny_session") {
        state.auth.logout(c.value())?;
    }
    Ok(jar.remove(Cookie::from("bunny_session")))
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
    Ok(Json(MeResponse {
        user_id: user.to_string(),
        email,
        created_at: created_at.to_rfc3339(),
        is_owner,
    }))
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
    ensure_session_access(&state, user, id, Action::TerminalRead)?;
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
    ensure_session_access(&state, user, id, Action::TerminalWrite)?;
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
    ensure_session_access(&state, user, id, Action::TerminalRead)?;
    Ok(Json(serde_json::json!({ "joined": true, "sessionId": id })))
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
    ensure_session_access(&state, user, id, Action::SessionStop)?;
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
    ensure_session_access(&state, user, id, Action::TerminalRead)?;
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
    State(_state): State<Arc<AppState>>,
    Extension(_user): Extension<Uuid>,
    Path(id): Path<Uuid>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let _ = (id, body);
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
#[derive(Serialize)]
pub struct LoginResponse { pub user_id: String, pub email: String, pub expires_at: String }
#[derive(Serialize)]
pub struct MeResponse { pub user_id: String, pub email: String, pub created_at: String, pub is_owner: bool }
#[derive(Deserialize)]
pub struct CreateSessionRequest {
    pub project_path: Option<String>,
    pub name: Option<String>,
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
