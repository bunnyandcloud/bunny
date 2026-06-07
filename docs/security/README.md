# Security

## Authentication

- Owner bootstrap via `bunny configure` (first run only).
- Email + password with Argon2id hashing.
- Optional **TOTP MFA** (RFC 6238, 6 digits / 30 s) — compatible with Google Authenticator, Microsoft Authenticator, GitHub Mobile, and other standard TOTP apps.
- Session tokens stored as SHA-256 hashes in SQLite.
- Web: `HttpOnly` cookie `bunny_session`.
- Mobile: `session_token` in login JSON, sent as `Authorization: Bearer …` (Keychain / Keystore).

**URLs are never credentials.** `/s/:sessionId` requires a valid session.

### MFA login flow

1. `POST /api/v1/auth/login` with email + password.
2. If MFA is enabled: response includes `mfa_required: true` and `mfa_challenge_token` (plus cookie `bunny_mfa_challenge` for the web UI). **No full session is created yet.**
3. `POST /api/v1/auth/mfa/verify` with TOTP or recovery code → `bunny_session` cookie and `session_token` in JSON.

Rate limiting: 5 failed attempts per challenge, then 15-minute lock.

### Enabling MFA

From the web UI: **Security** (`/security`) while signed in. Scan the QR code or enter the manual secret once. Save the **recovery codes** immediately — they are shown only once.

Sensitive actions (setup, disable, regenerate recovery codes) require **recent authentication**: password re-entry or a session where the password was verified within the last 5 minutes.

### Recovery codes

Ten one-time codes (format `bunny-XXXX-XXXX-XXXX-XXXX`, ~80 bits entropy). Stored hashed in SQLite. Use instead of TOTP if you lose your phone.

### TOTP secret storage

TOTP secrets are encrypted at rest (AES-256-GCM). Encryption key:

- Prefer `BUNNY_MFA_ENCRYPTION_KEY` (32 bytes, hex or base64) from Docker secrets, systemd, K8s, etc.
- Fallback: `{data_dir}/mfa.key` (auto-generated, mode `0600`).

**Threat model:** if an attacker reads both the database and the encryption key, TOTP secrets can be recovered. This is acceptable for self-hosted deployments but means server compromise can bypass MFA — rotate MFA and invalidate sessions after incident response.

### MFA audit events

Logged to `audit_logs`: `auth.login.mfa_required`, `auth.login.success`, `auth.mfa.challenge_created`, `auth.mfa.challenge_locked`, `auth.mfa.failed`, `auth.mfa.enabled`, `auth.mfa.disabled`, `auth.mfa.recovery_code_used`, `auth.mfa.recovery_regenerated`, `auth.mfa.setup_started`.

## Secrets

Three scopes: `system`, `project`, `session`.

- PTY children receive **allowlisted env only** — never full `process.env`.
- Injected secrets use env names `BUNNY_SECRET_<NAME>` (uppercase, sanitized).
- Database stores **secret references** (`secret_refs` table), not values.
- Values live in an encrypted file: `~/.config/bunny/secrets.enc` (Argon2id + AES-256-GCM).

### CLI

```bash
bunny secrets init                    # create vault (interactive passphrase)
bunny secrets unlock                  # unlock in-memory for this shell
export BUNNY_SECRETS_PASSPHRASE=...   # auto-unlock on `bunny start`
bunny secrets set API_KEY --scope system --value 'sk-...'
bunny secrets set DB_URL --scope session --session-id <uuid> --value 'postgres://...'
bunny secrets list
bunny secrets get API_KEY
bunny secrets remove API_KEY
bunny secrets status
```

Unlock the vault before starting terminals that need injected secrets, or set `BUNNY_SECRETS_PASSPHRASE` when running `bunny start`.

## Mobile credentials

- SSH passwords and private keys are stored as **AES-256-GCM envelopes** in secure storage (master key in Keychain/Keystore).
- Supports password auth and PEM private keys (optional passphrase for encrypted keys).

## Redaction

All timeline, audit, and streamed events pass through `bunny_core::redaction::Redactor`:

- Known secret values (from unlocked vault)
- Bearer tokens, JWT, AWS keys, PEM headers
- URL query strings stripped by default

## Network collection

Default: metadata only (method, redacted URL, status, timing).

Headers and bodies require explicit opt-in with redaction.

## Browser / CDP / VNC

- Chromium CDP binds `127.0.0.1` only.
- VNC and noVNC proxied through authenticated API.
- CDP sidecar runs locally, outputs redacted JSON lines.
- **Watch read-only (v1):** noVNC UI lock (`bunny_lock`) hides settings and forces client-side `view_only`; this does **not** block RFB input at the WebSocket proxy. See [novnc-readonly-server-enforcement](../improvements/novnc-readonly-server-enforcement.md).

## Discord

- Bridge authenticates with `Authorization: Bearer` (hashed token in agent config).
- Shell/agent commands require a Discord account linked to a Bunny user (home page OAuth) with Editor+ on the session.
- Tool approvals and risky shell commands require **DiscordApprove** (Admin+).
- All actions are written to `discord_audit_log`.
- Watch read-only links: see [novnc-readonly-server-enforcement](../improvements/novnc-readonly-server-enforcement.md).

## Relay mode

Outbound WSS from agent. Relay must not receive raw secrets. Auth remains on the bunny instance.
