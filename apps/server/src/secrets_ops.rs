use crate::api::ApiError;
use crate::state::AppState;
use bunny_core::permissions::{role_can, Action};
use bunny_secrets::{parse_scope, SecretEntry, SecretScope, SecretsError, VaultStatus};
use serde::Serialize;
use uuid::Uuid;

pub fn ensure_secrets_access(state: &AppState, user_id: Uuid) -> Result<(), ApiError> {
    // Vault is global, but we gate it by "global" access:
    // - owner always allowed
    // - any user who is Admin in at least one session is allowed
    // - or any user granted explicit global permission
    let owner = state
        .auth
        .owner_id()
        .map_err(|_| ApiError::forbidden("permission denied"))?;
    if user_id == owner {
        return Ok(());
    }
    if let Ok(profile) = state.auth.db().lock().get_user_profile(user_id) {
        if let Some(p) = profile {
            if p.disabled_at.is_none() && p.can_manage_vault {
                return Ok(());
            }
        }
    }
    let is_admin_anywhere = state
        .auth
        .db()
        .lock()
        .has_any_session_role(user_id, bunny_core::types::Role::Admin)
        .map_err(|_| ApiError::forbidden("permission denied"))?;
    if is_admin_anywhere && role_can(bunny_core::types::Role::Admin, Action::VaultManage) {
        return Ok(());
    }
    Err(ApiError::forbidden("permission denied"))
}

fn map_secrets_err(e: SecretsError) -> ApiError {
    match e {
        SecretsError::Locked => ApiError::conflict("vault is locked"),
        SecretsError::NotFound(_) => ApiError::not_found("vault"),
        SecretsError::AlreadyExists(path) => ApiError::conflict(&format!("vault already exists at {path}")),
        SecretsError::NotFoundEntry(name) => ApiError::not_found(&format!("secret {name}")),
        SecretsError::DecryptFailed => ApiError::validation("wrong passphrase"),
        other => ApiError::validation(&other.to_string()),
    }
}

pub fn parse_session_id(scope: SecretScope, session_id: Option<String>) -> Result<Option<Uuid>, ApiError> {
    match scope {
        SecretScope::Session => {
            let raw = session_id.ok_or_else(|| {
                ApiError::validation("session_id required for session-scoped secrets")
            })?;
            Uuid::parse_str(&raw)
                .map(Some)
                .map_err(|_| ApiError::validation("invalid session_id"))
        }
        _ => session_id
            .map(|s| Uuid::parse_str(&s))
            .transpose()
            .map_err(|_| ApiError::validation("invalid session_id")),
    }
}

fn sync_secret_ref(
    state: &AppState,
    name: &str,
    scope: &str,
    session_id: Option<Uuid>,
) -> Result<(), ApiError> {
    state
        .auth
        .db()
        .lock()
        .upsert_secret_ref(Uuid::new_v4(), scope, name, "file", name, session_id)
        .map_err(|e| ApiError::validation(&e.to_string()))
}

fn delete_secret_ref(
    state: &AppState,
    name: &str,
    scope: &str,
    session_id: Option<Uuid>,
) -> Result<(), ApiError> {
    state
        .auth
        .db()
        .lock()
        .delete_secret_ref(name, scope, session_id)
        .map_err(|e| ApiError::validation(&e.to_string()))
}

fn passphrase_for_save(state: &AppState) -> Result<String, ApiError> {
    if let Some(pass) = state.secrets_passphrase.lock().clone() {
        return Ok(pass);
    }
    std::env::var("BUNNY_SECRETS_PASSPHRASE").map_err(|_| {
        ApiError::conflict("vault unlocked without stored passphrase; lock and unlock again")
    })
}

pub fn vault_status(state: &AppState) -> VaultStatusResponse {
    let status = state.secrets.lock().status();
    let ref_count = state
        .auth
        .db()
        .lock()
        .list_secret_refs(None)
        .map(|r| r.len())
        .unwrap_or(0);
    VaultStatusResponse {
        status: status_label(status),
        path: state.secrets_path().display().to_string(),
        ref_count,
    }
}

fn status_label(status: VaultStatus) -> &'static str {
    match status {
        VaultStatus::Missing => "missing",
        VaultStatus::Locked => "locked",
        VaultStatus::Unlocked => "unlocked",
    }
}

pub fn init_vault(state: &AppState, passphrase: &str, confirm: &str) -> Result<(), ApiError> {
    if passphrase != confirm {
        return Err(ApiError::validation("passphrases do not match"));
    }
  if passphrase.len() < 8 {
        return Err(ApiError::validation("passphrase must be at least 8 characters"));
    }
    state
        .secrets
        .lock()
        .init(passphrase)
        .map_err(map_secrets_err)?;
    state
        .secrets
        .lock()
        .unlock(passphrase)
        .map_err(map_secrets_err)?;
    *state.secrets_passphrase.lock() = Some(passphrase.to_string());
    on_vault_unlocked(state, None);
    Ok(())
}

