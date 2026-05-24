mod error;
mod vault;

pub use error::SecretsError;
pub use vault::{
    env_var_name, parse_scope, SecretEntry, SecretScope, SecretsVault, VaultStatus,
};
