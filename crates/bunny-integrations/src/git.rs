use crate::db::{AcquireGitLeaseRequest, GitLeaseStatus, GitWorktreeLease, IntegrationsDb};
use anyhow::{Context, Result};
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
use uuid::Uuid;

pub struct GitWorkspaceManager {
    data_dir: PathBuf,
    db_path: String,
}

impl GitWorkspaceManager {
    pub fn new(data_dir: impl AsRef<Path>, db_path: impl Into<String>) -> Self {
        Self {
            data_dir: data_dir.as_ref().to_path_buf(),
            db_path: db_path.into(),
        }
    }

    fn db(&self) -> Result<IntegrationsDb> {
        IntegrationsDb::open(&self.db_path)
    }

    pub fn acquire(&self, req: AcquireGitLeaseRequest) -> Result<GitWorktreeLease> {
        let db = self.db()?;
        if let Some(ref ctx) = req.context_id {
            if let Some(existing) = db.get_active_lease_by_context(ctx)? {
                return Ok(existing);
            }
        }

        let binding_id = Uuid::new_v4();
        let (git_dir, base_commit, repo_binding_id) = if let Some(ref remote) = req.remote_url {
            let hash = repo_hash(remote);
            let mirror = self.data_dir.join("git/mirrors").join(&hash);
            if !mirror.exists() {
                std::fs::create_dir_all(mirror.parent().unwrap())?;
                ensure_mirror(remote, &mirror)?;
            } else {
                let _ = run_git(&mirror, &["remote", "update", "--prune"]);
            }
            let default_branch = req
                .default_branch
                .clone()
                .unwrap_or_else(|| "main".into());
            let commit = req
                .base_ref
                .clone()
                .unwrap_or_else(|| format!("origin/{}", default_branch));
            self.db()?.upsert_git_repo_binding(
                binding_id,
                req.session_id,
                "remote",
                None,
                Some(remote),
                &default_branch,
                Some(mirror.to_string_lossy().as_ref()),
            )?;
            (mirror.clone(), resolve_ref(&mirror, &commit)?, binding_id)
        } else {
            let local = req
                .local_path
                .clone()
                .context("local_path required for local git binding")?;
            if !is_git_repo(&local) {
                anyhow::bail!("{} is not a git repository", local.display());
            }
            let default_branch = req.default_branch.clone().unwrap_or_else(|| {
                git_output(&local, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|_| "main".into())
            });
            let commit = req.base_ref.clone().unwrap_or_else(|| {
                git_output(&local, &["rev-parse", "HEAD"]).unwrap_or_default()
            });
            self.db()?.upsert_git_repo_binding(
                binding_id,
                req.session_id,
                "local",
                Some(local.to_string_lossy().as_ref()),
                None,
                &default_branch,
                None,
            )?;
            (local.clone(), commit, binding_id)
        };

        let lease_id = Uuid::new_v4();
        let wt_dir = self
            .data_dir
            .join("git/worktrees")
            .join(lease_id.to_string());
        std::fs::create_dir_all(wt_dir.parent().unwrap())?;

        if req.remote_url.is_some() {
            run_git(
                &git_dir,
                &[
                    "worktree",
                    "add",
                    "-B",
                    &req.branch,
                    wt_dir.to_string_lossy().as_ref(),
                    &base_commit,
                ],
            )?;
        } else {
            // Local repo: worktree from existing repo
            run_git(
                &git_dir,
                &[
                    "worktree",
                    "add",
                    "-B",
                    &req.branch,
                    wt_dir.to_string_lossy().as_ref(),
                    &base_commit,
                ],
            )?;
        }

        let lease = GitWorktreeLease {
            id: lease_id,
            repo_binding_id,
            session_id: req.session_id,
            context_id: req.context_id.clone(),
            branch: req.branch.clone(),
            worktree_path: wt_dir,
            base_commit: base_commit.clone(),
            status: GitLeaseStatus::Active,
            created_at: Utc::now(),
            released_at: None,
        };
        self.db()?.insert_git_lease(&lease)?;
        Ok(lease)
    }

    pub fn release(&self, lease_id: Uuid, delete_branch: bool) -> Result<()> {
        let db = self.db()?;
        let Some(lease) = db.get_git_lease(lease_id)? else {
            return Ok(());
        };
        if lease.status == GitLeaseStatus::Released {
            return Ok(());
        }

        let main_repo = if let Some((_, source, local, _, _, mirror)) =
            db.get_git_repo_binding_for_session(lease.session_id)?
        {
            if source == "remote" {
                mirror.map(PathBuf::from)
            } else {
                local.map(PathBuf::from)
            }
        } else {
            None
        };

        if let Some(main) = main_repo {
            let _ = run_git(
                &main,
                &[
                    "worktree",
                    "remove",
                    "--force",
                    lease.worktree_path.to_string_lossy().as_ref(),
                ],
            );
            let _ = run_git(&main, &["worktree", "prune"]);
            if delete_branch {
                let _ = run_git(&main, &["branch", "-D", &lease.branch]);
            }
        }

        db.update_git_lease_status(lease_id, GitLeaseStatus::Released, Some(Utc::now()))?;
        Ok(())
    }

