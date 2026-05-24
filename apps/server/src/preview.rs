use crate::state::AppState;
use axum::{
    body::Body,
    extract::{Extension, Path, State},
    http::{Request, StatusCode},
    response::Response,
    routing::any,
    Router,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/s/:session_id/ports/:port/*path", any(proxy_handler))
        .with_state(state)
}

async fn proxy_handler(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Uuid>,
    Path((session_id, port, path)): Path<(Uuid, u16, String)>,
    req: Request<Body>,
) -> Result<Response, StatusCode> {
    let role = get_role(&state, user, session_id).unwrap_or(bunny_core::types::Role::Viewer);
    if !bunny_core::permissions::role_can(role, bunny_core::permissions::Action::PreviewView) {
        return Err(StatusCode::FORBIDDEN);
    }

    let method = req.method().clone();
    let target = format!("http://127.0.0.1:{port}/{path}");

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut builder = client.request(method, &target);
    for (k, v) in req.headers().iter() {
        let kl = k.as_str().to_lowercase();
        if !matches!(kl.as_str(), "host" | "cookie" | "authorization" | "connection") {
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
    let mut response = Response::builder().status(status);
    for (k, v) in resp.headers().iter() {
        let kl = k.as_str().to_lowercase();
        if !matches!(kl.as_str(), "set-cookie" | "transfer-encoding") {
            if let Ok(s) = v.to_str() {
                response = response.header(k.as_str(), s);
            }
        }
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    response
        .body(Body::from(bytes))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn get_role(state: &AppState, user_id: Uuid, session_id: Uuid) -> Option<bunny_core::types::Role> {
    if user_id.is_nil() {
        return Some(bunny_core::types::Role::Owner);
    }
    state.auth.member_role(session_id, user_id).ok().flatten()
}
