use crate::api::ApiError;
use crate::state::AppState;
use std::sync::Arc;
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

pub async fn find_or_create_browser(
    state: Arc<AppState>,
    session_id: Uuid,
    url: &str,
) -> Result<Uuid, ApiError> {
    if let Some(id) = consolidate_session_browsers(&state, session_id, None) {
        state
            .browsers
            .restart(id, url)
            .map_err(|e| ApiError::validation(&e.to_string()))?;
        state
            .clone()
            .start_browser_cdp(session_id, id)
            .await
            .map_err(|e| ApiError::validation(&e.to_string()))?;
        return Ok(id);
    }

    let created = state
        .browsers
        .create(session_id, url)
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    state
        .clone()
        .start_browser_cdp(session_id, created)
        .await
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    Ok(created)
}
