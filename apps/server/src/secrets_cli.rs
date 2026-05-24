use crate::state::AppState;
use anyhow::Result;
use bunny_secrets::{parse_scope, SecretEntry, SecretScope, SecretsError};
use clap::{Parser, Subcommand};

fn secrets_err(e: SecretsError) -> anyhow::Error {
    anyhow::anyhow!("{e}")
}
use std::io::{self, Write};
use uuid::Uuid;

#[derive(Parser)]
pub struct SecretsOpts {
    #[command(subcommand)]
    pub command: SecretsCommands,
}

#[derive(Subcommand)]
pub enum SecretsCommands {
    /// Create an empty encrypted secrets file
    Init {
        #[arg(long)]
        passphrase: Option<String>,
    },
    /// Unlock vault for this process (or use BUNNY_SECRETS_PASSPHRASE)
    Unlock {
        #[arg(long)]
        passphrase: Option<String>,
    },
    /// Lock vault in memory
    Lock,
    /// Set or update a secret value
    Set {
        name: String,
        #[arg(long, default_value = "system")]
        scope: String,
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long)]
        value: Option<String>,
        #[arg(long)]
        passphrase: Option<String>,
    },
    /// Print a secret value (stdout)
    Get {
        name: String,
        #[arg(long, default_value = "system")]
        scope: String,
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long)]
        passphrase: Option<String>,
    },
    /// List secret names (never values)
    List {
        #[arg(long)]
        passphrase: Option<String>,
    },
    /// Remove a secret
    Remove {
        name: String,
        #[arg(long, default_value = "system")]
        scope: String,
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long)]
        passphrase: Option<String>,
    },
    /// Show vault path and lock state
    Status,
}

pub async fn run_secrets(state: &AppState, opts: SecretsOpts) -> Result<()> {
    match opts.command {
        SecretsCommands::Init { passphrase } => run_init(state, passphrase),
        SecretsCommands::Unlock { passphrase } => run_unlock(state, passphrase),
        SecretsCommands::Lock => {
            state.secrets.lock().lock_vault();
            state.refresh_redactor_secrets();
            println!("✓ Secrets vault locked");
            Ok(())
        }
        SecretsCommands::Set {
            name,
            scope,
            session_id,
            value,
            passphrase,
        } => run_set(state, &name, &scope, session_id, value, passphrase),
        SecretsCommands::Get {
            name,
            scope,
            session_id,
            passphrase,
        } => run_get(state, &name, &scope, session_id, passphrase),
        SecretsCommands::List { passphrase } => run_list(state, passphrase),
        SecretsCommands::Remove {
            name,
            scope,
            session_id,
            passphrase,
        } => run_remove(state, &name, &scope, session_id, passphrase),
        SecretsCommands::Status => run_status(state),
    }
}

fn run_init(state: &AppState, passphrase: Option<String>) -> Result<()> {
    let pass = passphrase.unwrap_or_else(|| prompt_password("New vault passphrase: "));
    let confirm = prompt_password("Confirm passphrase: ");
    if pass != confirm {
        anyhow::bail!("passphrases do not match");
    }
    state.secrets.lock().init(&pass).map_err(secrets_err)?;
    state.secrets.lock().unlock(&pass).map_err(secrets_err)?;
    state.refresh_redactor_secrets();
    println!("✓ Created {}", state.secrets_path().display());
    Ok(())
}

fn run_unlock(state: &AppState, passphrase: Option<String>) -> Result<()> {
    let pass = passphrase.or_else(read_passphrase_from_env).unwrap_or_else(|| {
        prompt_password("Vault passphrase: ")
    });
    state.secrets.lock().unlock(&pass).map_err(secrets_err)?;
    state.refresh_redactor_secrets();
    println!("✓ Secrets vault unlocked");
    Ok(())
}

