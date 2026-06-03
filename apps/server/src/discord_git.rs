//! Git helpers for Discord thread workflows.

use crate::api::ApiError;
use std::path::Path;
use std::process::Command;

pub struct GitProbe {
    pub enabled: bool,
    pub base_branch: Option<String>,
    pub start_commit: Option<String>,
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

pub fn run_git(cwd: &Path, args: &[&str]) -> Result<String, ApiError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
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

fn git_ok(cwd: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn git_output(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
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
