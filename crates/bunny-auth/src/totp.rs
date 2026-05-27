use base64::Engine;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use anyhow::{anyhow, Result};
use data_encoding::BASE32_NOPAD;
use rand::RngCore;
use std::fs;
use std::path::Path;
use totp_rs::{Algorithm, Secret, TOTP};

const NONCE_LEN: usize = 12;
const ISSUER: &str = "Bunny";

pub fn load_encryption_key(data_dir: &str) -> Result<[u8; 32]> {
    if let Ok(raw) = std::env::var("BUNNY_MFA_ENCRYPTION_KEY") {
        return decode_key_material(&raw);
    }
    let path = Path::new(data_dir).join("mfa.key");
    if path.exists() {
        let bytes = fs::read(&path)?;
        if bytes.len() != 32 {
            return Err(anyhow!("mfa.key must be exactly 32 bytes"));
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        return Ok(key);
    }
    let mut key = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut key);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, key)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(key)
}

fn decode_key_material(raw: &str) -> Result<[u8; 32]> {
    let trimmed = raw.trim();
    if trimmed.len() == 64 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        let mut key = [0u8; 32];
        for (i, chunk) in trimmed.as_bytes().chunks(2).enumerate() {
            key[i] = u8::from_str_radix(std::str::from_utf8(chunk).unwrap(), 16)
                .map_err(|e| anyhow!(e.to_string()))?;
        }
        return Ok(key);
    }
    let decoded = BASE32_NOPAD
        .decode(trimmed.as_bytes())
        .or_else(|_| {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.decode(trimmed)
        })
        .map_err(|_| anyhow!("BUNNY_MFA_ENCRYPTION_KEY must be 32-byte hex, base64, or base32"))?;
    if decoded.len() != 32 {
        return Err(anyhow!(
            "BUNNY_MFA_ENCRYPTION_KEY must decode to exactly 32 bytes"
        ));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&decoded);
    Ok(key)
}

pub fn encrypt_secret(key: &[u8; 32], plaintext: &str) -> Result<String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| anyhow!(e.to_string()))?;
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| anyhow!(e.to_string()))?;
    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(base64::engine::general_purpose::STANDARD.encode(out))
}

pub fn decrypt_secret(key: &[u8; 32], encoded: &str) -> Result<String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| anyhow!(e.to_string()))?;
    if bytes.len() <= NONCE_LEN {
        return Err(anyhow!("invalid encrypted secret"));
    }
    let (nonce_bytes, ciphertext) = bytes.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| anyhow!(e.to_string()))?;
    let nonce = Nonce::from_slice(nonce_bytes);
    let plain = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow!(e.to_string()))?;
    String::from_utf8(plain).map_err(|e| anyhow!(e.to_string()))
}

pub fn generate_totp_secret() -> Result<String> {
    let secret = Secret::generate_secret();
    Ok(secret.to_encoded().to_string())
}

pub(crate) fn make_totp(email: &str, secret_base32: &str) -> Result<TOTP> {
    let secret = Secret::Encoded(secret_base32.to_string());
    let bytes = secret.to_bytes().map_err(|e| anyhow!(e.to_string()))?;
    TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        bytes,
        Some(ISSUER.to_string()),
        email.to_string(),
    )
    .map_err(|e| anyhow!(e.to_string()))
}

pub fn build_otpauth_uri(email: &str, secret_base32: &str) -> Result<String> {
    Ok(make_totp(email, secret_base32)?.get_url())
}

pub fn verify_totp_code(secret_base32: &str, code: &str) -> Result<bool> {
    let normalized = code.trim().replace(' ', "");
    if normalized.len() != 6 || !normalized.chars().all(|c| c.is_ascii_digit()) {
        return Ok(false);
    }
    // Email label is only used for TOTP construction; verification uses the secret bytes.
    let totp = make_totp("user", secret_base32)?;
    Ok(totp.check_current(&normalized).unwrap_or(false))
}

/// Crockford-style recovery code: `bunny-XXXX-XXXX-XXXX-XXXX` (~80 bits).
pub fn generate_recovery_code() -> String {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let s: String = bytes
        .iter()
        .map(|b| ALPHABET[(b & 0x1F) as usize] as char)
        .collect();
    format!(
        "bunny-{}-{}-{}-{}",
        &s[0..4],
        &s[4..8],
        &s[8..12],
        &s[12..16]
    )
}

pub fn normalize_recovery_code(input: &str) -> String {
    input.trim().to_uppercase().replace(' ', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totp_round_trip() {
        let secret = generate_totp_secret().unwrap();
        let totp = make_totp("test@bunny.local", &secret).unwrap();
        let code = totp.generate_current().unwrap();
        assert!(verify_totp_code(&secret, &code).unwrap());
    }

    #[test]
    fn encrypt_decrypt_secret() {
        let key = [7u8; 32];
        let enc = encrypt_secret(&key, "JBSWY3DPEHPK3PXP").unwrap();
        let dec = decrypt_secret(&key, &enc).unwrap();
        assert_eq!(dec, "JBSWY3DPEHPK3PXP");
    }
}
