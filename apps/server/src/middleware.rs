use crate::state::AppState;
use axum::{
    body::Body,
    extract::State,
    http::{header, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use uuid::Uuid;

pub async fn require_auth(
    State(state): State<Arc<AppState>>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    if !state.config.security.require_auth {
        req.extensions_mut().insert(Uuid::nil());
        return next.run(req).await;
    }

    let token = extract_token(req.headers());
    let Some(token) = token else {
        return (StatusCode::UNAUTHORIZED, "authentication required").into_response();
    };

    match state.auth.authenticate_session(&token) {
        Ok(session) => {
            req.extensions_mut().insert(session.user_id);
            req.extensions_mut().insert(session);
            next.run(req).await
        }
        Err(_) => (StatusCode::UNAUTHORIZED, "invalid session").into_response(),
    }
}

fn extract_token(headers: &axum::http::HeaderMap) -> Option<String> {
    if let Some(auth) = headers.get(header::AUTHORIZATION) {
        let s = auth.to_str().ok()?;
        if let Some(t) = s.strip_prefix("Bearer ") {
            return Some(t.to_string());
        }
    }
    if let Some(cookie) = headers.get(header::COOKIE) {
        let s = cookie.to_str().ok()?;
        for part in s.split(';') {
            let part = part.trim();
            if let Some(v) = part.strip_prefix("bunny_session=") {
                return Some(v.to_string());
            }
        }
    }
    None
}

pub async fn require_bridge(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if crate::discord_ops::verify_bridge_token(&state, req.headers()).is_ok() {
        return next.run(req).await;
    }
    (StatusCode::UNAUTHORIZED, "invalid bridge token").into_response()
}
