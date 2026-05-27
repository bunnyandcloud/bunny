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

/// Rollup ships platform-specific optional packages; `node_modules` from another OS breaks Vite builds.
fn rollup_native_pkg() -> Option<&'static str> {
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        return Some("@rollup/rollup-linux-arm64-gnu");
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        return Some("@rollup/rollup-linux-x64-gnu");
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        return Some("@rollup/rollup-darwin-arm64");
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        return Some("@rollup/rollup-darwin-x64");
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        return Some("@rollup/rollup-win32-x64-msvc");
    }
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    {
        return Some("@rollup/rollup-win32-arm64-msvc");
    }
    #[allow(unreachable_code)]
    None
}

fn web_deps_need_install(web_dir: &Path) -> bool {
    if !web_dir.join("node_modules").is_dir() {
        return true;
    }
    let Some(pkg) = rollup_native_pkg() else {
        return false;
    };
    !web_dir.join("node_modules").join(pkg).is_dir()
}

fn install_web_deps(web_dir: &Path) -> Result<()> {
    println!("→ Installing web UI dependencies…");
    if web_dir.join("package-lock.json").is_file() {
        run_npm(web_dir, &["ci", "--no-fund", "--no-audit"])?;
    } else {
        run_npm(web_dir, &["install", "--no-fund", "--no-audit"])?;
    }
    Ok(())
}

pub fn ensure_web_ui_built(repo_root: &Path) -> Result<PathBuf> {
    let web_dir = repo_root.join("apps/web");
    let dist = web_dir.join("dist");
    let dist_ready = dist.join("index.html").is_file();
    let stale = dist_ready && web_ui_stale(&web_dir, &dist);

    if dist_ready && !stale {
        println!("✓ Web UI already built ({})", dist.display());
        return Ok(dist);
    }

    if stale {
        println!("→ Web UI sources changed — rebuilding (apps/web)…");
    }

    if !Command::new("npm")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        bail!(
            "Node.js/npm required to build the web UI. Install Node 20+, then run:\n  \
             cd {} && npm ci && npm run build",
            web_dir.display()
        );
    }

    if web_deps_need_install(&web_dir) {
        if web_dir.join("node_modules").is_dir() {
            eprintln!(
                "  (rollup native module missing for this platform — reinstalling deps; \
                 do not reuse node_modules from macOS on Linux/Docker)"
            );
        }
        install_web_deps(&web_dir)?;
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
