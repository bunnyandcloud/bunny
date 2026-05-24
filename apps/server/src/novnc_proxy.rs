use crate::state::AppState;
use axum::{
    body::Body,
    extract::{Extension, Path, State},
    http::{header, Request, StatusCode},
    response::Response,
};
use bunny_core::permissions::{role_can, Action};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use uuid::Uuid;

pub async fn http_proxy(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path((browser_id, path)): Path<(Uuid, String)>,
    req: Request<Body>,
) -> Result<Response, StatusCode> {
    proxy_request(&state, user, browser_id, &path, req).await
}

pub async fn http_proxy_root(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(browser_id): Path<Uuid>,
    req: Request<Body>,
) -> Result<Response, StatusCode> {
    proxy_request(&state, user, browser_id, "", req).await
}

async fn proxy_request(
    state: &AppState,
    user: Uuid,
    browser_id: Uuid,
    path: &str,
    req: Request<Body>,
) -> Result<Response, StatusCode> {
    let session_id = state
        .browser_sessions
        .read()
        .get(&browser_id)
        .copied()
        .ok_or(StatusCode::NOT_FOUND)?;
    if !role_can(
        session_role(state, user, session_id),
        Action::BrowserView,
    ) {
        return Err(StatusCode::FORBIDDEN);
    }

    let _ = state
        .browsers
        .get_novnc_port(browser_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let path = path.trim_start_matches('/');
    if let Some(file_path) = resolve_novnc_file(path) {
        return serve_novnc_file(file_path).await;
    }

    // websockify only speaks WebSocket; static UI is served from disk above.
    let _ = req;
    Err(StatusCode::NOT_FOUND)
}

fn resolve_novnc_file(path: &str) -> Option<PathBuf> {
    let web = novnc_web_dir()?;
    let rel = if path.is_empty() || path == "vnc.html" {
        "vnc.html"
    } else {
        path
    };
    let file = web.join(rel);
    if !file.is_file() {
        return None;
    }
    let canonical = file.canonicalize().ok()?;
    let web_canonical = web.canonicalize().ok()?;
    if !canonical.starts_with(&web_canonical) {
        return None;
    }
    Some(canonical)
}

fn novnc_web_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("BUNNY_NOVNC_WEB") {
        let p = PathBuf::from(dir);
        if p.join("vnc.html").is_file() {
            return Some(p);
        }
    }
    for candidate in ["/usr/share/novnc", "/usr/local/share/novnc"] {
        let p = PathBuf::from(candidate);
        if p.join("vnc.html").is_file() {
            return Some(p);
        }
    }
    None
}

async fn serve_novnc_file(path: PathBuf) -> Result<Response, StatusCode> {
    let bytes = fs::read(&path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let content_type = match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "application/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("woff2") => "font/woff2",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(bytes))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn session_role(state: &AppState, user_id: Uuid, session_id: Uuid) -> bunny_core::types::Role {
    if user_id.is_nil() {
        return bunny_core::types::Role::Owner;
    }
    state
        .auth
        .member_role(session_id, user_id)
        .ok()
        .flatten()
        .unwrap_or(bunny_core::types::Role::Viewer)
}
