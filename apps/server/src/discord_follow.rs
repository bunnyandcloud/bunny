//! Background worker: polls active Discord follow subscriptions and could notify the bridge.
//! Snapshots are generated server-side; the bridge polls or receives webhooks in a future iteration.

use crate::state::AppState;
use std::sync::Arc;
use tracing::debug;

pub fn spawn_follow_worker(state: Arc<AppState>) {
    if !state.config.discord.enabled {
        return;
    }
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
            let follows = match state.discord.lock().list_active_follows() {
                Ok(f) => f,
                Err(e) => {
                    debug!("follow list error: {e}");
                    continue;
                }
            };
            for follow in follows {
                debug!(
                    session = %follow.session_id,
                    target = %follow.target,
                    interval = follow.interval_secs,
                    "discord follow active"
                );
            }
        }
    });
}
