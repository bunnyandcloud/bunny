use crate::api::ApiError;
use crate::browser_ops;
use crate::discord_ops::BridgeContext;
use crate::novnc_proxy;
use crate::state::AppState;
use crate::ws;
use axum::extract::{Path, State, WebSocketUpgrade};
use axum::response::Response;
use axum::Json;
use bunny_discord::WatchSession;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Serialize)]
pub struct WatchLinkResponse {
    pub watch_url: String,
    pub token: String,
    pub expires_at: String,
    pub layout: String,
    pub visibility: String,
    pub mode: String,
}

pub async fn create_watch_link(
    state: &AppState,
    ctx: &BridgeContext,
    session_id: Uuid,
    browser_id: Uuid,
    layout: Option<String>,
    visibility: Option<String>,
    ttl_hours: Option<u64>,
    interactive: bool,
) -> Result<Json<WatchLinkResponse>, ApiError> {
    let token = Uuid::new_v4().simple().to_string();
    let ttl = ttl_hours.unwrap_or(1).max(1);
    let mode = if interactive {
        "interactive"
    } else {
        "read_only"
    };
    let watch = WatchSession {
        id: Uuid::new_v4(),
        token: token.clone(),
        session_id,
        guild_id: ctx.guild_id.clone(),
        channel_id: ctx.channel_id.clone(),
        thread_id: ctx.thread_id.clone(),
        layout: layout.unwrap_or_else(|| "full".into()),
        visibility: visibility.unwrap_or_else(|| "channel".into()),
        mode: mode.into(),
        status: "active".into(),
        required_role_ids: vec![],
        browser_id: Some(browser_id),
        expires_at: Utc::now() + Duration::hours(ttl as i64),
        created_at: Utc::now(),
    };
    state
        .discord
        .lock()
        .create_watch(&watch)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    let base = state.config.resolve_public_url();
    let watch_url = format!("{base}/watch/{token}");
    Ok(Json(WatchLinkResponse {
        watch_url,
        token,
        expires_at: watch.expires_at.to_rfc3339(),
        layout: watch.layout,
        visibility: watch.visibility,
        mode: watch.mode,
    }))
}

pub fn parse_watch_token_from_url(input: &str) -> Result<String, ApiError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(ApiError::validation("watch URL is empty"));
    }
    if let Some(token) = extract_watch_token_from_path(input) {
        return Ok(token);
    }
    if let Some(idx) = input.find("/watch/") {
        let rest = &input[idx + "/watch/".len()..];
        if let Some(token) = token_from_watch_suffix(rest) {
            return Ok(token);
        }
    }
    Err(ApiError::validation(
        "invalid watch URL: expected .../watch/<token>",
    ))
}

fn extract_watch_token_from_path(path: &str) -> Option<String> {
    let path = path.split(['?', '#']).next()?;
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let pos = segments.iter().position(|s| *s == "watch")?;
    token_from_watch_suffix(segments.get(pos + 1)?.trim())
}

