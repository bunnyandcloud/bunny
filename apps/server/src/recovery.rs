use crate::state::AppState;
use crate::terminals::restore_all_terminals;
use std::sync::Arc;
use tracing::info;

/// Restore session metadata from SQLite after agent restart.
pub fn restore_sessions(state: &Arc<AppState>) {
    let auth_db = state.auth.db();
    let sessions = {
        let db = auth_db.lock();
        db.list_all_stream_sessions().unwrap_or_default()
    };
    for (id, path, status) in sessions {
        if status != "stopped" && status != "expired" {
            info!(%id, %path, "session recoverable after restart");
            let _ = state.record_timeline(
                id,
                "recovery",
                "recovery.started",
                serde_json::json!({
                    "detail": "agent restarted",
                    "previousStatus": status,
                    "newStatus": "recoverable"
                }),
            );
        }
    }
    restore_all_terminals(state);
}

/// Spawn relay reconnect loop when configured.
pub fn spawn_relay_if_enabled(state: Arc<AppState>) {
    let relay_url = state.config.auth.relay_url.clone();
    if relay_url.is_none() {
        return;
    }
    let url = relay_url.unwrap();
    let agent_id = state.data_dir.clone();
    tokio::spawn(async move {
        let client = bunny_relay::RelayClient::new(url, agent_id);
        client.connect_loop().await;
    });
}

/// Periodic health checks for browser stack and supervisor.
pub fn spawn_health_checks(state: Arc<AppState>) {
    if !state.config.recovery.process_supervisor.enabled {
        return;
    }
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
        loop {
            interval.tick().await;
            state.browsers.health_check();
        }
    });
}