fn run_set(
    state: &AppState,
    name: &str,
    scope_str: &str,
    session_id: Option<String>,
    value: Option<String>,
    passphrase: Option<String>,
) -> Result<()> {
    let pass_hint = passphrase.clone();
    ensure_unlocked(state, passphrase)?;
    let scope = parse_scope(scope_str).map_err(secrets_err)?;
    let sid = parse_session_id(scope, session_id)?;
    let val = value.unwrap_or_else(|| prompt(&format!("Value for {name}: ")));
    let entry = SecretEntry {
        name: name.into(),
        scope,
        session_id: sid,
        value: val,
    };
    state.secrets.lock().set(entry).map_err(secrets_err)?;
    let pass = current_passphrase(pass_hint)?;
    state.secrets.lock().save(&pass).map_err(secrets_err)?;
    sync_secret_ref(state, name, scope_str, sid)?;
    state.refresh_redactor_secrets();
    println!("✓ Secret {name} saved ({scope_str})");
    Ok(())
}

fn run_get(
    state: &AppState,
    name: &str,
    scope_str: &str,
    session_id: Option<String>,
    passphrase: Option<String>,
) -> Result<()> {
    ensure_unlocked(state, passphrase)?;
    let scope = parse_scope(scope_str).map_err(secrets_err)?;
    let sid = parse_session_id(scope, session_id)?;
    let val = state.secrets.lock().get(name, scope, sid).map_err(secrets_err)?;
    print!("{val}");
    Ok(())
}

fn run_list(state: &AppState, passphrase: Option<String>) -> Result<()> {
    ensure_unlocked(state, passphrase.clone())?;
    for meta in state.secrets.lock().list().map_err(secrets_err)? {
        let sid = meta
            .session_id
            .map(|u| u.to_string())
            .unwrap_or_else(|| "-".into());
        println!("{}  scope={:?}  session={sid}", meta.name, meta.scope);
    }
    Ok(())
}

fn run_remove(
    state: &AppState,
    name: &str,
    scope_str: &str,
    session_id: Option<String>,
    passphrase: Option<String>,
) -> Result<()> {
    let pass_hint = passphrase.clone();
    ensure_unlocked(state, passphrase)?;
    let scope = parse_scope(scope_str).map_err(secrets_err)?;
    let sid = parse_session_id(scope, session_id)?;
    state.secrets.lock().remove(name, scope, sid).map_err(secrets_err)?;
    let pass = current_passphrase(pass_hint)?;
    state.secrets.lock().save(&pass).map_err(secrets_err)?;
    state
        .auth
        .db()
        .lock()
        .delete_secret_ref(name, scope_str, sid)?;
    state.refresh_redactor_secrets();
    println!("✓ Removed {name}");
    Ok(())
}

fn run_status(state: &AppState) -> Result<()> {
    println!("path: {}", state.secrets_path().display());
    println!("status: {:?}", state.secrets.lock().status());
    let refs = state.auth.db().lock().list_secret_refs(None)?;
    println!("db refs: {}", refs.len());
    Ok(())
}

fn ensure_unlocked(state: &AppState, passphrase: Option<String>) -> Result<()> {
    if state.secrets.lock().is_unlocked() {
        return Ok(());
    }
    run_unlock(state, passphrase)
}

fn current_passphrase(passphrase: Option<String>) -> Result<String> {
    Ok(passphrase
        .or_else(read_passphrase_from_env)
        .unwrap_or_else(|| prompt_password("Vault passphrase (to save): ")))
}

fn read_passphrase_from_env() -> Option<String> {
    std::env::var("BUNNY_SECRETS_PASSPHRASE").ok()
}

fn parse_session_id(scope: SecretScope, session_id: Option<String>) -> Result<Option<Uuid>> {
    match scope {
        SecretScope::Session => {
            let raw = session_id.ok_or_else(|| {
                anyhow::anyhow!("--session-id required for session-scoped secrets")
            })?;
            Ok(Some(Uuid::parse_str(&raw)?))
        }
        _ => Ok(session_id.map(|s| Uuid::parse_str(&s)).transpose()?),
    }
}

fn sync_secret_ref(
    state: &AppState,
    name: &str,
    scope: &str,
    session_id: Option<Uuid>,
) -> Result<()> {
    state.auth.db().lock().upsert_secret_ref(
        Uuid::new_v4(),
        scope,
        name,
        "file",
        name,
        session_id,
    )
}

fn prompt(label: &str) -> String {
    print!("{label}");
    let _ = io::stdout().flush();
    let mut s = String::new();
    io::stdin().read_line(&mut s).unwrap();
    s.trim().to_string()
}

fn prompt_password(label: &str) -> String {
    prompt(label)
}
