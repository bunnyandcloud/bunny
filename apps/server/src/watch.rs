use crate::api::ApiError;
use crate::discord_ops::BridgeContext;
use crate::state::AppState;
use axum::extract::{Path, State};
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
}

pub async fn create_watch_link(
    state: &AppState,
    ctx: &BridgeContext,
    session_id: Uuid,
    layout: Option<String>,
    visibility: Option<String>,
    ttl_hours: Option<u64>,
) -> Result<Json<WatchLinkResponse>, ApiError> {
    let token = Uuid::new_v4().simple().to_string();
    let ttl = ttl_hours.unwrap_or(1).max(1);
    let watch = WatchSession {
        id: Uuid::new_v4(),
        token: token.clone(),
        session_id,
        guild_id: ctx.guild_id.clone(),
        channel_id: ctx.channel_id.clone(),
        thread_id: ctx.thread_id.clone(),
        layout: layout.unwrap_or_else(|| "full".into()),
        visibility: visibility.unwrap_or_else(|| "channel".into()),
        mode: "read_only".into(),
        status: "active".into(),
        required_role_ids: vec![],
        expires_at: Utc::now() + Duration::hours(ttl as i64),
        created_at: Utc::now(),
    };
    state
        .discord
        .lock()
        .create_watch(&watch)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    let base = state
        .config
        .discord
        .public_url
        .clone()
        .unwrap_or_else(|| format!("http://{}:{}", state.config.server.bind_host, state.config.server.port));
    let watch_url = format!("{base}/watch/{token}");
    Ok(Json(WatchLinkResponse {
        watch_url,
        token,
        expires_at: watch.expires_at.to_rfc3339(),
        layout: watch.layout,
        visibility: watch.visibility,
    }))
}

pub fn stop_watch_for_channel(state: &AppState, guild_id: &str, channel_id: &str) -> Result<bool, ApiError> {
    if let Some(w) = state
        .discord
        .lock()
        .active_watch_for_channel(guild_id, channel_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?
    {
        return state
            .discord
            .lock()
            .stop_watch(&w.token)
            .map_err(|e| ApiError::validation(&e.to_string()));
    }
    Ok(false)
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
    let browsers: Vec<String> = state
        .browser_sessions
        .read()
        .iter()
        .filter(|(_, sid)| **sid == watch.session_id)
        .map(|(id, _)| id.to_string())
        .collect();
    Ok(Json(serde_json::json!({
        "session_id": watch.session_id.to_string(),
        "layout": watch.layout,
        "mode": watch.mode,
        "visibility": watch.visibility,
        "browser_ids": browsers,
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
