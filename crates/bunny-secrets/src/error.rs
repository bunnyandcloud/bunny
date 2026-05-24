use thiserror::Error;

#[derive(Debug, Error)]
pub enum SecretsError {
    #[error("secrets vault not found at {0}")]
    NotFound(String),
    #[error("secrets vault is locked; run `bunny secrets unlock` or set BUNNY_SECRETS_PASSPHRASE")]
    Locked,
    #[error("secrets vault already exists at {0}")]
    AlreadyExists(String),
    #[error("invalid secrets file format")]
    InvalidFormat,
    #[error("decryption failed (wrong passphrase?)")]
    DecryptFailed,
    #[error("secret not found: {0}")]
    NotFoundEntry(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Other(String),
}
