use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

/// Monorepo root containing `apps/web/package.json`.
pub fn find_repo_root() -> Option<PathBuf> {
    if let Ok(dir) = std::env::current_dir() {
        if let Some(root) = search_repo_root(&dir) {
            return Some(root);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent()?.to_path_buf();
        for _ in 0..8 {
            if let Some(root) = search_repo_root(&dir) {
                return Some(root);
            }
            if !dir.pop() {
                break;
            }
        }
    }
    None
}

fn search_repo_root(dir: &Path) -> Option<PathBuf> {
    let mut current = dir.to_path_buf();
    loop {
        if current.join("apps/web/package.json").is_file() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

pub fn web_dist_dir(repo_root: Option<&Path>) -> Option<PathBuf> {
    if let Some(root) = repo_root {
        let dist = root.join("apps/web/dist");
        if dist.join("index.html").is_file() {
            return Some(dist);
        }
    }
    let rel = PathBuf::from("apps/web/dist");
    if rel.join("index.html").is_file() {
        return Some(rel);
    }
    None
}

fn dir_max_mtime(dir: &Path) -> Option<SystemTime> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut max: Option<SystemTime> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        let candidate = if path.is_dir() {
            dir_max_mtime(&path)
        } else {
            entry.metadata().ok().and_then(|m| m.modified().ok())
        };
        if let Some(t) = candidate {
            max = Some(max.map(|m| m.max(t)).unwrap_or(t));
        }
    }
    max
}

fn web_ui_stale(web_dir: &Path, dist: &Path) -> bool {
    let dist_index = dist.join("index.html");
    let Ok(dist_mtime) = dist_index.metadata().and_then(|m| m.modified()) else {
        return true;
    };
    let src = web_dir.join("src");
    dir_max_mtime(&src).is_some_and(|src_mtime| src_mtime > dist_mtime)
}

pub fn ensure_web_ui_built(repo_root: &Path) -> Result<PathBuf> {
    let web_dir = repo_root.join("apps/web");
    let dist = web_dir.join("dist");
    if dist.join("index.html").is_file() {
        if web_ui_stale(&web_dir, &dist) {
            println!("→ Web UI sources changed — rebuilding (apps/web)…");
        } else {
            println!("✓ Web UI already built ({})", dist.display());
            return Ok(dist);
        }
    }

    if !Command::new("npm")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        bail!(
            "Node.js/npm required to build the web UI. Install Node 20+, then run:\n  \
             cd {} && npm install && npm run build",
            web_dir.display()
        );
    }

    if !web_dir.join("node_modules").is_dir() {
        println!("→ Installing web UI dependencies…");
        run_npm(&web_dir, &["install"])?;
    }

    println!("→ Building web UI (apps/web)…");
    run_npm(&web_dir, &["run", "build"])?;

    if !dist.join("index.html").is_file() {
        bail!("web UI build finished but {} is missing", dist.join("index.html").display());
    }
    println!("✓ Web UI built");
    Ok(dist)
}

fn run_npm(cwd: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("npm")
        .current_dir(cwd)
        .args(args)
        .status()
        .with_context(|| format!("npm {} in {}", args.join(" "), cwd.display()))?;
    if !status.success() {
        bail!("npm {} failed (exit {})", args.join(" "), status);
    }
    Ok(())
}
