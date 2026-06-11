use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Ensure `bunny-discord-bridge` exists and is up to date with workspace sources.
pub fn ensure_bridge_binary_sync(log: Option<&dyn Fn(&str)>) -> Result<PathBuf> {
    if let Ok(path) = std::env::var("BUNNY_DISCORD_BRIDGE_BIN") {
        if !path.is_empty() {
            let p = PathBuf::from(path);
            if p.is_file() {
                return Ok(p);
            }
        }
    }

    let root = workspace_root()?;
    if let Some(bin) = locate_workspace_bridge_binary(&root) {
        if !bridge_binary_stale(&root, &bin) {
            return Ok(bin);
        }
        emit_log(
            log,
            "discord bridge sources changed since last build — recompiling…",
        );
    } else {
        emit_log(log, "building discord bridge (first time)…");
    }

    build_bridge_binary(&root)?;
    locate_workspace_bridge_binary(&root).ok_or_else(|| {
        anyhow::anyhow!("bunny-discord-bridge binary not found (set BUNNY_DISCORD_BRIDGE_BIN)")
    })
}

fn emit_log(log: Option<&dyn Fn(&str)>, message: &str) {
    if let Some(log) = log {
        log(&format!("→ {message}"));
    } else {
        tracing::info!("{message}");
    }
}

fn build_bridge_binary(root: &Path) -> Result<()> {
    let status = std::process::Command::new("cargo")
        .current_dir(root)
        .args(["build", "--release", "-p", "bunny-discord-bridge", "-q"])
        .status()?;
    if !status.success() {
        bail!("failed to build bunny-discord-bridge");
    }
    Ok(())
}

fn locate_workspace_bridge_binary(root: &Path) -> Option<PathBuf> {
    let bin = resolve_bridge_binary(root);
    if bin.is_file() {
        Some(bin)
    } else {
        None
    }
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

fn bridge_binary_stale(root: &Path, bin: &Path) -> bool {
    let Ok(meta) = bin.metadata() else {
        return true;
    };
    let Ok(since) = meta.modified() else {
        return true;
    };

    for dir in [
        root.join("apps/discord-bridge"),
        root.join("crates/bunny-discord"),
        root.join("crates/bunny-i18n"),
    ] {
        if dir_has_sources_newer_than(&dir, since) {
            return true;
        }
    }

    file_newer_than(&root.join("packages/i18n/messages.json"), since)
}

fn dir_has_sources_newer_than(dir: &Path, since: SystemTime) -> bool {
    if !dir.is_dir() {
        return false;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if dir_has_sources_newer_than(&path, since) {
                return true;
            }
        } else if is_bridge_source_file(&path) && file_newer_than(&path, since) {
            return true;
        }
    }
    false
}

fn is_bridge_source_file(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") | Some("toml") => true,
        _ => path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == "messages.json"),
    }
}

fn file_newer_than(path: &Path, since: SystemTime) -> bool {
    path.metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .is_some_and(|mtime| mtime > since)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_source_files_include_messages_json() {
        assert!(is_bridge_source_file(Path::new("messages.json")));
        assert!(is_bridge_source_file(Path::new("main.rs")));
        assert!(is_bridge_source_file(Path::new("Cargo.toml")));
        assert!(!is_bridge_source_file(Path::new("README.md")));
    }
}
