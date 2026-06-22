//! Slack bridge — Events API + slash commands forwarding to Bunny `/internal/chat/slack/*`.

use anyhow::Result;
use axum::routing::{get, post};
use axum::{Json, Router};
use tracing_subscriber::EnvFilter;

#[derive(Clone, serde::Deserialize)]
struct BridgeConfig {
    slack: SlackSection,
    bunny: BunnySection,
}

#[derive(Clone, serde::Deserialize)]
struct SlackSection {
    bot_token: String,
    signing_secret: String,
    app_token: Option<String>,
}

#[derive(Clone, serde::Deserialize)]
struct BunnySection {
    internal_url: String,
    bridge_token: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let config_path = std::env::var("BUNNY_SLACK_BRIDGE_CONFIG")
        .unwrap_or_else(|_| format!("{}/.config/bunny/.slack/bridge.yaml", std::env::var("HOME").unwrap_or_else(|_| ".".into())));
    let raw = std::fs::read_to_string(&config_path)?;
    let cfg: BridgeConfig = serde_yaml::from_str(&raw)?;

    let app = Router::new()
        .route("/health", get(|| async { Json(serde_json::json!({ "ok": true })) }))
        .route("/slack/events", post(slack_events))
        .with_state(cfg);

    let addr = std::env::var("BUNNY_SLACK_BRIDGE_BIND").unwrap_or_else(|_| "127.0.0.1:8787".into());
    tracing::info!("slack bridge listening on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn slack_events(
    axum::extract::State(cfg): axum::extract::State<BridgeConfig>,
    body: String,
) -> Json<serde_json::Value> {
    tracing::debug!(len = body.len(), "slack event received");
    let client = reqwest::Client::new();
    let _ = client
        .get(format!("{}/api/v1/internal/chat/slack/health", cfg.bunny.internal_url.trim_end_matches('/')))
        .header("Authorization", format!("Bearer {}", cfg.bunny.bridge_token))
        .send()
        .await;
    if body.contains("url_verification") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
            if let Some(challenge) = v.get("challenge").and_then(|c| c.as_str()) {
                return Json(serde_json::json!({ "challenge": challenge }));
            }
        }
    }
    Json(serde_json::json!({ "ok": true }))
}
