use crate::state::AppState;
use crate::ws;
use axum::{
    body::Body,
    extract::{Extension, FromRequest, Path, Request, State, WebSocketUpgrade},
    http::{header, HeaderMap, Request as HttpRequest, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::any,
    Router,
};
use bunny_core::permissions::{role_can, Action};
use bunny_core::types::Role;
use std::sync::Arc;
use uuid::Uuid;

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        // Trailing slash is required by the web UI; Axum's `/*path` does not match it alone.
        .route("/s/:session_id/ports/:port/", any(proxy_root))
        .route("/s/:session_id/ports/:port", any(proxy_root))
        .route("/s/:session_id/ports/:port/*path", any(proxy_handler))
        .with_state(state)
}

/// Catches `/_next/*` requested at the Bunny origin (Next/Vite dev assets) and proxies
/// using the port encoded in the Referer preview URL.
pub fn root_dev_assets_router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/_next/*path", any(root_dev_asset_proxy))
        .route("/@vite/*path", any(root_vite_asset_proxy))
        .route("/@fs/*path", any(root_vite_fs_proxy))
        .with_state(state)
}

/// `public/` assets (e.g. `/next.svg`) requested at the Bunny origin without the preview prefix.
pub fn root_public_assets_router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/:file", any(root_public_file_proxy))
        .with_state(state)
}

async fn proxy_root(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path((session_id, port)): Path<(Uuid, u16)>,
    headers: HeaderMap,
    req: HttpRequest<Body>,
) -> Result<Response, StatusCode> {
    proxy_handler(
        State(state),
        Extension(user),
        Path((session_id, port, String::new())),
        headers,
        req,
    )
    .await
}

async fn root_public_file_proxy(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(file): Path<String>,
    headers: HeaderMap,
    req: HttpRequest<Body>,
) -> Result<Response, StatusCode> {
    if !is_public_root_file(&file) {
        return Err(StatusCode::NOT_FOUND);
    }
    root_asset_proxy(state, user, headers, req, &file).await
}

fn is_public_root_file(file: &str) -> bool {
    if file.is_empty() || file.contains('/') || file.starts_with('.') {
        return false;
    }
    let Some((_, ext)) = file.rsplit_once('.') else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "svg" | "ico" | "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "woff2" | "woff" | "ttf"
            | "css" | "js" | "json" | "txt" | "map"
    )
}

async fn root_dev_asset_proxy(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(path): Path<String>,
    headers: HeaderMap,
    req: HttpRequest<Body>,
) -> Result<Response, StatusCode> {
    root_asset_proxy(state, user, headers, req, &format!("_next/{path}")).await
}

async fn root_vite_asset_proxy(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(path): Path<String>,
    headers: HeaderMap,
    req: HttpRequest<Body>,
) -> Result<Response, StatusCode> {
    root_asset_proxy(state, user, headers, req, &format!("@vite/{path}")).await
}

async fn root_vite_fs_proxy(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path(path): Path<String>,
    headers: HeaderMap,
    req: HttpRequest<Body>,
) -> Result<Response, StatusCode> {
    root_asset_proxy(state, user, headers, req, &format!("@fs/{path}")).await
}

async fn root_asset_proxy(
    state: Arc<AppState>,
    user: Uuid,
    headers: HeaderMap,
    req: HttpRequest<Body>,
    asset_path: &str,
) -> Result<Response, StatusCode> {
    let referer = headers
        .get(header::REFERER)
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let (session_id, port) = parse_preview_referer(referer).ok_or(StatusCode::BAD_REQUEST)?;
    if !role_can(
        get_role(&state, user, session_id).unwrap_or(Role::Viewer),
        Action::PreviewView,
    ) {
        return Err(StatusCode::FORBIDDEN);
    }
    let prefix = preview_prefix(session_id, port);
    let target = format!("http://127.0.0.1:{port}/{asset_path}");
    let public_host = public_host(&headers);
    proxy_http(req, &target, &prefix, port, &public_host, false).await
}

async fn proxy_handler(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path((session_id, port, path)): Path<(Uuid, u16, String)>,
    headers: HeaderMap,
    req: HttpRequest<Body>,
) -> Result<Response, StatusCode> {
    let role = get_role(&state, user, session_id).unwrap_or(Role::Viewer);
    if !role_can(role, Action::PreviewView) {
        return Err(StatusCode::FORBIDDEN);
    }

    let prefix = preview_prefix(session_id, port);
    let path = path.trim_start_matches('/');
    let target = if path.is_empty() {
        format!("http://127.0.0.1:{port}/")
    } else {
        format!("http://127.0.0.1:{port}/{path}")
    };
    let public_host = public_host(&headers);

    if is_websocket_upgrade(&headers) {
        let upgrade = WebSocketUpgrade::from_request(req, &())
            .await
            .map_err(|_| StatusCode::BAD_REQUEST)?;
        let ws_upstream = if path.is_empty() {
            format!("ws://127.0.0.1:{port}/")
        } else {
            format!("ws://127.0.0.1:{port}/{path}")
        };
        return Ok(upgrade
            .on_upgrade(move |socket| ws::proxy_websocket(socket, ws_upstream))
            .into_response());
    }

    proxy_http(req, &target, &prefix, port, &public_host, true).await
}

async fn proxy_http(
    req: HttpRequest<Body>,
    target: &str,
    prefix: &str,
    port: u16,
    public_host: &str,
    rewrite_body: bool,
) -> Result<Response, StatusCode> {
    let method = req.method().clone();
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut builder = client.request(method, target);
    builder = builder.header("Host", format!("127.0.0.1:{port}"));
    builder = builder.header("Origin", format!("http://127.0.0.1:{port}"));

    for (k, v) in req.headers().iter() {
        let kl = k.as_str().to_lowercase();
        if !matches!(
            kl.as_str(),
            "host" | "origin" | "cookie" | "authorization" | "connection" | "upgrade"
        ) {
            if let Ok(s) = v.to_str() {
                builder = builder.header(k.as_str(), s);
            }
        }
    }

    let body_bytes = axum::body::to_bytes(req.into_body(), 10 * 1024 * 1024)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    if !body_bytes.is_empty() {
        builder = builder.body(body_bytes);
    }

    let resp = builder
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let mut response = Response::builder().status(status);
    for (k, v) in resp.headers().iter() {
        let kl = k.as_str().to_lowercase();
        if matches!(kl.as_str(), "set-cookie" | "transfer-encoding" | "content-length") {
            continue;
        }
        if kl == "location" {
            if let Ok(loc) = v.to_str() {
                if let Some(rewritten) = rewrite_location(loc, prefix) {
                    response = response.header(k.as_str(), rewritten);
                }
                continue;
            }
        }
        if let Ok(s) = v.to_str() {
            response = response.header(k.as_str(), s);
        }
    }

    let mut bytes = resp
        .bytes()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?
        .to_vec();

    if rewrite_body && should_rewrite_body(&content_type) {
        if let Ok(text) = std::str::from_utf8(&bytes) {
            bytes = rewrite_asset_urls(text, prefix, port, public_host).into_bytes();
        }
    }

    response
        .body(Body::from(bytes))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn preview_prefix(session_id: Uuid, port: u16) -> String {
    format!("/s/{session_id}/ports/{port}/")
}

fn public_host(headers: &HeaderMap) -> String {
    headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("127.0.0.1:7681")
        .to_string()
}

fn is_websocket_upgrade(headers: &HeaderMap) -> bool {
    headers
        .get(header::UPGRADE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
}

fn parse_preview_referer(referer: &str) -> Option<(Uuid, u16)> {
    let ports_idx = referer.find("/ports/")?;
    let after = &referer[ports_idx + "/ports/".len()..];
    let slash = after.find('/')?;
    let port: u16 = after[..slash].parse().ok()?;
    let before_ports = &referer[..ports_idx];
    let s_idx = before_ports.rfind("/s/")?;
    let session_str = before_ports[s_idx + "/s/".len()..]
        .trim_end_matches('/')
        .split('?')
        .next()
        .unwrap_or("");
    let session_id = Uuid::parse_str(session_str).ok()?;
    Some((session_id, port))
}

fn should_rewrite_body(content_type: &str) -> bool {
    let ct = content_type.to_ascii_lowercase();
    ct.contains("text/html")
        || ct.contains("javascript")
        || ct.contains("text/css")
        || ct.contains("application/json")
        || ct.contains("text/javascript")
}

/// Rewrite root-absolute URLs so assets load through the preview prefix (not Bunny's SPA).
fn rewrite_asset_urls(body: &str, prefix: &str, port: u16, public_host: &str) -> String {
    let mut out = body.to_string();
    for attr in ["href", "src", "srcSet", "action"] {
        for quote in ["\"", "'"] {
            let from = format!("{attr}={quote}/");
            let to = format!("{attr}={quote}{prefix}");
            out = out.replace(&from, &to);
        }
    }
    for from in ["url(/", "url('/", "url(\"/"] {
        let to = match from {
            "url(/" => format!("url({prefix}"),
            "url('/" => format!("url('{prefix}"),
            _ => format!("url(\"{prefix}"),
        };
        out = out.replace(from, &to);
    }

    let ws_base = format!("ws://{public_host}{}", prefix.trim_end_matches('/'));
    for host in ["127.0.0.1", "localhost", "0.0.0.0"] {
        let http_from = format!("http://{host}:{port}");
        let http_to = format!("http://{public_host}{}", prefix.trim_end_matches('/'));
        out = out.replace(&http_from, &http_to);
        let ws_from = format!("ws://{host}:{port}");
        out = out.replace(&ws_from, &ws_base);
        let wss_from = format!("wss://{host}:{port}");
        let wss_to = ws_base.replace("ws://", "wss://");
        out = out.replace(&wss_from, &wss_to);
    }

    // Next/Vite dev assets referenced as string paths inside JS bundles.
    let next_via_prefix = format!("{prefix}_next/");
    out = out.replace("/_next/", &next_via_prefix);
    out = out.replace("/@vite/", &format!("{prefix}@vite/"));
    out = out.replace("/@fs/", &format!("{prefix}@fs/"));

    if out.contains("<head") && !out.contains("<base ") {
        if let Some(pos) = out.find("<head>") {
            let insert_at = pos + "<head>".len();
            let base = format!(r#"<base href="{prefix}">"#);
            out.insert_str(insert_at, &base);
        } else if let Some(pos) = out.find("<head ") {
            if let Some(close) = out[pos..].find('>') {
                let insert_at = pos + close + 1;
                let base = format!(r#"<base href="{prefix}">"#);
                out.insert_str(insert_at, &base);
            }
        }
    }
    out
}

fn rewrite_location(loc: &str, prefix: &str) -> Option<String> {
    if loc.starts_with(prefix) {
        return None;
    }
    if let Ok(uri) = loc.parse::<Uri>() {
        if let Some(auth) = uri.authority() {
            let host = auth.host();
            if host == "127.0.0.1" || host == "localhost" {
                let path = uri.path();
                let q = uri
                    .query()
                    .map(|q| format!("?{q}"))
                    .unwrap_or_default();
                return Some(format!("{prefix}{}{q}", path.trim_start_matches('/')));
            }
        }
    }
    if loc.starts_with('/') && !loc.starts_with("//") {
        return Some(format!("{prefix}{}", loc.trim_start_matches('/')));
    }
    None
}

fn get_role(state: &AppState, user_id: Uuid, session_id: Uuid) -> Option<Role> {
    if user_id.is_nil() {
        return Some(Role::Owner);
    }
    state.auth.member_role(session_id, user_id).ok().flatten()
}
