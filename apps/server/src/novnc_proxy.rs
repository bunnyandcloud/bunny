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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NovncEmbedLock {
    /// Force interactive mode (clears sticky noVNC view_only storage).
    Interactive,
    /// Force read-only; hide settings so view_only cannot be toggled.
    ReadOnly,
}

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
    let rel = if path.is_empty() { "vnc.html" } else { path };
    if rel == "vnc.html" {
        if let Some(lock) = parse_bunny_lock(req.uri().query()) {
            return serve_locked_novnc_html(lock).await;
        }
    }
    if let Some(file_path) = resolve_novnc_file(path) {
        return serve_novnc_file(file_path).await;
    }

    // websockify only speaks WebSocket; static UI is served from disk above.
    let _ = req;
    Err(StatusCode::NOT_FOUND)
}

fn parse_bunny_lock(query: Option<&str>) -> Option<NovncEmbedLock> {
    let query = query?;
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=')?;
        if key == "bunny_lock" {
            return match value {
                "readonly" => Some(NovncEmbedLock::ReadOnly),
                "interactive" => Some(NovncEmbedLock::Interactive),
                _ => None,
            };
        }
    }
    None
}

fn novnc_html_inject(lock: NovncEmbedLock) -> &'static str {
    match lock {
        NovncEmbedLock::Interactive => r#"
<script>
try { localStorage.setItem('view_only', 'false'); } catch (e) {}
(function () {
  function unlock() {
    var cb = document.getElementById('noVNC_setting_view_only');
    if (!cb) { requestAnimationFrame(unlock); return; }
    cb.checked = false;
    cb.disabled = false;
  }
  unlock();
})();
</script>
"#,
        NovncEmbedLock::ReadOnly => r#"
<style>#noVNC_settings_button,#noVNC_settings{display:none!important}</style>
<script>
try { localStorage.setItem('view_only', 'true'); } catch (e) {}
(function () {
  function lock() {
    var cb = document.getElementById('noVNC_setting_view_only');
    if (!cb) { requestAnimationFrame(lock); return; }
    cb.checked = true;
    cb.disabled = true;
    cb.addEventListener('change', function () { cb.checked = true; });
  }
  lock();
})();
</script>
"#,
    }
}

pub async fn serve_locked_novnc_html(lock: NovncEmbedLock) -> Result<Response, StatusCode> {
    let file_path = resolve_novnc_file("vnc.html").ok_or(StatusCode::NOT_FOUND)?;
    let mut html = fs::read_to_string(&file_path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let inject = novnc_html_inject(lock);
    if let Some(pos) = html.rfind("</body>") {
        html.insert_str(pos, inject);
    } else {
        html.push_str(inject);
    }
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(html))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub(crate) fn resolve_novnc_file(path: &str) -> Option<PathBuf> {
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

pub(crate) async fn serve_novnc_file(path: PathBuf) -> Result<Response, StatusCode> {
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
