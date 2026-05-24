use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashMap;
use tokio::sync::broadcast;
use uuid::Uuid;

const HUB_CAPACITY: usize = 512;

/// Broadcast hub for session-scoped realtime events (browser, terminal meta, recovery).
#[derive(Default)]
pub struct RealtimeHub {
    channels: RwLock<HashMap<Uuid, broadcast::Sender<String>>>,
}

impl RealtimeHub {
    pub fn new() -> Self {
        Self::default()
    }

    fn sender(&self, session_id: Uuid) -> broadcast::Sender<String> {
        let mut channels = self.channels.write();
        channels
            .entry(session_id)
            .or_insert_with(|| broadcast::channel(HUB_CAPACITY).0)
            .clone()
    }

    pub fn publish(&self, session_id: Uuid, event: &Value) {
        if let Ok(json) = serde_json::to_string(event) {
            let _ = self.sender(session_id).send(json);
        }
    }

    pub fn publish_json(&self, session_id: Uuid, json: String) {
        let _ = self.sender(session_id).send(json);
    }

    pub fn subscribe(&self, session_id: Uuid) -> broadcast::Receiver<String> {
        self.sender(session_id).subscribe()
    }

    pub fn remove_session(&self, session_id: Uuid) {
        self.channels.write().remove(&session_id);
    }
}

/// Map CDP sidecar JSON line to realtime protocol event.
pub fn map_cdp_line_to_event(line: &Value) -> Option<Value> {
    let event_type = line.get("type")?.as_str()?;
    match event_type {
        "browser.console" => Some(serde_json::json!({
            "type": "browser.console",
            "level": line.get("level").and_then(|v| v.as_str()).unwrap_or("log"),
            "text": line.get("text").and_then(|v| v.as_str()).unwrap_or(""),
            "url": line.get("url"),
            "ts": line.get("ts"),
        })),
        "browser.pageerror" => Some(serde_json::json!({
            "type": "browser.pageerror",
            "message": line.get("message").and_then(|v| v.as_str()).unwrap_or(""),
            "ts": line.get("ts"),
        })),
        "browser.network" => Some(serde_json::json!({
            "type": "browser.network",
            "phase": line.get("phase").and_then(|v| v.as_str()).unwrap_or("started"),
            "requestId": line.get("requestId").and_then(|v| v.as_str()).unwrap_or(""),
            "method": line.get("method"),
            "urlRedacted": line.get("urlRedacted"),
            "status": line.get("status"),
            "timingMs": line.get("timingMs"),
            "resourceType": line.get("resourceType"),
            "error": line.get("error"),
            "ts": line.get("ts"),
        })),
        "browser.navigation" => Some(serde_json::json!({
            "type": "browser.navigation",
            "urlRedacted": line.get("urlRedacted").or_else(|| line.get("url")),
            "ts": line.get("ts"),
        })),
        "browser.screenshot" => Some(serde_json::json!({
            "type": "browser.screenshot",
            "refId": line.get("refId").and_then(|v| v.as_str()).unwrap_or(""),
            "ts": line.get("ts"),
        })),
        "collector.ready" => Some(serde_json::json!({
            "type": "browser.collector.ready",
            "cdpUrl": line.get("cdpUrl"),
        })),
        "collector.crashed" => Some(serde_json::json!({
            "type": "recovery.failed",
            "component": "cdp_collector",
            "detail": line.get("message"),
        })),
        _ => None,
    }
}
