//! Per-user git identity: terminal actor tracking, disk cache, and wrapper script.

use crate::state::AppState;
use anyhow::Result;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedGitProfile {
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone)]
struct TerminalGitActor {
    user_id: Uuid,
    pinned: bool,
    #[allow(dead_code)]
    updated_at: Instant,
}

pub struct GitIdentityService {
    data_dir: PathBuf,
    actors: RwLock<HashMap<Uuid, TerminalGitActor>>,
}

impl GitIdentityService {
    pub fn new(data_dir: impl Into<PathBuf>) -> Result<Self> {
        let data_dir = data_dir.into();
        let svc = Self {
            data_dir: data_dir.clone(),
            actors: RwLock::new(HashMap::new()),
        };
        svc.ensure_layout()?;
        svc.ensure_wrapper()?;
        Ok(svc)
    }

    pub fn bunny_bin_dir(&self) -> PathBuf {
        self.data_dir.join("bin")
    }

    pub fn path_with_bunny_bin(&self, home: &str) -> String {
        format!(
            "{}:{home}/.local/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
            self.bunny_bin_dir().display()
        )
    }

    pub fn terminal_session_env(&self, terminal_id: Uuid, home: &str) -> HashMap<String, String> {
        let mut env = HashMap::new();
        env.insert(
            "BUNNY_TERMINAL_ID".into(),
            terminal_id.to_string(),
        );
        env.insert("PATH".into(), self.path_with_bunny_bin(home));
        env
    }

    pub fn on_attach(&self, terminal_id: Uuid, user_id: Uuid, can_write: bool) {
        if !can_write {
            return;
        }
        let mut actors = self.actors.write();
        let replace = match actors.get(&terminal_id) {
            None => true,
            Some(a) => !a.pinned,
        };
        if replace {
            actors.insert(
                terminal_id,
                TerminalGitActor {
                    user_id,
                    pinned: false,
                    updated_at: Instant::now(),
                },
            );
            let _ = self.write_terminal_actor_file(terminal_id, user_id);
        }
    }

    pub fn note_input(&self, terminal_id: Uuid, user_id: Uuid) {
        let mut actors = self.actors.write();
        let pinned = actors.get(&terminal_id).map(|a| a.pinned).unwrap_or(false);
        let current = actors.get(&terminal_id).map(|a| a.user_id);
        let update = match current {
            None => true,
            Some(cur) if !pinned || cur == user_id => true,
            _ => false,
        };
        if update {
            actors.insert(
                terminal_id,
                TerminalGitActor {
                    user_id,
                    pinned,
                    updated_at: Instant::now(),
                },
            );
            let _ = self.write_terminal_actor_file(terminal_id, user_id);
        }
    }

    pub fn pin_user(&self, terminal_id: Uuid, user_id: Uuid) {
        self.actors.write().insert(
            terminal_id,
            TerminalGitActor {
                user_id,
                pinned: true,
                updated_at: Instant::now(),
            },
        );
        let _ = self.write_terminal_actor_file(terminal_id, user_id);
    }

    /// Set the active git actor for a terminal (e.g. Discord `/bunny run`).
    pub fn set_actor(&self, terminal_id: Uuid, user_id: Uuid, pinned: bool) {
        self.actors.write().insert(
            terminal_id,
            TerminalGitActor {
                user_id,
                pinned,
                updated_at: Instant::now(),
            },
        );
        let _ = self.write_terminal_actor_file(terminal_id, user_id);
    }

    pub fn whoami_message(&self, state: &AppState, terminal_id: Uuid, user_id: Uuid) -> String {
        let actor = self
            .actors
            .read()
            .get(&terminal_id)
            .map(|a| a.user_id)
            .unwrap_or(user_id);
        match git_env_for_user(state, actor) {
            Ok(env) => {
                let name = env.get("GIT_AUTHOR_NAME").map(String::as_str).unwrap_or("?");
                let email = env.get("GIT_AUTHOR_EMAIL").map(String::as_str).unwrap_or("?");
                format!("bunny git: active identity — {name} <{email}> (user {actor})\r\n")
            }
            Err(e) => format!("bunny git: {e}\r\n"),
        }
    }