fn token_from_watch_suffix(suffix: &str) -> Option<String> {
    let token = suffix.split('/').next()?.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

pub fn stop_browser_streams(
    state: &AppState,
    guild_id: &str,
    channel_id: &str,
    watch_url: Option<&str>,
) -> Result<u32, ApiError> {
    let tokens = {
        let db = state.discord.lock();
        if let Some(url) = watch_url {
            let token = parse_watch_token_from_url(url)?;
            let watch = db
                .get_watch_by_token(&token)
                .map_err(|e| ApiError::validation(&e.to_string()))?
                .ok_or_else(|| ApiError::not_found("watch session"))?;
            if watch.guild_id != guild_id || watch.channel_id != channel_id {
                return Err(ApiError::forbidden(
                    "watch URL does not belong to this channel",
                ));
            }
            db.stop_watch(&token)
                .map_err(|e| ApiError::validation(&e.to_string()))?;
            vec![token]
        } else {
            let tokens = db
                .active_watch_tokens_for_channel(guild_id, channel_id)
                .map_err(|e| ApiError::validation(&e.to_string()))?;
            db.stop_all_watches_for_channel(guild_id, channel_id)
                .map_err(|e| ApiError::validation(&e.to_string()))?;
            tokens
        }
    };
    state.watch_hub.revoke_many(&tokens);
    Ok(tokens.len() as u32)
}

pub async fn get_watch_meta(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let watch = state
        .discord
        .lock()
        .get_watch_by_token(&token)
        .map_err(|e| ApiError::validation(&e.to_string()))?
        .ok_or_else(|| ApiError::not_found("watch session"))?;
    if watch.expires_at < Utc::now() {
        return Err(ApiError::not_found("watch expired"));
    }
    let browser_id =
        browser_ops::resolve_session_browser_id(&state, watch.session_id, None)?;
    Ok(Json(serde_json::json!({
        "session_id": watch.session_id.to_string(),
        "layout": watch.layout,
        "mode": watch.mode,
        "visibility": watch.visibility,
        "browser_id": browser_id.to_string(),
        "browser_ids": [browser_id.to_string()],
        "expires_at": watch.expires_at.to_rfc3339(),
    })))
}

#[derive(Deserialize)]
pub struct WatchAccessRequest {
    pub discord_user_id: Option<String>,
    pub bunny_session_token: Option<String>,
}

pub async fn grant_watch_access(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
    Json(body): Json<WatchAccessRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let watch = state
        .discord
        .lock()
        .get_watch_by_token(&token)
        .map_err(|e| ApiError::validation(&e.to_string()))?
        .ok_or_else(|| ApiError::not_found("watch session"))?;
    if watch.expires_at < Utc::now() {
        return Err(ApiError::not_found("watch expired"));
    }

    if let Some(discord_id) = &body.discord_user_id {
        if state
            .discord
            .lock()
            .get_bunny_user_for_discord(discord_id)
            .map_err(|e| ApiError::validation(&e.to_string()))?
            .is_none()
        {
            return Err(ApiError::forbidden("link discord account first"));
        }
    } else if let Some(cookie_token) = &body.bunny_session_token {
        if state.auth.authenticate_session(cookie_token).is_err() {
            return Err(ApiError::forbidden("invalid session"));
        }
    } else if watch.visibility == "guild" || watch.visibility == "channel" {
        // Watch link itself is the capability for channel-scoped read-only streams.
    } else {
        return Err(ApiError::forbidden("authentication required"));
    }

    let access_token = Uuid::new_v4().to_string();
    Ok(Json(serde_json::json!({
        "access_token": access_token,
        "session_id": watch.session_id.to_string(),
        "expires_in_secs": (watch.expires_at - Utc::now()).num_seconds().max(0),
    })))
}

fn resolve_active_watch(state: &AppState, token: &str) -> Result<WatchSession, ApiError> {
    let watch = state
        .discord
        .lock()
        .get_watch_by_token(token)
        .map_err(|e| ApiError::validation(&e.to_string()))?
        .ok_or_else(|| ApiError::not_found("watch session"))?;
    if watch.expires_at < Utc::now() {
        return Err(ApiError::not_found("watch expired"));
    }
    Ok(watch)
}

fn watch_browser_id(state: &AppState, watch: &WatchSession) -> Result<Uuid, ApiError> {
    browser_ops::resolve_session_browser_id(state, watch.session_id, None)
}

pub async fn watch_novnc_http_root(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
) -> Result<Response, ApiError> {
    watch_novnc_http(State(state), Path((token, String::new()))).await
}

pub async fn watch_novnc_http(
    State(state): State<Arc<AppState>>,
    Path((token, path)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let watch = resolve_active_watch(&state, &token)?;
    let path = path.trim_start_matches('/');
    let rel = if path.is_empty() { "vnc.html" } else { path };
    if rel == "vnc.html" {
        let lock = if watch.mode == "interactive" {
            novnc_proxy::NovncEmbedLock::Interactive
        } else {
            novnc_proxy::NovncEmbedLock::ReadOnly
        };
        return novnc_proxy::serve_locked_novnc_html(lock)
            .await
            .map_err(|_| ApiError::not_found("noVNC asset"));
    }
    if let Some(file_path) = novnc_proxy::resolve_novnc_file(path) {
        return novnc_proxy::serve_novnc_file(file_path)
            .await
            .map_err(|_| ApiError::not_found("noVNC asset"));
    }
    Err(ApiError::not_found("noVNC asset"))
}

pub async fn watch_novnc_ws(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
) -> Result<Response, ApiError> {
    let watch = resolve_active_watch(&state, &token)?;
    let browser_id = watch_browser_id(&state, &watch)?;
    let novnc_port = state
        .browsers
        .get_novnc_port(browser_id)
        .ok_or_else(|| ApiError::not_found("browser session"))?;
    let revoked_rx = state.watch_hub.subscribe(&token);
    Ok(ws.on_upgrade(move |socket| {
        let revoked = crate::watch_hub::WatchHub::revoked_flag(revoked_rx);
        ws::handle_novnc_proxy(socket, novnc_port, Some(revoked))
    }))
}

#[cfg(test)]
mod tests {
    use super::parse_watch_token_from_url;

    #[test]
    fn parse_watch_token_from_full_url() {
        let token = parse_watch_token_from_url("http://127.0.0.1:7681/watch/abc123def456");
        assert!(token.is_ok());
        assert_eq!(token.ok(), Some("abc123def456".to_string()));
    }

    #[test]
    fn parse_watch_token_from_path() {
        let token = parse_watch_token_from_url("/watch/abc123def456");
        assert!(token.is_ok());
        assert_eq!(token.ok(), Some("abc123def456".to_string()));
    }

    #[test]
    fn parse_watch_token_strips_query() {
        let token = parse_watch_token_from_url("https://host/watch/abc123?autoconnect=1");
        assert!(token.is_ok());
        assert_eq!(token.ok(), Some("abc123".to_string()));
    }
}
