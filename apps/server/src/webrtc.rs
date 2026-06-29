use crate::state::AppState;
use anyhow::{anyhow, Result};
use bunny_core::install_root;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tracing::{info, warn};
use uuid::Uuid;

pub struct WebRtcSidecar {
    pub port: u16,
    _child: Child,
}

pub fn sidecar_script_path() -> Option<PathBuf> {
    if let Some(dir) = install_root::sidecar_dir("webrtc-sidecar") {
        let script = dir.join("index.js");
        if script.is_file() {
            return Some(script);
        }
    }
    for p in [
        PathBuf::from("apps/server/webrtc-sidecar/index.js"),
        PathBuf::from("webrtc-sidecar/index.js"),
    ] {
        if p.is_file() {
            return Some(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let p = parent.join("webrtc-sidecar/index.js");
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

pub async fn spawn_webrtc_sidecar(state: Arc<AppState>) -> Result<WebRtcSidecar> {
    let script = sidecar_script_path()
        .ok_or_else(|| anyhow!("webrtc-sidecar/index.js not found"))?;
    let port = state.config.webrtc.sidecar_port;
    let stun = state.config.webrtc.stun_urls.join(",");

    let mut child = Command::new("node")
        .arg(&script)
        .env("BUNNY_WEBRTC_PORT", port.to_string())
        .env("BUNNY_STUN_URLS", stun)
        .env(
            "BUNNY_TURN_URL",
            state.config.webrtc.turn_url.clone().unwrap_or_default(),
        )
        .env(
            "BUNNY_TURN_USERNAME",
            state
                .config
                .webrtc
                .turn_username
                .clone()
                .unwrap_or_default(),
        )
        .env(
            "BUNNY_TURN_CREDENTIAL",
            state
                .config
                .webrtc
                .turn_credential
                .clone()
                .unwrap_or_default(),
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    // Wait for health
    for _ in 0..30 {
        if health_ok(port).await {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("no stdout from webrtc sidecar"))?;

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
                    warn!(%e, "webrtc sidecar bad json");
                    continue;
                }
            };
            let event_type = parsed.get("type").and_then(|t| t.as_str());

            if event_type == Some("webrtc.browser.ice") {
                if let Some(browser_id) = parsed
                    .get("browserId")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Uuid::parse_str(s).ok())
                {
                    if let Some(session_id) =
                        state_clone.browser_sessions.read().get(&browser_id).copied()
                    {
                        if let Some(candidate) = parsed.get("candidate") {
                            state_clone.webrtc_ice_broadcast(session_id, candidate.clone());
                        }
                    }
                }
                continue;
            }

            let Some(session_id) = parsed
                .get("sessionId")
                .and_then(|v| v.as_str())
                .and_then(|s| Uuid::parse_str(s).ok())
            else {
                continue;
            };
            if event_type == Some("webrtc.message") {
                if let Some(data) = parsed.get("data").and_then(|d| d.as_str()) {
                    if let Ok(inner) = serde_json::from_str::<serde_json::Value>(data) {
                        hub.publish(session_id, &inner);
                    } else {
                        hub.publish(
                            session_id,
                            &serde_json::json!({
                                "type": "webrtc.message",
                                "data": data,
                            }),
                        );
                    }
                }
            }
            if event_type == Some("webrtc.ice") {
                if let Some(candidate) = parsed.get("candidate") {
                    let _ = state_clone.webrtc_ice_broadcast(session_id, candidate.clone());
                }
            }
        }
    });

    info!(%port, "WebRTC sidecar started");
    Ok(WebRtcSidecar { port, _child: child })
}

