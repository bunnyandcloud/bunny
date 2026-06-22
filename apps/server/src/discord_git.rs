//! Git helpers for Discord thread workflows.

use crate::api::ApiError;
use crate::git_identity::{git_env_for_user, GitIdentityError};
use crate::state::AppState;
use std::path::{Path, PathBuf};
use std::process::Command;
use uuid::Uuid;

pub struct GitProbe {
    pub enabled: bool,
    pub base_branch: Option<String>,
    pub start_commit: Option<String>,
}

pub struct TerminalGitContext {
    pub project: Option<String>,
    pub branch: Option<String>,
}

pub fn terminal_git_context(cwd: &Path) -> TerminalGitContext {
    let probe = probe_git_repo(cwd);
    if !probe.enabled {
        return TerminalGitContext {
            project: None,
            branch: None,
        };
    }
    let project = git_output(cwd, &["rev-parse", "--show-toplevel"])
        .ok()
        .and_then(|p| {
            Path::new(&p)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
        });
    TerminalGitContext {
        project,
        branch: probe.base_branch,
    }
}

pub fn probe_git_repo(cwd: &Path) -> GitProbe {
    if !git_ok(cwd, &["rev-parse", "--is-inside-work-tree"]) {
        return GitProbe {
            enabled: false,
            base_branch: None,
            start_commit: None,
        };
    }
    let base_branch = git_output(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]).ok();
    let start_commit = git_output(cwd, &["rev-parse", "HEAD"]).ok();
    GitProbe {
        enabled: true,
        base_branch,
        start_commit,
    }
}

pub fn init_thread_branch(
    cwd: &Path,
    branch_name: &str,
) -> Result<(String, String), ApiError> {
    let base = git_output(cwd, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map_err(|e| ApiError::validation(&e))?;
    if !git_ok(cwd, &["checkout", "-b", branch_name]) {
        return Err(ApiError::validation(&format!(
            "git checkout -b {branch_name} failed"
        )));
    }
    let commit = git_output(cwd, &["rev-parse", "HEAD"]).unwrap_or_default();
    Ok((base, commit))
}

pub fn reset_to_commit(cwd: &Path, commit: &str) -> Result<(), ApiError> {
    if !git_ok(cwd, &["reset", "--hard", commit]) {
        return Err(ApiError::validation("git reset --hard failed"));
    }
    Ok(())
}

pub fn run_git(
    state: &AppState,
    cwd: &Path,
    args: &[&str],
    acting_user: Uuid,
) -> Result<String, ApiError> {
    let git_env = git_env_for_user(state, acting_user).map_err(git_identity_api_error)?;
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .envs(git_env)
        .output()
        .map_err(|e| ApiError::validation(&e.to_string()))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !output.status.success() {
        return Err(ApiError::validation(&format!(
            "git {} failed: {stderr}{stdout}",
            args.join(" ")
        )));
    }
    Ok(if stdout.trim().is_empty() {
        stderr
    } else {
        stdout
    })
}

fn git_identity_api_error(err: GitIdentityError) -> ApiError {
    ApiError::validation(&err.to_string())
}

fn git_ok(cwd: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn git_output(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Main repository path for a session (local binding only — not bare mirrors).
pub fn resolve_main_repo_path(state: &AppState, session_id: Uuid) -> Result<PathBuf, ApiError> {
    if let Some((_, source, local, _, _, _)) = state
        .integrations
        .lock()
        .get_git_repo_binding_for_session(session_id)
        .map_err(|e| ApiError::validation(&e.to_string()))?
    {
        if source == "remote" {
            return Err(ApiError::validation(
                "Merge automatique indisponible pour un dépôt distant — poussez la branche et ouvrez une PR sur votre forge.",
            ));
        }
        if let Some(l) = local {
            return Ok(PathBuf::from(l));
        }
    }
    Err(ApiError::validation(
        "dépôt git principal introuvable pour cette session",
    ))
}

/// Merge a thread worktree branch into the base branch on the main local repo.
pub fn merge_thread_branch_into_base(
    state: &AppState,
    main_repo: &Path,
    base_branch: &str,
    thread_branch: &str,
    acting_user: Uuid,
) -> Result<String, ApiError> {
    let ref_name = format!("refs/heads/{thread_branch}");
    if !git_ok(main_repo, &["show-ref", "--verify", &ref_name]) {
        return Err(ApiError::validation(&format!(
            "La branche `{thread_branch}` n'existe plus (déjà mergée ou supprimée ?)"
        )));
    }
    run_git(state, main_repo, &["checkout", base_branch], acting_user)?;
    let merge_out = run_git(
        state,
        main_repo,
        &["merge", "--no-edit", thread_branch],
        acting_user,
    )?;
    let mut summary = merge_out;
    if let Ok(del) = run_git(state, main_repo, &["branch", "-d", thread_branch], acting_user) {
        if !del.trim().is_empty() {
            summary.push_str("\n");
            summary.push_str(del.trim());
        }
    }
    Ok(summary.trim().to_string())
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
