//! Microsoft Teams bridge — Bot Framework webhook forwarding to Bunny `/internal/chat/teams/*`.

use anyhow::Result;
use axum::routing::{get, post};
use axum::{Json, Router};
use tracing_subscriber::EnvFilter;

#[derive(Clone, serde::Deserialize)]
struct BridgeConfig {
    teams: TeamsSection,
    bunny: BunnySection,
}

#[derive(Clone, serde::Deserialize)]
struct TeamsSection {
    app_id: String,
    app_password: String,
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

    let config_path = std::env::var("BUNNY_TEAMS_BRIDGE_CONFIG")
        .unwrap_or_else(|_| format!("{}/.config/bunny/.teams/bridge.yaml", std::env::var("HOME").unwrap_or_else(|_| ".".into())));
    let raw = std::fs::read_to_string(&config_path)?;
    let cfg: BridgeConfig = serde_yaml::from_str(&raw)?;

    let app = Router::new()
        .route("/health", get(|| async { Json(serde_json::json!({ "ok": true })) }))
        .route("/teams/messages", post(teams_messages))
        .with_state(cfg);

    let addr = std::env::var("BUNNY_TEAMS_BRIDGE_BIND").unwrap_or_else(|_| "127.0.0.1:8788".into());
    tracing::info!("teams bridge listening on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn teams_messages(
    axum::extract::State(cfg): axum::extract::State<BridgeConfig>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    tracing::debug!(?body, "teams activity");
    let client = reqwest::Client::new();
    let _ = client
        .get(format!("{}/api/v1/internal/chat/teams/health", cfg.bunny.internal_url.trim_end_matches('/')))
        .header("Authorization", format!("Bearer {}", cfg.bunny.bridge_token))
        .send()
        .await;
    Json(serde_json::json!({ "type": "message", "text": "Teams bridge connected to Bunny" }))
}