async fn health_ok(port: u16) -> bool {
    let url = format!("http://127.0.0.1:{port}/health");
    reqwest::Client::new()
        .get(url)
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IceServerConfig {
    pub urls: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WebRtcConfigResponse {
    pub enabled: bool,
    pub ice_servers: Vec<IceServerConfig>,
    pub sidecar_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdpPayload {
    #[serde(rename = "type")]
    pub sdp_type: String,
    pub sdp: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IceCandidatePayload {
    pub candidate: serde_json::Value,
}

impl AppState {
    pub fn webrtc_ice_servers(&self) -> Vec<IceServerConfig> {
        let mut servers: Vec<IceServerConfig> = self
            .config
            .webrtc
            .stun_urls
            .iter()
            .map(|u| IceServerConfig {
                urls: vec![u.clone()],
                username: None,
                credential: None,
            })
            .collect();
        if let Some(turn) = &self.config.webrtc.turn_url {
            servers.push(IceServerConfig {
                urls: vec![turn.clone()],
                username: self.config.webrtc.turn_username.clone(),
                credential: self.config.webrtc.turn_credential.clone(),
            });
        }
        servers
    }

    pub async fn webrtc_post_offer(
        &self,
        session_id: Uuid,
        offer: SdpPayload,
    ) -> Result<SdpPayload> {
        let port = self
            .webrtc_port()
            .ok_or_else(|| anyhow!("WebRTC sidecar not running"))?;
        let url = format!("http://127.0.0.1:{port}/v1/sessions/{session_id}/offer");
        let client = reqwest::Client::new();
        let res = client
            .post(&url)
            .json(&serde_json::json!({
                "type": offer.sdp_type,
                "sdp": offer.sdp,
            }))
            .send()
            .await?;
        if !res.status().is_success() {
            return Err(anyhow!("webrtc offer failed: {}", res.status()));
        }
        let body: serde_json::Value = res.json().await?;
        Ok(SdpPayload {
            sdp_type: body["type"].as_str().unwrap_or("answer").into(),
            sdp: body["sdp"].as_str().unwrap_or("").into(),
        })
    }

    pub async fn webrtc_post_candidate(
        &self,
        session_id: Uuid,
        candidate: serde_json::Value,
    ) -> Result<()> {
        let port = self
            .webrtc_port()
            .ok_or_else(|| anyhow!("WebRTC sidecar not running"))?;
        let url = format!("http://127.0.0.1:{port}/v1/sessions/{session_id}/candidate");
        reqwest::Client::new()
            .post(url)
            .json(&serde_json::json!({ "candidate": candidate }))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub fn webrtc_port(&self) -> Option<u16> {
        self.webrtc_sidecar
            .read()
            .as_ref()
            .map(|s| s.port)
    }

    pub fn webrtc_ice_broadcast(&self, session_id: Uuid, candidate: serde_json::Value) {
        self.realtime.publish(
            session_id,
            &serde_json::json!({
                "type": "webrtc.ice",
                "candidate": candidate,
            }),
        );
    }

    pub async fn webrtc_browser_offer(
        &self,
        browser_id: Uuid,
        cdp_port: u16,
        offer: SdpPayload,
    ) -> Result<SdpPayload> {
        let port = self
            .webrtc_port()
            .ok_or_else(|| anyhow!("WebRTC sidecar not running"))?;
        let url = format!(
            "http://127.0.0.1:{port}/v1/browser-sessions/{browser_id}/offer"
        );
        let res = reqwest::Client::new()
            .post(&url)
            .json(&serde_json::json!({
                "type": offer.sdp_type,
                "sdp": offer.sdp,
                "cdpPort": cdp_port,
            }))
            .send()
            .await?;
        if !res.status().is_success() {
            return Err(anyhow!("browser webrtc offer failed: {}", res.status()));
        }
        let body: serde_json::Value = res.json().await?;
        Ok(SdpPayload {
            sdp_type: body["type"].as_str().unwrap_or("answer").into(),
            sdp: body["sdp"].as_str().unwrap_or("").into(),
        })
    }

    pub async fn webrtc_browser_candidate(
        &self,
        browser_id: Uuid,
        candidate: serde_json::Value,
    ) -> Result<()> {
        let port = self
            .webrtc_port()
            .ok_or_else(|| anyhow!("WebRTC sidecar not running"))?;
        let url = format!(
            "http://127.0.0.1:{port}/v1/browser-sessions/{browser_id}/candidate"
        );
        reqwest::Client::new()
            .post(url)
            .json(&serde_json::json!({ "candidate": candidate }))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn webrtc_browser_stop(&self, browser_id: Uuid) -> Result<()> {
        let port = self
            .webrtc_port()
            .ok_or_else(|| anyhow!("WebRTC sidecar not running"))?;
        let url = format!("http://127.0.0.1:{port}/v1/browser-sessions/{browser_id}/stop");
        let _ = reqwest::Client::new().post(url).send().await?;
        Ok(())
    }
}
