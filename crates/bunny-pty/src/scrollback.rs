//! Persist terminal output and last cwd so history survives agent restarts.

use std::path::{Path, PathBuf};
use uuid::Uuid;

pub fn scrollback_path(dir: &Path, terminal_id: Uuid) -> PathBuf {
    dir.join(format!("{}.scrollback", terminal_id.as_simple()))
}

pub fn meta_path(dir: &Path, terminal_id: Uuid) -> PathBuf {
    dir.join(format!("{}.cwd", terminal_id.as_simple()))
}

pub fn save(dir: &Path, terminal_id: Uuid, content: &str) {
    if content.is_empty() {
        return;
    }
    let _ = std::fs::create_dir_all(dir);
    let path = scrollback_path(dir, terminal_id);
    if std::fs::write(&path, content).is_ok() {
        let bytes = content.len();
        if bytes > 80 {
            tracing::info!(terminal = %terminal_id, bytes, "scrollback saved");
        } else {
            tracing::debug!(terminal = %terminal_id, bytes, "scrollback saved");
        }
    }
}

pub fn load(dir: &Path, terminal_id: Uuid) -> Option<String> {
    let path = scrollback_path(dir, terminal_id);
    std::fs::read_to_string(&path).ok().filter(|s| !s.is_empty())
}

pub fn save_cwd(dir: &Path, terminal_id: Uuid, cwd: &str) {
    if cwd.is_empty() {
        return;
    }
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(meta_path(dir, terminal_id), cwd);
}

pub fn load_cwd(dir: &Path, terminal_id: Uuid) -> Option<String> {
    std::fs::read_to_string(meta_path(dir, terminal_id))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn save_session(dir: &Path, terminal_id: Uuid, content: &str, cwd: Option<&str>) {
    save(dir, terminal_id, content);
    if let Some(cwd) = cwd {
        save_cwd(dir, terminal_id, cwd);
    }
    if content.is_empty() && cwd.is_some() {
        tracing::debug!(
            terminal = %terminal_id,
            "scrollback empty on save (cwd only)"
        );
    }
}

/// Prefer the longer snapshot; if both differ, keep captured tmux tail with disk prefix.
pub fn merge(disk: Option<String>, captured: String) -> String {
    let captured = captured.trim_end().to_string();
    if captured.is_empty() {
        return disk.unwrap_or_default();
    }
    let Some(disk) = disk.filter(|d| !d.is_empty()) else {
        return captured;
    };
    if disk.contains(captured.as_str()) || captured.len() <= disk.len() {
        return disk;
    }
    if captured.contains(disk.as_str()) {
        return captured;
    }
    format!("{disk}\n{captured}")
}
