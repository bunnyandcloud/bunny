use crate::realtime::map_cdp_line_to_event;
use crate::state::AppState;
use anyhow::{anyhow, Result};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tracing::{info, warn};
use uuid::Uuid;

#[allow(dead_code)]
pub struct CdpCollectorHandle {
    pub child: Child,
    pub browser_id: Uuid,
    pub session_id: Uuid,
}

pub fn sidecar_script_path() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("apps/server/cdp-sidecar/index.js"),
        PathBuf::from("cdp-sidecar/index.js"),
    ];
    for p in candidates {
        if p.exists() {
            return Some(p);
        }
    }
    // Relative to executable for installed binary
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let p = parent.join("cdp-sidecar/index.js");
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

pub fn extract_oauth_script_path() -> Option<PathBuf> {
    sidecar_script_path().and_then(|p| {
        let sibling = p.parent()?.join("extract-oauth-code.js");
        if sibling.exists() {
            Some(sibling)
        } else {
            None
        }
    })
}

pub async fn spawn_cdp_collector(
    state: Arc<AppState>,
    session_id: Uuid,
    browser_id: Uuid,
    cdp_port: u16,
) -> Result<CdpCollectorHandle> {
    let script = sidecar_script_path().ok_or_else(|| {
        anyhow!("cdp-sidecar/index.js not found — run from repo root or install sidecar")
    })?;

    let cdp_url = format!("http://127.0.0.1:{cdp_port}");

    let mut child = tokio::process::Command::new("node")
        .arg(&script)
        .env("BUNNY_CDP_URL", &cdp_url)
        .env("BUNNY_CDP_PORT", cdp_port.to_string())
        .env("BUNNY_SESSION_ID", session_id.to_string())
        .env("BUNNY_BROWSER_ID", browser_id.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    info!(%session_id, %browser_id, %cdp_url, "CDP collector started");

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("no stdout from cdp sidecar"))?;

    let hub = Arc::clone(&state.realtime);
    let state_clone = state.clone();

    tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            let parsed: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(e) => {
                    warn!(error = %e, "invalid cdp json line");
                    continue;
                }
            };

            if parsed.get("type").and_then(|t| t.as_str()) == Some("claude.oauth_code") {
                // Legacy — auto-import disabled; import is manual via /claude/auth/detect-code only.
                continue;
            }

            if let Some(event) = map_cdp_line_to_event(&parsed) {
                let event_type = event
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("browser.event")
                    .to_string();

                hub.publish(session_id, &event);

                if event_type.starts_with("browser.") || event_type.starts_with("recovery.") {
                    let _ = state_clone.record_timeline(
                        session_id,
                        "browser",
                        &event_type,
                        event.clone(),
                    );
                }
            }
        }
        warn!(%session_id, "CDP collector stdout closed");
        state_clone.realtime.publish(
            session_id,
            &serde_json::json!({
                "type": "recovery.failed",
                "component": "cdp_collector",
                "detail": "collector exited"
            }),
        );
    });

    Ok(CdpCollectorHandle {
        child,
        browser_id,
        session_id,
    })
}