pub fn unlock_vault(
    state: &AppState,
    passphrase: &str,
    session_id: Option<Uuid>,
) -> Result<(), ApiError> {
    state
        .secrets
        .lock()
        .unlock(passphrase)
        .map_err(map_secrets_err)?;
    *state.secrets_passphrase.lock() = Some(passphrase.to_string());
    on_vault_unlocked(state, session_id);
    Ok(())
}

fn on_vault_unlocked(state: &AppState, session_id: Option<Uuid>) {
    state.refresh_redactor_secrets();
    crate::terminals::refresh_secrets_in_running_shells(state, session_id);
}

/// CLI unlock/init: refresh secrets in all running shells.
pub fn on_vault_unlocked_cli(state: &AppState) {
    on_vault_unlocked(state, None);
}

pub fn lock_vault(state: &AppState) {
    state.secrets.lock().lock_vault();
    *state.secrets_passphrase.lock() = None;
    state.refresh_redactor_secrets();
}

pub fn list_secrets(state: &AppState) -> Result<Vec<SecretMetaResponse>, ApiError> {
    let metas = state.secrets.lock().list().map_err(map_secrets_err)?;
    Ok(metas
        .into_iter()
        .map(|m| {
            let env_var = bunny_secrets::env_var_name(&m.name);
            SecretMetaResponse {
                name: m.name,
                scope: scope_label(m.scope).to_string(),
                session_id: m.session_id.map(|u| u.to_string()),
                env_var,
            }
        })
        .collect())
}

fn scope_label(scope: SecretScope) -> &'static str {
    match scope {
        SecretScope::System => "system",
        SecretScope::Project => "project",
        SecretScope::Session => "session",
    }
}

pub fn upsert_secret(
    state: &AppState,
    name: &str,
    scope_str: &str,
    session_id: Option<String>,
    value: &str,
) -> Result<SecretMetaResponse, ApiError> {
    let name = normalize_secret_name(name)?;
    if value.is_empty() {
        return Err(ApiError::validation("value cannot be empty"));
    }
    let scope = parse_scope(scope_str).map_err(map_secrets_err)?;
    let sid = parse_session_id(scope, session_id)?;
    let entry = SecretEntry {
        name: name.clone(),
        scope,
        session_id: sid,
        value: value.to_string(),
    };
    state.secrets.lock().set(entry).map_err(map_secrets_err)?;
    let pass = passphrase_for_save(state)?;
    state.secrets.lock().save(&pass).map_err(map_secrets_err)?;
    sync_secret_ref(state, &name, scope_str, sid)?;
    state.refresh_redactor_secrets();
    let env_var = bunny_secrets::env_var_name(&name);
    Ok(SecretMetaResponse {
        name,
        scope: scope_str.to_string(),
        session_id: sid.map(|u| u.to_string()),
        env_var,
    })
}

pub fn remove_secret(
    state: &AppState,
    name: &str,
    scope_str: &str,
    session_id: Option<String>,
) -> Result<(), ApiError> {
    let scope = parse_scope(scope_str).map_err(map_secrets_err)?;
    let sid = parse_session_id(scope, session_id)?;
    state
        .secrets
        .lock()
        .remove(name, scope, sid)
        .map_err(map_secrets_err)?;
    let pass = passphrase_for_save(state)?;
    state.secrets.lock().save(&pass).map_err(map_secrets_err)?;
    delete_secret_ref(state, name, scope_str, sid)?;
    state.refresh_redactor_secrets();
    Ok(())
}

pub fn reveal_secret(
    state: &AppState,
    name: &str,
    scope_str: &str,
    session_id: Option<String>,
) -> Result<SecretRevealResponse, ApiError> {
    let scope = parse_scope(scope_str).map_err(map_secrets_err)?;
    let sid = parse_session_id(scope, session_id)?;
    let value = state
        .secrets
        .lock()
        .get(name, scope, sid)
        .map_err(map_secrets_err)?;
    Ok(SecretRevealResponse { value })
}

fn normalize_secret_name(name: &str) -> Result<String, ApiError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ApiError::validation("name cannot be empty"));
    }
    if trimmed.len() > 128 {
        return Err(ApiError::validation("name must be at most 128 characters"));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(ApiError::validation(
            "name may only contain letters, digits, underscore, and hyphen",
        ));
    }
    Ok(trimmed.to_string())
}

#[derive(Serialize)]
pub struct VaultStatusResponse {
    pub status: &'static str,
    pub path: String,
    pub ref_count: usize,
}

#[derive(Serialize)]
pub struct SecretMetaResponse {
    pub name: String,
    pub scope: String,
    pub session_id: Option<String>,
    pub env_var: String,
}

#[derive(Serialize)]
pub struct SecretRevealResponse {
    pub value: String,
}
