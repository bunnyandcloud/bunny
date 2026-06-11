use crate::discord_bridge_binary;
use crate::discord_ops;
use crate::state::AppState;
use anyhow::{Context, Result};
use serde::Serialize;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Child;

pub struct DiscordBridgeSidecar {
    child: Child,
}

#[derive(Serialize)]
pub struct DiscordBridgeReloadResponse {
    pub ok: bool,
    pub bridge_running: bool,
    /// Bridge restart was queued; build/start may still be in progress.
    pub bridge_starting: bool,
    pub bridge_path: String,
}

pub async fn start_managed(state: &AppState) -> Result<bool> {
    if !discord_ops::discord_bridge_configured(state) {
        return Ok(false);
    }
    let cfg = discord_ops::default_bridge_path();
    if !cfg.is_file() {
        anyhow::bail!("bridge config not found at {}", cfg.display());
    }
    if let Err(e) = crate::config_init::sync_agent_from_bridge_file(&cfg) {
        tracing::warn!("discord bridge config sync: {e}");
    }
    let bridge_bin = ensure_bridge_binary().await?;
    stop_orphan_bridge_processes().await;
    let child = tokio::process::Command::new(&bridge_bin)
        .env("BUNNY_DISCORD_BRIDGE_CONFIG", &cfg)
        .env(
            "RUST_LOG",
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "bunny_discord_bridge=info,serenity=warn".into()),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("spawn discord bridge ({})", bridge_bin.display()))?;
    tracing::info!("discord bridge started ({})", cfg.display());
    *state.discord_bridge.lock().await = Some(DiscordBridgeSidecar { child });
    Ok(true)
}

pub async fn stop_managed(state: &AppState) -> Result<()> {
    if let Some(mut sidecar) = state.discord_bridge.lock().await.take() {
        let _ = sidecar.child.kill().await;
        let _ = sidecar.child.wait().await;
    }
    stop_orphan_bridge_processes().await;
    Ok(())
}

pub async fn restart_managed(state: &AppState) -> Result<DiscordBridgeReloadResponse> {
    if !discord_ops::discord_bridge_configured(state) {
        anyhow::bail!("discord bridge is not configured");
    }
    let bridge_path = discord_ops::default_bridge_path();
    stop_managed(state).await?;
    let _started = start_managed(state).await?;
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    let still_running = bridge_process_alive(state).await;
    Ok(DiscordBridgeReloadResponse {
        ok: still_running,
        bridge_running: still_running,
        bridge_starting: false,
        bridge_path: bridge_path.display().to_string(),
    })
}

/// Restart the bridge without blocking the caller (e.g. HTTP handlers).
pub fn spawn_restart_managed(state: Arc<AppState>) {
    tokio::spawn(async move {
        match restart_managed(&state).await {
            Ok(r) => tracing::info!(
                running = r.bridge_running,
                "discord bridge background restart finished"
            ),
            Err(e) => tracing::warn!("discord bridge background restart: {e}"),
        }
    });
}

/// Build the bridge binary in the background so first setup does not block HTTP requests.
pub fn spawn_prefetch_binary() {
    tokio::spawn(async move {
        if let Err(e) = ensure_bridge_binary().await {
            tracing::debug!("discord bridge binary prefetch: {e}");
        } else {
            tracing::info!("discord bridge binary prefetched");
        }
    });
}

async fn bridge_process_alive(state: &AppState) -> bool {
    let mut slot = state.discord_bridge.lock().await;
    if let Some(sidecar) = slot.as_mut() {
        match sidecar.child.try_wait() {
            Ok(None) => return true,
            Ok(Some(_)) => {
                slot.take();
                return false;
            }
            Err(_) => return false,
        }
    }
    false
}

async fn stop_orphan_bridge_processes() {
    let _ = tokio::process::Command::new("pkill")
        .args(["-f", "bunny-discord-bridge"])
        .status()
        .await;
}

async fn ensure_bridge_binary() -> Result<PathBuf> {
    tokio::task::spawn_blocking(|| discord_bridge_binary::ensure_bridge_binary_sync(None))
        .await
        .context("discord bridge build task")?
}

pub async fn shutdown_managed(state: &AppState) {
    let _ = stop_managed(state).await;
}
