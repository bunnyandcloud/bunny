# Security

## Authentication

- Owner bootstrap via `bunny configure` (first run only).
- Email + password with Argon2id hashing.
- Session tokens stored as SHA-256 hashes in SQLite.
- Web: `HttpOnly` cookie `bunny_session`.
- Mobile: Bearer token in secure storage (Keychain / Keystore).

**URLs are never credentials.** `/s/:sessionId` requires a valid session.

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

## Relay mode

Outbound WSS from agent. Relay must not receive raw secrets. Auth remains on the bunny instance.