    pub fn sync_profile_cache(&self, user_id: Uuid, name: &str, email: &str) -> Result<()> {
        self.ensure_layout()?;
        let profile = CachedGitProfile {
            name: name.to_string(),
            email: email.to_string(),
        };
        let path = self.profile_cache_path(user_id);
        let json = serde_json::to_string_pretty(&profile)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn clear_profile_cache(&self, user_id: Uuid) -> Result<()> {
        let path = self.profile_cache_path(user_id);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    /// Write disk caches for every user that already has a git profile in SQLite.
    pub fn backfill_profile_caches(&self, auth: &bunny_auth::AuthService) -> Result<()> {
        let users = auth.db().lock().list_users()?;
        for user in users {
            if git_profile_configured(user.git_name.as_deref(), user.git_email.as_deref()) {
                let name = user.git_name.as_deref().unwrap_or_default();
                let email = user.git_email.as_deref().unwrap_or_default();
                let _ = self.sync_profile_cache(user.id, name, email);
            }
        }
        Ok(())
    }

    fn ensure_layout(&self) -> Result<()> {
        std::fs::create_dir_all(self.bunny_bin_dir())?;
        std::fs::create_dir_all(self.data_dir.join("git-identity"))?;
        std::fs::create_dir_all(self.data_dir.join("git-identity/terminals"))?;
        Ok(())
    }

    fn profile_cache_path(&self, user_id: Uuid) -> PathBuf {
        self.data_dir
            .join("git-identity")
            .join(format!("{user_id}.json"))
    }

    fn terminal_actor_path(&self, terminal_id: Uuid) -> PathBuf {
        self.data_dir
            .join("git-identity/terminals")
            .join(terminal_id.to_string())
    }

    fn write_terminal_actor_file(&self, terminal_id: Uuid, user_id: Uuid) -> Result<()> {
        self.ensure_layout()?;
        std::fs::write(self.terminal_actor_path(terminal_id), user_id.to_string())?;
        Ok(())
    }

    fn ensure_wrapper(&self) -> Result<()> {
        self.ensure_layout()?;
        let real_git = find_real_git();
        let script = format!(
            r#"#!/usr/bin/env bash
set -euo pipefail
DATA_DIR="{data_dir}"
REAL_GIT="{real_git}"
TERM_ID="${{BUNNY_TERMINAL_ID:-}}"

if [[ -z "$TERM_ID" ]]; then
  exec "$REAL_GIT" "$@"
fi

ACTOR_FILE="$DATA_DIR/git-identity/terminals/$TERM_ID"
if [[ ! -f "$ACTOR_FILE" ]]; then
  echo "bunny: no active git user for this shell." >&2
  echo "Type in the terminal or run: bunny git use-me" >&2
  echo "Configure git name/email in Bunny account settings (Web UI)." >&2
  exit 1
fi

USER_ID="$(tr -d '[:space:]' < "$ACTOR_FILE")"
PROFILE="$DATA_DIR/git-identity/$USER_ID.json"
if [[ ! -f "$PROFILE" ]]; then
  echo "bunny: git identity not configured for your Bunny account." >&2
  echo "Set git name and email in Bunny account settings (Web UI home page)." >&2
  exit 1
fi

NAME="$(sed -n 's/.*"name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$PROFILE" | head -1)"
EMAIL="$(sed -n 's/.*"email"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$PROFILE" | head -1)"
if [[ -z "$NAME" || -z "$EMAIL" ]]; then
  echo "bunny: incomplete git profile (name and email required)." >&2
  echo "Update your Bunny account settings (Web UI home page)." >&2
  exit 1
fi

export GIT_AUTHOR_NAME="$NAME"
export GIT_AUTHOR_EMAIL="$EMAIL"
export GIT_COMMITTER_NAME="$NAME"
export GIT_COMMITTER_EMAIL="$EMAIL"
exec "$REAL_GIT" "$@"
"#,
            data_dir = self.data_dir.display(),
            real_git = real_git.display(),
        );
        let path = self.bunny_bin_dir().join("git");
        std::fs::write(&path, script)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
        }
        Ok(())
    }
}

fn find_real_git() -> PathBuf {
    for candidate in ["/usr/bin/git", "/bin/git", "/usr/local/bin/git"] {
        if Path::new(candidate).is_file() {
            return PathBuf::from(candidate);
        }
    }
    PathBuf::from("git")
}

#[derive(Debug)]
pub enum GitIdentityError {
    NotConfigured,
    Message(String),
}

impl std::fmt::Display for GitIdentityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured => write!(
                f,
                "git identity not configured — set name and email in Bunny account settings"
            ),
            Self::Message(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for GitIdentityError {}

pub fn git_profile_configured(name: Option<&str>, email: Option<&str>) -> bool {
    name.map(|s| !s.trim().is_empty()).unwrap_or(false)
        && email.map(|s| s.contains('@') && !s.trim().is_empty()).unwrap_or(false)
}

pub fn git_env_for_user(
    state: &AppState,
    user_id: Uuid,
) -> Result<HashMap<String, String>, GitIdentityError> {
    let profile = state
        .auth
        .get_user_git_profile(user_id)
        .map_err(|e| GitIdentityError::Message(e.to_string()))?;
    let name = profile
        .git_name
        .filter(|s| !s.trim().is_empty())
        .ok_or(GitIdentityError::NotConfigured)?;
    let email = profile
        .git_email
        .filter(|s| s.contains('@') && !s.trim().is_empty())
        .ok_or(GitIdentityError::NotConfigured)?;
    let mut env = HashMap::new();
    env.insert("GIT_AUTHOR_NAME".into(), name.clone());
    env.insert("GIT_AUTHOR_EMAIL".into(), email.clone());
    env.insert("GIT_COMMITTER_NAME".into(), name);
    env.insert("GIT_COMMITTER_EMAIL".into(), email);
    Ok(env)
}

pub fn apply_git_env(cmd: &mut std::process::Command, env: &HashMap<String, String>) {
    for (k, v) in env {
        cmd.env(k, v);
    }
}

pub fn apply_bunny_path(cmd: &mut std::process::Command, state: &AppState, home: &str) {
    cmd.env("PATH", state.git_identity.path_with_bunny_bin(home));
}
