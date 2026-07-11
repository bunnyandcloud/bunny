use crate::api::ApiError;
use crate::state::AppState;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// Live browser stacks registered for a Bunny session (may include stale map entries).
fn live_browser_ids(state: &AppState, session_id: Uuid) -> Vec<Uuid> {
    state
        .browser_sessions
        .read()
        .iter()
        .filter(|(_, sid)| **sid == session_id)
        .map(|(id, _)| *id)
        .filter(|id| state.browsers.get_novnc_port(*id).is_some())
        .collect()
}

/// Keep one browser per session; stop duplicate Chromium stacks.
pub fn consolidate_session_browsers(
    state: &AppState,
    session_id: Uuid,
    prefer: Option<Uuid>,
) -> Option<Uuid> {
    let live = live_browser_ids(state, session_id);
    if live.is_empty() {
        state
            .browser_sessions
            .write()
            .retain(|_, sid| *sid != session_id);
        return None;
    }

    let keep = prefer
        .filter(|id| live.contains(id))
        .or_else(|| live.iter().copied().max());

    let Some(keep) = keep else {
        return None;
    };

    for id in live {
        if id != keep {
            state.browsers.stop(id);
            state.browser_sessions.write().remove(&id);
        }
    }
    Some(keep)
}

pub fn resolve_session_browser_id(
    state: &AppState,
    session_id: Uuid,
    prefer: Option<Uuid>,
) -> Result<Uuid, ApiError> {
    consolidate_session_browsers(state, session_id, prefer).ok_or_else(|| {
        ApiError::not_found("no browser stream in this session — start the Browser tab or stream_browser_start")
    })
}

fn browser_cdp_ready(state: &AppState, id: Uuid) -> bool {
    state
        .browsers
        .get_cdp_port(id)
        .is_some_and(bunny_browser::stack::tcp_port_open)
}

async fn wait_for_browser_cdp(state: &AppState, id: Uuid, timeout: Duration) {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if browser_cdp_ready(state, id) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

async fn create_browser_blocking(
    state: Arc<AppState>,
    session_id: Uuid,
    url: &str,
) -> Result<Uuid, ApiError> {
    let url = url.to_string();
    tokio::task::spawn_blocking(move || {
        state
            .browsers
            .create(session_id, &url)
            .map_err(|e| ApiError::validation(&e.to_string()))
    })
    .await
    .map_err(|e| ApiError::validation(&e.to_string()))?
}

async fn restart_browser_blocking(
    state: Arc<AppState>,
    id: Uuid,
    url: &str,
) -> Result<(), ApiError> {
    let url = url.to_string();
    tokio::task::spawn_blocking(move || {
        state
            .browsers
            .restart(id, &url)
            .map_err(|e| ApiError::validation(&e.to_string()))
    })
    .await
    .map_err(|e| ApiError::validation(&e.to_string()))?
}

pub async fn find_or_create_browser(
    state: Arc<AppState>,
    session_id: Uuid,
    url: &str,
) -> Result<Uuid, ApiError> {
    if let Some(id) = consolidate_session_browsers(&state, session_id, None) {
        if browser_cdp_ready(&state, id) {
            state
                .clone()
                .start_browser_cdp(session_id, id)
                .await
                .map_err(|e| ApiError::validation(&e.to_string()))?;
            return Ok(id);
        }
        restart_browser_blocking(Arc::clone(&state), id, url).await?;
        wait_for_browser_cdp(&state, id, Duration::from_secs(25)).await;
        state
            .clone()
            .start_browser_cdp(session_id, id)
            .await
            .map_err(|e| ApiError::validation(&e.to_string()))?;
        return Ok(id);
    }

    let created = create_browser_blocking(Arc::clone(&state), session_id, url).await?;
    wait_for_browser_cdp(&state, created, Duration::from_secs(25)).await;
    state
        .clone()
        .start_browser_cdp(session_id, created)
        .await
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(created)
}
