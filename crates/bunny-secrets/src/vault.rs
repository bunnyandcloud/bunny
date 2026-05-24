use crate::error::SecretsError;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::{Argon2, Params, Version};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const MAGIC: &[u8; 8] = b"BUNYSEC1";
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SecretScope {
    System,
    Project,
    Session,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretEntry {
    pub name: String,
    pub scope: SecretScope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<Uuid>,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VaultPayload {
    version: u32,
    entries: Vec<SecretEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultStatus {
    Missing,
    Locked,
    Unlocked,
}

pub struct SecretsVault {
    path: PathBuf,
    payload: Option<VaultPayload>,
}

impl SecretsVault {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            payload: None,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn status(&self) -> VaultStatus {
        if !self.path.exists() {
            VaultStatus::Missing
        } else if self.payload.is_none() {
            VaultStatus::Locked
        } else {
            VaultStatus::Unlocked
        }
    }

    pub fn is_unlocked(&self) -> bool {
        self.payload.is_some()
    }

    pub fn init(&mut self, passphrase: &str) -> Result<(), SecretsError> {
        if self.path.exists() {
            return Err(SecretsError::AlreadyExists(self.path.display().to_string()));
        }
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        self.payload = Some(VaultPayload {
            version: 1,
            entries: vec![],
        });
        self.save(passphrase)?;
        Ok(())
    }

    pub fn unlock(&mut self, passphrase: &str) -> Result<(), SecretsError> {
        if !self.path.exists() {
            return Err(SecretsError::NotFound(self.path.display().to_string()));
        }
        let bytes = fs::read(&self.path)?;
        self.payload = Some(decrypt_file(&bytes, passphrase)?);
        Ok(())
    }

    pub fn lock_vault(&mut self) {
        self.payload = None;
    }

    pub fn save(&self, passphrase: &str) -> Result<(), SecretsError> {
        let payload = self
            .payload
            .as_ref()
            .ok_or(SecretsError::Locked)?;
        let bytes = encrypt_file(payload, passphrase)?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.path, bytes)?;
        Ok(())
    }

    pub fn set(&mut self, entry: SecretEntry) -> Result<(), SecretsError> {
        let payload = self.payload.as_mut().ok_or(SecretsError::Locked)?;
        if let Some(idx) = payload.entries.iter().position(|e| entry_key(e) == entry_key(&entry)) {
            payload.entries[idx] = entry;
        } else {
            payload.entries.push(entry);
        }
        Ok(())
    }

    pub fn remove(
        &mut self,
        name: &str,
        scope: SecretScope,
        session_id: Option<Uuid>,
    ) -> Result<(), SecretsError> {
        let payload = self.payload.as_mut().ok_or(SecretsError::Locked)?;
        let before = payload.entries.len();
        payload.entries.retain(|e| {
            !(e.name == name && e.scope == scope && e.session_id == session_id)
        });
        if payload.entries.len() == before {
            return Err(SecretsError::NotFoundEntry(name.into()));
        }
        Ok(())
    }

    pub fn get(
        &self,
        name: &str,
        scope: SecretScope,
        session_id: Option<Uuid>,
    ) -> Result<String, SecretsError> {
        let payload = self.payload.as_ref().ok_or(SecretsError::Locked)?;
        payload
            .entries
            .iter()
            .find(|e| e.name == name && e.scope == scope && e.session_id == session_id)
            .map(|e| e.value.clone())
            .ok_or_else(|| SecretsError::NotFoundEntry(name.into()))
    }

    pub fn list(&self) -> Result<Vec<SecretEntryMeta>, SecretsError> {
        let payload = self.payload.as_ref().ok_or(SecretsError::Locked)?;
        Ok(payload
            .entries
            .iter()
            .map(|e| SecretEntryMeta {
                name: e.name.clone(),
                scope: e.scope,
                session_id: e.session_id,
            })
            .collect())
    }

    /// Env vars injected into PTY children (allowlisted names only).
    pub fn env_for_session(&self, session_id: Uuid) -> Result<HashMap<String, String>, SecretsError> {
        let payload = self.payload.as_ref().ok_or(SecretsError::Locked)?;
        let mut out = HashMap::new();
        for e in &payload.entries {
            let include = match e.scope {
                SecretScope::System | SecretScope::Project => true,
                SecretScope::Session => e.session_id == Some(session_id),
            };
            if include {
                out.insert(env_var_name(&e.name), e.value.clone());
            }
        }
        Ok(out)
    }

    pub fn all_values(&self) -> Result<Vec<String>, SecretsError> {
        let payload = self.payload.as_ref().ok_or(SecretsError::Locked)?;
        Ok(payload.entries.iter().map(|e| e.value.clone()).collect())
    }
}

#[derive(Debug, Clone)]
pub struct SecretEntryMeta {
    pub name: String,
    pub scope: SecretScope,
    pub session_id: Option<Uuid>,
}

fn entry_key(e: &SecretEntry) -> String {
    format!(
        "{}:{:?}:{:?}",
        e.name,
        e.scope,
        e.session_id.map(|u| u.to_string())
    )
}

pub fn env_var_name(secret_name: &str) -> String {
    let upper = secret_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("BUNNY_SECRET_{upper}")
}

fn derive_key(passphrase: &str, salt: &[u8]) -> Result<[u8; 32], SecretsError> {
    let params = Params::new(19 * 1024, 2, 1, Some(32))
        .map_err(|e| SecretsError::Other(e.to_string()))?;
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; 32];
    argon2
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| SecretsError::Other(e.to_string()))?;
    Ok(key)
}

fn encrypt_file(payload: &VaultPayload, passphrase: &str) -> Result<Vec<u8>, SecretsError> {
    let mut salt = [0u8; SALT_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    let key = derive_key(passphrase, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| SecretsError::Other(e.to_string()))?;

    let mut nonce = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce);

    let plaintext = serde_json::to_vec(payload)?;
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_ref())
        .map_err(|_| SecretsError::Other("encrypt failed".into()))?;

    let mut out = Vec::with_capacity(MAGIC.len() + SALT_LEN + NONCE_LEN + ciphertext.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

fn decrypt_file(bytes: &[u8], passphrase: &str) -> Result<VaultPayload, SecretsError> {
    let min = MAGIC.len() + SALT_LEN + NONCE_LEN + 16;
    if bytes.len() < min || &bytes[..MAGIC.len()] != MAGIC {
        return Err(SecretsError::InvalidFormat);
    }
    let salt = &bytes[MAGIC.len()..MAGIC.len() + SALT_LEN];
    let nonce = &bytes[MAGIC.len() + SALT_LEN..MAGIC.len() + SALT_LEN + NONCE_LEN];
    let ciphertext = &bytes[MAGIC.len() + SALT_LEN + NONCE_LEN..];

    let key = derive_key(passphrase, salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| SecretsError::Other(e.to_string()))?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(nonce), ciphertext)
        .map_err(|_| SecretsError::DecryptFailed)?;
    let payload: VaultPayload =
        serde_json::from_slice(&plaintext).map_err(|_| SecretsError::InvalidFormat)?;
    Ok(payload)
}

pub fn parse_scope(s: &str) -> Result<SecretScope, SecretsError> {
    match s.to_lowercase().as_str() {
        "system" => Ok(SecretScope::System),
        "project" => Ok(SecretScope::Project),
        "session" => Ok(SecretScope::Session),
        _ => Err(SecretsError::Other(
            "scope must be system, project, or session".into(),
        )),
    }
}
