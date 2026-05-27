use crate::db::AuthDb;
use crate::tokens::{generate_token, hash_token};
use crate::totp::{
    self, build_otpauth_uri, decrypt_secret, encrypt_secret, generate_recovery_code,
    generate_totp_secret, normalize_recovery_code, verify_totp_code,
};
use anyhow::{anyhow, Result};
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use bunny_core::types::Role;
use chrono::{DateTime, Duration, Utc};
use parking_lot::Mutex;
use rand::rngs::OsRng;
use std::sync::Arc;
use uuid::Uuid;

const MFA_CHALLENGE_TTL_MINUTES: i64 = 5;
const MFA_SETUP_TTL_MINUTES: i64 = 15;
const MFA_MAX_FAILURES: i64 = 5;
const MFA_LOCK_MINUTES: i64 = 15;
const RECENT_AUTH_MINUTES: i64 = 5;
const RECOVERY_CODE_COUNT: usize = 10;

pub struct AuthService {
    db: Arc<Mutex<AuthDb>>,
    session_ttl_hours: i64,
    pub(crate) mfa_key: [u8; 32],
}

pub struct LoginResult {
    pub user_id: Uuid,
    pub email: String,
    pub session_token: String,
    pub expires_at: chrono::DateTime<Utc>,
}

pub enum LoginStep {
    Complete(LoginResult),
    MfaRequired {
        challenge_token: String,
        user_id: Uuid,
        email: String,
    },
}

#[derive(Clone)]
pub struct AuthenticatedSession {
    pub user_id: Uuid,
    pub session_id: Uuid,
    pub password_verified_at: Option<DateTime<Utc>>,
}

pub struct MfaStatus {
    pub enabled: bool,
    pub recovery_remaining: u64,
}

pub struct MfaSetupBegin {
    pub otpauth_uri: String,
    pub secret_base32: String,
}

