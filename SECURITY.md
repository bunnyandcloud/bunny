# Security Policy

## Reporting

Report vulnerabilities to security@bunny.dev (placeholder).

## Principles

- Authentication is always required for session access
- URLs are pointers, never credentials
- CDP and VNC ports bind to 127.0.0.1 only
- Secrets are redacted before storage and streaming
- PTY children receive allowlisted environment only

## Sensitive data

Never commit: `.env`, `secrets.enc`, `bunny.db`, session cookies.