    pub fn reset(&self, lease_id: Uuid) -> Result<()> {
        let db = self.db()?;
        let Some(lease) = db.get_git_lease(lease_id)? else {
            anyhow::bail!("lease not found");
        };
        run_git(
            &lease.worktree_path,
            &["reset", "--hard", &lease.base_commit],
        )?;
        Ok(())
    }
}


fn repo_hash(url: &str) -> String {
    let mut h = Sha256::new();
    h.update(url.as_bytes());
    format!("{:x}", h.finalize())[..16].to_string()
}

fn ensure_mirror(url: &str, dest: &Path) -> Result<()> {
    let status = Command::new("git")
        .args(["clone", "--mirror", url, dest.to_string_lossy().as_ref()])
        .status()
        .context("git clone --mirror")?;
    if !status.success() {
        anyhow::bail!("git clone --mirror failed for {url}");
    }
    Ok(())
}

fn is_git_repo(path: &Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn git_output(cwd: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .context("git command")?;
    if !output.status.success() {
        anyhow::bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn resolve_ref(repo: &Path, reference: &str) -> Result<String> {
    if reference.len() == 40 && reference.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(reference.to_string());
    }
    git_output(repo, &["rev-parse", reference])
}

fn run_git(cwd: &Path, args: &[&str]) -> Result<()> {
    run_git_with_env(cwd, args, &[])
}

fn run_git_with_env(cwd: &Path, args: &[&str], env: &[(&str, &str)]) -> Result<()> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(cwd).args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let output = cmd.output().context("git")?;
    if !output.status.success() {
        anyhow::bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

pub fn sanitize_branch_token(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .chars()
        .take(40)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn sanitize_branch() {
        assert_eq!(sanitize_branch_token("hello/world!"), "hello-world-");
    }

    #[test]
    fn local_worktree_acquire() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        run_git_with_env(&repo, &["init"], &[("GIT_TEMPLATE_DIR", "")]).unwrap();
        run_git_with_env(&repo, &["config", "user.email", "t@t.com"], &[]).unwrap();
        run_git_with_env(&repo, &["config", "user.name", "t"], &[]).unwrap();
        std::fs::write(repo.join("README.md"), "hi").unwrap();
        run_git(&repo, &["add", "."]).unwrap();
        run_git(&repo, &["commit", "-m", "init"]).unwrap();

        let db_path = tmp.path().join("bunny.db");
        let mgr = GitWorkspaceManager::new(tmp.path(), db_path.to_str().unwrap());
        let session = Uuid::new_v4();
        let lease = mgr
            .acquire(AcquireGitLeaseRequest {
                session_id: session,
                context_id: Some("thread-1".into()),
                branch: "bunny/test".into(),
                base_ref: None,
                local_path: Some(repo.clone()),
                remote_url: None,
                default_branch: None,
            })
            .unwrap();
        assert!(lease.worktree_path.exists());
        mgr.release(lease.id, false).unwrap();
    }

    #[test]
    fn local_worktree_release_deletes_branch_when_requested() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        run_git_with_env(&repo, &["init"], &[("GIT_TEMPLATE_DIR", "")]).unwrap();
        run_git_with_env(&repo, &["config", "user.email", "t@t.com"], &[]).unwrap();
        run_git_with_env(&repo, &["config", "user.name", "t"], &[]).unwrap();
        std::fs::write(repo.join("README.md"), "hi").unwrap();
        run_git(&repo, &["add", "."]).unwrap();
        run_git(&repo, &["commit", "-m", "init"]).unwrap();

        let db_path = tmp.path().join("bunny.db");
        let mgr = GitWorkspaceManager::new(tmp.path(), db_path.to_str().unwrap());
        let session = Uuid::new_v4();
        let lease = mgr
            .acquire(AcquireGitLeaseRequest {
                session_id: session,
                context_id: Some("thread-cancel".into()),
                branch: "bunny/cancel-test".into(),
                base_ref: None,
                local_path: Some(repo.clone()),
                remote_url: None,
                default_branch: None,
            })
            .unwrap();
        assert!(lease.worktree_path.exists());
        assert!(
            git_output(&repo, &["show-ref", "--verify", "refs/heads/bunny/cancel-test"]).is_ok()
        );

        mgr.release(lease.id, true).unwrap();

        assert!(
            git_output(&repo, &["show-ref", "--verify", "refs/heads/bunny/cancel-test"]).is_err()
        );
    }
}