impl AuthService {
    pub fn new(db_path: &str, data_dir: &str, session_ttl_hours: i64) -> Result<Self> {
        let mfa_key = totp::load_encryption_key(data_dir)?;
        Ok(Self {
            db: Arc::new(Mutex::new(AuthDb::open(db_path)?)),
            session_ttl_hours,
            mfa_key,
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

    pub fn login(&self, email: &str, password: &str, device_id: Option<&str>) -> Result<LoginStep> {
        let db = self.db.lock();
        let (user_id, stored_hash) = db
            .find_user_by_email(email)?
            .ok_or_else(|| anyhow!("invalid credentials"))?;
        verify_password(password, &stored_hash)?;

        let (user_email, _) = db
            .find_user_by_id(user_id)?
            .ok_or_else(|| anyhow!("invalid credentials"))?;

        if !db.user_mfa_enabled(user_id)? {
            drop(db);
            let result = self.issue_session(user_id, user_email, device_id, true)?;
            self.audit(
                Some(user_id),
                "auth.login.success",
                "auth_sessions",
                None,
            )?;
            return Ok(LoginStep::Complete(result));
        }

        db.insert_audit(
            Uuid::new_v4(),
            Some(user_id),
            "auth.login.mfa_required",
            "users",
            None,
        )?;
        drop(db);

        let challenge_token = generate_token();
        let challenge_id = Uuid::new_v4();
        let expires = Utc::now() + Duration::minutes(MFA_CHALLENGE_TTL_MINUTES);
        let db = self.db.lock();
        db.insert_mfa_challenge(
            challenge_id,
            user_id,
            &hash_token(&challenge_token),
            expires,
        )?;
        db.insert_audit(
            Uuid::new_v4(),
            Some(user_id),
            "auth.mfa.challenge_created",
            "mfa_challenges",
            None,
        )?;

        Ok(LoginStep::MfaRequired {
            challenge_token,
            user_id,
            email: user_email,
        })
    }

    pub fn verify_mfa_login(
        &self,
        challenge_token: &str,
        code: &str,
        device_id: Option<&str>,
    ) -> Result<LoginResult> {
        let token_hash = hash_token(challenge_token);
        let db = self.db.lock();
        let challenge = db
            .get_mfa_challenge_by_token_hash(&token_hash)?
            .ok_or_else(|| anyhow!("invalid or expired challenge"))?;

        if Utc::now() > challenge.expires_at {
            db.delete_mfa_challenge(challenge.id)?;
            return Err(anyhow!("invalid or expired challenge"));
        }
        if let Some(locked) = challenge.locked_until {
            if Utc::now() < locked {
                return Err(anyhow!("too many attempts; try again later"));
            }
        }

        let user_id = challenge.user_id;
        let used_recovery = self.verify_mfa_code_for_user(&db, user_id, code)?;

        if !used_recovery {
            // TOTP path already validated
        } else {
            db.insert_audit(
                Uuid::new_v4(),
                Some(user_id),
                "auth.mfa.recovery_code_used",
                "mfa_recovery_codes",
                None,
            )?;
        }

        let (email, _) = db
            .find_user_by_id(user_id)?
            .ok_or_else(|| anyhow!("user not found"))?;
        db.delete_mfa_challenge(challenge.id)?;
        drop(db);

        let result = self.issue_session(user_id, email, device_id, true)?;
        self.audit(
            Some(user_id),
            "auth.login.success",
            "auth_sessions",
            None,
        )?;
        Ok(result)
    }

    fn verify_mfa_code_for_user(
        &self,
        db: &AuthDb,
        user_id: Uuid,
        code: &str,
    ) -> Result<bool> {
        let secret_enc = db
            .get_user_totp_secret_enc(user_id)?
            .ok_or_else(|| anyhow!("mfa not configured"))?;
        let secret = decrypt_secret(&self.mfa_key, &secret_enc)?;

        if verify_totp_code(&secret, code)? {
            return Ok(false);
        }

        let normalized = normalize_recovery_code(code);
        let code_hash = hash_token(&normalized);
        if db.consume_recovery_code(user_id, &code_hash)? {
            return Ok(true);
        }

        Err(anyhow!("invalid code"))
    }

    fn record_mfa_failure(&self, challenge_id: Uuid, failed: i64) -> Result<()> {
        let db = self.db.lock();
        let locked_until = if failed >= MFA_MAX_FAILURES {
            Some(Utc::now() + Duration::minutes(MFA_LOCK_MINUTES))
        } else {
            None
        };
        db.increment_mfa_challenge_failure(challenge_id, failed, locked_until)?;
        if locked_until.is_some() {
            db.insert_audit(
                Uuid::new_v4(),
                None,
                "auth.mfa.challenge_locked",
                "mfa_challenges",
                None,
            )?;
        }
        db.insert_audit(
            Uuid::new_v4(),
            None,
            "auth.mfa.failed",
            "mfa_challenges",
            None,
        )?;
        Ok(())
    }

    pub fn verify_mfa_login_with_failure(
        &self,
        challenge_token: &str,
        code: &str,
        device_id: Option<&str>,
    ) -> Result<LoginResult> {
        match self.verify_mfa_login(challenge_token, code, device_id) {
            Ok(r) => Ok(r),
            Err(e) => {
                let token_hash = hash_token(challenge_token);
                // Avoid any chance of nested locking on the DB mutex.
                let failure = {
                    let db = self.db.lock();
                    db.get_mfa_challenge_by_token_hash(&token_hash)
                        .ok()
                        .flatten()
                        .map(|ch| (ch.id, ch.failed_attempts + 1))
                };
                if let Some((challenge_id, failed)) = failure {
                    let _ = self.record_mfa_failure(challenge_id, failed);
                }
                Err(e)
            }
        }
    }

    fn issue_session(
        &self,
        user_id: Uuid,
        email: String,
        device_id: Option<&str>,
        password_verified: bool,
    ) -> Result<LoginResult> {
        let token = generate_token();
        let token_hash = hash_token(&token);
        let session_id = Uuid::new_v4();
        let expires = Utc::now() + Duration::hours(self.session_ttl_hours);
        let pva = if password_verified {
            Some(Utc::now())
        } else {
            None
        };
        self.db.lock().create_auth_session(
            session_id,
            user_id,
            &token_hash,
            device_id,
            expires,
            pva,
        )?;
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
        Ok(self.authenticate_session(token)?.user_id)
    }

    pub fn authenticate_session(&self, token: &str) -> Result<AuthenticatedSession> {
        let db = self.db.lock();
        let (session_id, user_id, password_verified_at) = db
            .find_session_by_token_hash(&hash_token(token))?
            .ok_or_else(|| anyhow!("invalid session"))?;
        Ok(AuthenticatedSession {
            user_id,
            session_id,
            password_verified_at,
        })
    }

    pub fn assert_recent_auth(
        &self,
        session: &AuthenticatedSession,
        password: Option<&str>,
    ) -> Result<()> {
        if let Some(pw) = password {
            self.verify_user_password(session.user_id, pw)?;
            self.db
                .lock()
                .touch_password_verified(session.session_id)?;
            return Ok(());
        }
        if let Some(pva) = session.password_verified_at {
            if Utc::now() - pva < Duration::minutes(RECENT_AUTH_MINUTES) {
                return Ok(());
            }
        }
        Err(anyhow!("recent authentication required"))
    }

    pub fn verify_user_password(&self, user_id: Uuid, password: &str) -> Result<()> {
        let db = self.db.lock();
        let email = db
            .find_user_by_id(user_id)?
            .ok_or_else(|| anyhow!("user not found"))?
            .0;
        let (_, stored_hash) = db
            .find_user_by_email(&email)?
            .ok_or_else(|| anyhow!("invalid credentials"))?;
        verify_password(password, &stored_hash)
    }

    pub fn mfa_status(&self, user_id: Uuid) -> Result<MfaStatus> {
        let db = self.db.lock();
        Ok(MfaStatus {
            enabled: db.user_mfa_enabled(user_id)?,
            recovery_remaining: db.count_unused_recovery_codes(user_id)?,
        })
    }

    pub fn mfa_setup_begin(&self, user_id: Uuid) -> Result<MfaSetupBegin> {
        let db = self.db.lock();
        if db.user_mfa_enabled(user_id)? {
            return Err(anyhow!("mfa already enabled"));
        }
        db.delete_expired_mfa_pending()?;
        db.delete_mfa_pending_for_user(user_id)?;

        let secret_base32 = generate_totp_secret()?;
        let secret_enc = encrypt_secret(&self.mfa_key, &secret_base32)?;
        let email = db
            .find_user_by_id(user_id)?
            .ok_or_else(|| anyhow!("user not found"))?
            .0;
        let pending_id = Uuid::new_v4();
        let expires = Utc::now() + Duration::minutes(MFA_SETUP_TTL_MINUTES);
        db.insert_mfa_pending(pending_id, user_id, &secret_enc, expires)?;
        db.insert_audit(
            Uuid::new_v4(),
            Some(user_id),
            "auth.mfa.setup_started",
            "mfa_pending_setups",
            None,
        )?;
        let otpauth_uri = build_otpauth_uri(&email, &secret_base32)?;
        Ok(MfaSetupBegin {
            otpauth_uri,
            secret_base32,
        })
    }

    pub fn mfa_setup_confirm(&self, user_id: Uuid, code: &str) -> Result<Vec<String>> {
        let db = self.db.lock();
        let secret_enc = db
            .get_mfa_pending_secret_enc(user_id)?
            .ok_or_else(|| anyhow!("no pending mfa setup or setup expired"))?;
        let secret = decrypt_secret(&self.mfa_key, &secret_enc)?;
        if !verify_totp_code(&secret, code)? {
            return Err(anyhow!("invalid code"));
        }
        db.set_user_totp_active(user_id, &secret_enc)?;
        db.delete_mfa_pending_for_user(user_id)?;
        db.delete_recovery_codes_for_user(user_id)?;

        let mut codes = Vec::with_capacity(RECOVERY_CODE_COUNT);
        for _ in 0..RECOVERY_CODE_COUNT {
            let plain = generate_recovery_code();
            db.insert_recovery_code(Uuid::new_v4(), user_id, &hash_token(&plain))?;
            codes.push(plain);
        }
        db.insert_audit(
            Uuid::new_v4(),
            Some(user_id),
            "auth.mfa.enabled",
            "users",
            None,
        )?;
        Ok(codes)
    }

    pub fn mfa_disable(&self, user_id: Uuid, code: &str) -> Result<()> {
        let db = self.db.lock();
        if !db.user_mfa_enabled(user_id)? {
            return Err(anyhow!("mfa not enabled"));
        }
        self.verify_mfa_code_for_user(&db, user_id, code)?;
        drop(db);
        let db = self.db.lock();
        db.clear_user_totp(user_id)?;
        db.insert_audit(
            Uuid::new_v4(),
            Some(user_id),
            "auth.mfa.disabled",
            "users",
            None,
        )?;
        Ok(())
    }

    pub fn mfa_regenerate_recovery(&self, user_id: Uuid, code: &str) -> Result<Vec<String>> {
        let db = self.db.lock();
        if !db.user_mfa_enabled(user_id)? {
            return Err(anyhow!("mfa not enabled"));
        }
        self.verify_mfa_code_for_user(&db, user_id, code)?;
        db.delete_recovery_codes_for_user(user_id)?;
        let mut codes = Vec::with_capacity(RECOVERY_CODE_COUNT);
        for _ in 0..RECOVERY_CODE_COUNT {
            let plain = generate_recovery_code();
            db.insert_recovery_code(Uuid::new_v4(), user_id, &hash_token(&plain))?;
            codes.push(plain);
        }
        db.insert_audit(
            Uuid::new_v4(),
            Some(user_id),
            "auth.mfa.recovery_regenerated",
            "mfa_recovery_codes",
            None,
        )?;
        Ok(codes)
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

    fn audit(
        &self,
        actor: Option<Uuid>,
        action: &str,
        resource: &str,
        metadata: Option<&str>,
    ) -> Result<()> {
        self.db.lock().insert_audit(
            Uuid::new_v4(),
            actor,
            action,
            resource,
            metadata,
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_auth() -> (AuthService, String) {
        let dir = std::env::temp_dir().join(format!("bunny-mfa-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let db = dir.join("bunny.db");
        let svc = AuthService::new(db.to_str().unwrap(), dir.to_str().unwrap(), 2).unwrap();
        (svc, dir.to_string_lossy().to_string())
    }

    #[test]
    fn login_with_mfa_requires_second_step() {
        let (auth, _dir) = temp_auth();
        let id = auth.bootstrap_owner("a@b.com", "pw").unwrap();
        auth.mfa_setup_begin(id).unwrap();
        let db_guard = auth.db();
        let db = db_guard.lock();
        let enc = db.get_mfa_pending_secret_enc(id).unwrap().unwrap();
        let secret = totp::decrypt_secret(&auth.mfa_key, &enc).unwrap();
        drop(db);
        let totp = totp::make_totp("a@b.com", &secret).unwrap();
        let code = totp.generate_current().unwrap();
        auth.mfa_setup_confirm(id, &code).unwrap();

        let step = auth.login("a@b.com", "pw", None).unwrap();
        assert!(matches!(step, LoginStep::MfaRequired { .. }));
        assert!(auth.authenticate("bogus").is_err());
    }
}
