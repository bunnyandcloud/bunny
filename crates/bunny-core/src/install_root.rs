//! Resolve pre-built install layout (release tarball / Docker image) vs git checkout.

use std::path::{Path, PathBuf};

const WEB_DIST_REL: &str = "web/dist/index.html";

/// Root directory containing `share/bunny/` (or equivalent layout).
pub fn install_root() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("BUNNY_INSTALL_DIR") {
        let p = PathBuf::from(dir);
        if install_share_dir_from_root(&p).is_some() {
            return Some(p);
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent()?.to_path_buf();
        for _ in 0..6 {
            if install_share_dir_from_root(&dir).is_some() {
                return Some(dir);
            }
            if let Some(share) = share_relative_to_bin(&dir) {
                if share.join(WEB_DIST_REL).is_file() {
                    return dir.parent().map(|p| p.to_path_buf()).or(Some(dir));
                }
            }
            if !dir.pop() {
                break;
            }
        }
    }

    None
}

fn install_share_dir_from_root(root: &Path) -> Option<PathBuf> {
    let share = root.join("share/bunny");
    if share.join(WEB_DIST_REL).is_file() {
        return Some(share);
    }
    None
}

fn share_relative_to_bin(bin_dir: &Path) -> Option<PathBuf> {
    for rel in ["../share/bunny", "../../share/bunny"] {
        let share = bin_dir.join(rel);
        if share.join(WEB_DIST_REL).is_file() {
            return Some(share);
        }
    }
    None
}

/// `{install}/share/bunny` when a release layout is detected.
pub fn install_share_dir() -> Option<PathBuf> {
    if let Some(root) = install_root() {
        return install_share_dir_from_root(&root).or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
                .and_then(|bin_dir| share_relative_to_bin(&bin_dir))
        });
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
        .and_then(|bin_dir| share_relative_to_bin(&bin_dir))
}

pub fn web_dist_path() -> Option<PathBuf> {
    install_share_dir().map(|share| share.join("web/dist"))
}

pub fn sidecar_dir(name: &str) -> Option<PathBuf> {
    install_share_dir()
        .map(|share| share.join(name))
        .filter(|dir| dir.join("index.js").is_file())
}

/// Monorepo root containing `apps/web/package.json` (dev checkout).
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

pub fn is_dev_checkout() -> bool {
    find_repo_root()
        .map(|root| root.join("apps/web/src").is_dir())
        .unwrap_or(false)
}

pub fn resolved_web_dist() -> Option<PathBuf> {
    if let Some(dist) = web_dist_path() {
        if dist.join("index.html").is_file() {
            return Some(dist);
        }
    }
    find_repo_root().and_then(|root| {
        let dist = root.join("apps/web/dist");
        if dist.join("index.html").is_file() {
            Some(dist)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_relative_paths() {
        let tmp = std::env::temp_dir().join("bunny-install-test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("share/bunny/web/dist")).unwrap();
        std::fs::write(tmp.join("share/bunny/web/dist/index.html"), "ok").unwrap();
        assert!(install_share_dir_from_root(&tmp).is_some());
    }
}
