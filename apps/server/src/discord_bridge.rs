use crate::discord_ops;
use crate::state::AppState;
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
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
        bail!("bridge config not found at {}", cfg.display());
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
        bail!("discord bridge is not configured");
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
        if locate_bridge_binary().is_some() {
            return;
        }
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
    if let Some(bin) = locate_bridge_binary() {
        return Ok(bin);
    }
    tokio::task::spawn_blocking(build_bridge_binary)
        .await
        .context("discord bridge build task")??;
    locate_bridge_binary().ok_or_else(|| {
        anyhow::anyhow!("bunny-discord-bridge binary not found (set BUNNY_DISCORD_BRIDGE_BIN)")
    })
}

fn build_bridge_binary() -> Result<()> {
    let root = workspace_root()?;
    tracing::info!("building discord bridge (first time)…");
    let status = std::process::Command::new("cargo")
        .current_dir(&root)
        .args(["build", "--release", "-p", "bunny-discord-bridge", "-q"])
        .status()?;
    if !status.success() {
        bail!("failed to build bunny-discord-bridge");
    }
    Ok(())
}

fn locate_bridge_binary() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("BUNNY_DISCORD_BRIDGE_BIN") {
        if !path.is_empty() {
            let p = PathBuf::from(path);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    workspace_root()
        .ok()
        .map(|root| resolve_bridge_binary(&root))
        .filter(|p| p.is_file())
}

fn resolve_bridge_binary(root: &Path) -> PathBuf {
    let debug = root.join("target/debug/bunny-discord-bridge");
    let release = root.join("target/release/bunny-discord-bridge");
    if debug.is_file() {
        if !release.is_file() {
            return debug;
        }
        let debug_mtime = debug.metadata().and_then(|m| m.modified()).ok();
        let release_mtime = release.metadata().and_then(|m| m.modified()).ok();
        if debug_mtime >= release_mtime {
            return debug;
        }
    }
    release
}

fn workspace_root() -> Result<PathBuf> {
    let mut dir = std::env::current_dir()?;
    loop {
        let manifest = dir.join("Cargo.toml");
        if manifest.is_file() {
            let text = std::fs::read_to_string(&manifest)?;
            if text.contains("[workspace]") {
                return Ok(dir);
            }
        }
        if !dir.pop() {
            break;
        }
    }
    if let Some(root) = crate::web_ui::find_repo_root() {
        return Ok(root);
    }
    bail!("run from the bunny repo root (workspace Cargo.toml not found)")
}

pub async fn shutdown_managed(state: &AppState) {
    let _ = stop_managed(state).await;
}
