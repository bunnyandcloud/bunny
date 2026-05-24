use crate::db::AuthDb;
use crate::tokens::{generate_token, hash_token};
use anyhow::{anyhow, Result};
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use bunny_core::types::Role;
use chrono::{Duration, Utc};
use parking_lot::Mutex;
use rand::rngs::OsRng;
use std::sync::Arc;
use uuid::Uuid;

pub struct AuthService {
    db: Arc<Mutex<AuthDb>>,
    session_ttl_hours: i64,
}

pub struct LoginResult {
    pub user_id: Uuid,
    pub email: String,
    pub session_token: String,
    pub expires_at: chrono::DateTime<Utc>,
}

pub struct AuthContext {
    pub user_id: Uuid,
    pub session_id: Uuid,
    pub token_hash: String,
}

impl AuthService {
    pub fn new(db_path: &str, session_ttl_hours: i64) -> Result<Self> {
        Ok(Self {
            db: Arc::new(Mutex::new(AuthDb::open(db_path)?)),
            session_ttl_hours,
        })
    }

    pub fn needs_bootstrap(&self) -> Result<bool> {
        Ok(self.db.lock().user_count()? == 0)
    }

    pub fn bootstrap_owner(&self, email: &str, password: &str) -> Result<Uuid> {
        let db = self.db.lock();
        if db.user_count()? > 0 {
            return Err(anyhow!("owner already exists"));
        }
        let id = Uuid::new_v4();
        let hash = hash_password(password)?;
        db.create_user(id, email, &hash)?;
        db.insert_audit(
            Uuid::new_v4(),
            Some(id),
            "auth.bootstrap",
            "users",
            None,
        )?;
        Ok(id)
    }

    pub fn login(&self, email: &str, password: &str, device_id: Option<&str>) -> Result<LoginResult> {
        let db = self.db.lock();
        let (user_id, stored_hash) = db
            .find_user_by_email(email)?
            .ok_or_else(|| anyhow!("invalid credentials"))?;

        verify_password(password, &stored_hash)?;

        let token = generate_token();
        let token_hash = hash_token(&token);
        let session_id = Uuid::new_v4();
        let expires = Utc::now() + Duration::hours(self.session_ttl_hours);
        db.create_auth_session(session_id, user_id, &token_hash, device_id, expires)?;
        db.insert_audit(
            Uuid::new_v4(),
            Some(user_id),
            "auth.login",
            "auth_sessions",
            None,
        )?;

        let (email, _) = db.find_user_by_id(user_id)?.unwrap();

        Ok(LoginResult {
            user_id,
            email,
            session_token: token,
            expires_at: expires,
        })
    }

    pub fn logout(&self, token: &str) -> Result<()> {
        let db = self.db.lock();
        db.revoke_session(&hash_token(token))?;
        Ok(())
    }

    pub fn authenticate(&self, token: &str) -> Result<Uuid> {
        let db = self.db.lock();
        let (_, user_id) = db
            .find_session_by_token_hash(&hash_token(token))?
            .ok_or_else(|| anyhow!("invalid session"))?;
        Ok(user_id)
    }

    pub fn me(&self, user_id: Uuid) -> Result<(String, chrono::DateTime<Utc>)> {
        let db = self.db.lock();
        db.find_user_by_id(user_id)?
            .ok_or_else(|| anyhow!("user not found"))
    }

    pub fn create_stream_session(
        &self,
        owner_id: Uuid,
        project_path: &str,
        name: Option<&str>,
    ) -> Result<Uuid> {
        let id = Uuid::new_v4();
        let db = self.db.lock();
        db.create_stream_session(id, owner_id, project_path, name, "ready")?;
        db.add_session_member(id, owner_id, Role::Owner)?;
        Ok(id)
    }

    pub fn member_role(&self, session_id: Uuid, user_id: Uuid) -> Result<Option<Role>> {
        self.db.lock().get_member_role(session_id, user_id)
    }

    pub fn invite_user(
        &self,
        session_id: Uuid,
        email: &str,
        role: Role,
        inviter: Uuid,
    ) -> Result<String> {
        let db = self.db.lock();
        let inviter_role = db.get_member_role(session_id, inviter)?;
        if !matches!(inviter_role, Some(Role::Owner) | Some(Role::Admin)) {
            return Err(anyhow!("forbidden"));
        }
        let token = generate_token();
        let token_hash = hash_token(&token);
        db.insert_invitation(
            Uuid::new_v4(),
            session_id,
            email,
            role,
            &token_hash,
            Utc::now() + Duration::days(7),
        )?;
        Ok(token)
    }

    pub fn owner_id(&self) -> Result<Uuid> {
        self.db
            .lock()
            .first_user_id()?
            .ok_or_else(|| anyhow!("no owner configured"))
    }

    pub fn db(&self) -> Arc<Mutex<AuthDb>> {
        self.db.clone()
    }
}

fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow!(e.to_string()))?
        .to_string();
    Ok(hash)
}

fn verify_password(password: &str, hash: &str) -> Result<()> {
    let parsed = PasswordHash::new(hash).map_err(|e| anyhow!(e.to_string()))?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| anyhow!("invalid credentials"))?;
    Ok(())
}
