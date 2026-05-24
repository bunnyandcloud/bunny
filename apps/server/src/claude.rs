//! Claude Code install + OAuth helpers for remote Bunny hosts (VM / Docker).

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

use crate::state::AppState;
use crate::terminals::{default_shell_cwd, persist_terminal};
use bunny_pty::tmux;

pub const INSTALL_SCRIPT_URL: &str = "https://claude.ai/install.sh";

#[derive(Debug, Clone, serde::Serialize)]
pub struct ClaudeStatus {
    pub installed: bool,
    pub authenticated: bool,
    pub version: Option<String>,
    pub binary: Option<String>,
    pub install: InstallStatus,
    pub auth: AuthStatus,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct InstallStatus {
    pub state: String,
    pub message: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AuthStatus {
    pub active: bool,
    pub phase: String,
    pub session_id: Option<String>,
    pub terminal_id: Option<String>,
    pub oauth_url: Option<String>,
    /// Short same-host URL for the remote browser (avoids truncating a long OAuth URL).
    pub oauth_browser_url: Option<String>,
    pub code_submitted: bool,
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct InstallState {
    pub state: String,
    pub message: String,
    pub error: Option<String>,
}

impl Default for InstallState {
    fn default() -> Self {
        Self {
            state: "idle".into(),
            message: "Claude Code is not installed.".into(),
            error: None,
        }
    }
}

#[derive(Debug, Default)]
pub struct AuthFlow {
    pub session_id: Option<Uuid>,
    pub terminal_id: Option<Uuid>,
    pub oauth_url: Option<String>,
    pub oauth_redirect_token: Option<Uuid>,
    pub oauth_browser_url: Option<String>,
    pub phase: String,
    pub error: Option<String>,
    pub code_submitted: bool,
}

fn store_oauth_url(auth: &mut AuthFlow, url: String, redirect_port: u16) {
    if auth.oauth_url.as_deref() == Some(url.as_str()) {
        return;
    }
    let token = Uuid::new_v4();
    auth.oauth_url = Some(url);
    auth.oauth_redirect_token = Some(token);
    auth.oauth_browser_url = Some(oauth_browser_url(redirect_port, token));
    auth.phase = "waiting_code".into();
}

pub fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/root"))
}

pub fn local_bin() -> PathBuf {
    home_dir().join(".local/bin")
}

pub fn credentials_path() -> PathBuf {
    home_dir().join(".claude/.credentials.json")
}

pub fn resolve_binary() -> Option<PathBuf> {
    let direct = local_bin().join("claude");
    if direct.is_file() {
        return Some(direct);
    }
    for dir in std::env::var("PATH").unwrap_or_default().split(':') {
        let p = PathBuf::from(dir).join("claude");
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

pub fn is_installed() -> bool {
    resolve_binary().is_some()
}

pub fn is_authenticated() -> bool {
    credentials_path().is_file()
}

pub fn version_string() -> Option<String> {
    let bin = resolve_binary()?;
    let out = Command::new(&bin)
        .arg("--version")
        .output()
        .ok()?;
    if out.status.success() {
        if let Some(s) = non_empty(&String::from_utf8_lossy(&out.stdout)) {
            return Some(s);
        }
    }
    non_empty(String::from_utf8_lossy(&out.stderr).trim())
}

fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

pub fn install_script_path(data_dir: &str) -> PathBuf {
    Path::new(data_dir).join("vendor/claude-install.sh")
}

pub async fn ensure_install_script(data_dir: &str) -> Result<PathBuf> {
    let path = install_script_path(data_dir);
    if path.is_file() {
        return Ok(path);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()?;
    let body = client
        .get(INSTALL_SCRIPT_URL)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    std::fs::write(&path, &body)?;
    Ok(path)
}

pub fn ensure_login_profile() -> Result<()> {
    let home = home_dir();
    let profile_dir = home.join(".config/bunny");
    std::fs::create_dir_all(&profile_dir)?;
    let env_sh = profile_dir.join("env.sh");
    let content = r#"# Added by bunny — Claude Code and other user-local tools
export PATH="$HOME/.local/bin:$PATH"
"#;
    std::fs::write(&env_sh, content)?;

    let marker = "# bunny env";
    for rc in [".bashrc", ".profile"] {
        let rc_path = home.join(rc);
        if !rc_path.exists() {
            continue;
        }
        let existing = std::fs::read_to_string(&rc_path).unwrap_or_default();
        if existing.contains(marker) {
            continue;
        }
        let append = format!(
            "\n{marker}\n[ -f \"$HOME/.config/bunny/env.sh\" ] && . \"$HOME/.config/bunny/env.sh\"\n"
        );
        std::fs::write(rc_path, format!("{existing}{append}"))?;
    }
    Ok(())
}

pub fn run_install_sync(_data_dir: &str, script: &Path) -> Result<()> {
    let home = home_dir();
    std::fs::create_dir_all(home.join(".local/bin"))?;

    let status = Command::new("bash")
        .arg(&script)
        .env("HOME", &home)
        .env(
            "PATH",
            format!(
                "{}:/usr/local/bin:/usr/bin:/bin",
                home.join(".local/bin").display()
            ),
        )
        .status()
        .context("run claude install.sh")?;

    if !status.success() {
        anyhow::bail!("claude install.sh exited with {}", status);
    }

    ensure_login_profile()?;
    Ok(())
}

const OAUTH_PREFIXES: &[&str] = &[
    "https://claude.com/cai/oauth/authorize",
    "https://claude.ai/oauth/authorize",
];

/// Strip ANSI escape sequences from terminal scrollback.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for ch in chars.by_ref() {
                    if ch.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

fn is_oauth_url_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-._~:/?#[]@!$&'()*+,;=%".contains(c)
}

/// Parse OAuth URL from terminal output, including when the TUI wraps across lines.
pub fn extract_oauth_url(text: &str) -> Option<String> {
    let text = strip_ansi(text);
    let start = OAUTH_PREFIXES.iter().filter_map(|p| text.find(p)).min()?;
    let tail = &text[start..];
    let url: String = tail.chars().filter(|c| is_oauth_url_char(*c)).collect();
    if oauth_url_is_complete(&url) {
        Some(url)
    } else {
        None
    }
}

/// Claude Code OAuth must include `scope` (and usually `state`).
pub fn oauth_url_is_complete(url: &str) -> bool {
    url.starts_with("https://claude.com/cai/oauth/authorize")
        && url.contains("scope=")
        && url.contains("client_id=")
        && url.contains("redirect_uri=")
}

pub fn oauth_redirect_path(token: Uuid) -> String {
    format!("/api/v1/claude/oauth/redirect/{token}")
}

pub fn oauth_browser_url(bind_port: u16, token: Uuid) -> String {
    format!(
        "http://127.0.0.1:{bind_port}{}",
        oauth_redirect_path(token)
    )
}

pub fn status_snapshot(
    install: &InstallState,
    auth: &AuthFlow,
) -> ClaudeStatus {
    ClaudeStatus {
        installed: is_installed(),
        authenticated: is_authenticated(),
        version: version_string(),
        binary: resolve_binary().map(|p| p.display().to_string()),
        install: InstallStatus {
            state: install.state.clone(),
            message: install.message.clone(),
            error: install.error.clone(),
        },
        auth: AuthStatus {
            active: auth.session_id.is_some(),
            phase: auth.phase.clone(),
            session_id: auth.session_id.map(|u| u.to_string()),
            terminal_id: auth.terminal_id.map(|u| u.to_string()),
            oauth_url: auth.oauth_url.clone(),
            oauth_browser_url: auth.oauth_browser_url.clone(),
            code_submitted: auth.code_submitted,
            error: auth.error.clone(),
        },
    }
}

pub fn spawn_install(state: Arc<AppState>) {
    let mut install = state.claude_install.lock();
    if install.state == "installing" || install.state == "downloading" {
        return;
    }
    install.state = "downloading".into();
    install.message = "Downloading Claude installer…".into();
    install.error = None;
    drop(install);

    let data_dir = state.data_dir.clone();
    tokio::spawn(async move {
        {
            let mut install = state.claude_install.lock();
            install.state = "installing".into();
            install.message = "Installing Claude Code…".into();
        }
        let result = async {
            let script = ensure_install_script(&data_dir).await?;
            let script_path = script.clone();
            tokio::task::spawn_blocking(move || run_install_sync(&data_dir, &script_path))
                .await??;
            Ok::<(), anyhow::Error>(())
        }
        .await;

        let mut install = state.claude_install.lock();
        match result {
            Ok(()) => {
                install.state = "ready".into();
                install.message = "Claude Code installed.".into();
                install.error = None;
            }
            Err(e) => {
                install.state = "failed".into();
                install.message = "Installation failed.".into();
                install.error = Some(e.to_string());
            }
        }
    });
}

pub async fn start_auth_flow(state: Arc<AppState>, user_id: Uuid, session_id: Option<Uuid>) -> Result<(Uuid, Uuid)> {
    if !is_installed() {
        anyhow::bail!("Claude is not installed — run install first");
    }

    let session_id = match session_id {
        Some(id) => id,
        None => {
            let cwd = default_shell_cwd();
            state.auth.create_stream_session(
                user_id,
                cwd.to_str().unwrap_or("/"),
                Some("Claude setup"),
            )?
        }
    };

    let cwd = default_shell_cwd();
    let shell = state.config.terminal.shell.clone();
    let bin = resolve_binary()
        .ok_or_else(|| anyhow::anyhow!("claude binary not found"))?
        .display()
        .to_string();
    let init = format!("exec \"{bin}\"");

    let secret_env = state.secret_env_for_session(session_id);
    let (term_id, tmux_target) = state.terminals.create(
        session_id,
        "claude login",
        &cwd,
        Some(&init),
        120,
        32,
        secret_env,
    )?;
    state.terminal_sessions.write().insert(term_id, session_id);
    persist_terminal(
        &state,
        term_id,
        session_id,
        "claude login",
        &shell,
        Some(&init),
        &cwd,
        120,
        32,
        tmux_target.as_deref(),
    )?;

    {
        let mut auth = state.claude_auth.lock();
        auth.session_id = Some(session_id);
        auth.terminal_id = Some(term_id);
        auth.oauth_url = None;
        auth.oauth_redirect_token = None;
        auth.oauth_browser_url = None;
        auth.code_submitted = false;
        auth.phase = "waiting_url".into();
        auth.error = None;
    }

    let poll_state = state.clone();
    let redirect_port = state.config.server.port;
    tokio::spawn(async move {
        poll_auth_flow(poll_state, term_id, redirect_port).await;
    });

    Ok((session_id, term_id))
}

async fn poll_auth_flow(state: Arc<AppState>, terminal_id: Uuid, redirect_port: u16) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(600);
    while tokio::time::Instant::now() < deadline {
        if is_authenticated() {
            let mut auth = state.claude_auth.lock();
            auth.phase = "done".into();
            auth.oauth_url = None;
            auth.oauth_redirect_token = None;
            auth.oauth_browser_url = None;
            return;
        }

        if let Some(buf) = state.terminals.recent_output(terminal_id) {
            if buf.contains("Invalid code") || buf.contains("OAuth error") {
                let mut auth = state.claude_auth.lock();
                if auth.code_submitted {
                    auth.code_submitted = false;
                    auth.phase = "waiting_code".into();
                    auth.error = Some(
                        "Invalid code — stay on the Authentication Code page and click Import again."
                            .into(),
                    );
                }
            }
            if let Some(url) = extract_oauth_url(&buf) {
                let mut auth = state.claude_auth.lock();
                store_oauth_url(&mut auth, url, redirect_port);
            }
        }

        tokio::time::sleep(Duration::from_millis(800)).await;
    }

    let mut auth = state.claude_auth.lock();
    if auth.phase != "done" {
        auth.phase = "failed".into();
        auth.error = Some("Timed out waiting for Claude sign-in.".into());
    }
}

/// Validate and normalize a Claude OAuth paste code (not `code=true` from authorize URLs).
pub fn normalize_oauth_code(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "true" || trimmed == "false" {
        return None;
    }
    if trimmed.len() < 20 || trimmed.len() > 256 {
        return None;
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '#')
    {
        return None;
    }
    Some(trimmed.to_string())
}

/// Parse `code` from Claude OAuth callback URL (Chromium CDP exposes full URL).
pub fn extract_oauth_code_from_callback_url(url: &str) -> Option<String> {
    if !url.contains("oauth/code/callback") {
        return None;
    }
    let query = url.split_once('?')?.1.split('#').next()?;
    for pair in query.split('&') {
        if let Some(code) = pair.strip_prefix("code=") {
            return normalize_oauth_code(code);
        }
    }
    None
}

/// Parse full paste code from Claude "Authentication Code" page body (includes `#` suffix).
pub fn extract_oauth_code_from_page_text(text: &str) -> Option<String> {
    for token in text.split_whitespace() {
        if token.contains('#') {
            if let Some(code) = normalize_oauth_code(token.trim_matches(|c| c == '"' || c == '\'' || c == '.' || c == ',')) {
                return Some(code);
            }
        }
    }
    None
}

async fn detect_code_from_cdp_json_list(cdp_port: u16) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .ok()?;
    let list: Vec<serde_json::Value> = client
        .get(format!("http://127.0.0.1:{cdp_port}/json/list"))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    for tab in list {
        let url = tab.get("url").and_then(|u| u.as_str())?;
        if let Some(code) = extract_oauth_code_from_callback_url(url) {
            return Some(code);
        }
    }
    None
}

fn detect_code_from_cdp_playwright(cdp_port: u16) -> Option<String> {
    let script = crate::cdp_collector::extract_oauth_script_path()?;
    let cdp_url = format!("http://127.0.0.1:{cdp_port}");
    let out = std::process::Command::new("node")
        .arg(&script)
        .arg(&cdp_url)
        .output()
        .ok()?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    v.get("code")
        .and_then(|c| c.as_str())
        .and_then(|s| normalize_oauth_code(s))
}

fn prefer_full_oauth_code(playwright: Option<String>, url_list: Option<String>) -> Option<String> {
    for code in [playwright, url_list].into_iter().flatten() {
        if code.contains('#') {
            return Some(code);
        }
    }
    None
}

pub async fn detect_code_from_cdp_port(cdp_port: u16) -> Option<String> {
    let playwright = tokio::task::spawn_blocking(move || detect_code_from_cdp_playwright(cdp_port))
        .await
        .ok()
        .flatten();
    if playwright.as_ref().is_some_and(|c| c.contains('#')) {
        return playwright;
    }
    let url_list = detect_code_from_cdp_json_list(cdp_port).await;
    prefer_full_oauth_code(playwright, url_list)
}

pub fn oauth_code_hint(code: &str) -> String {
    let chars: Vec<char> = code.chars().collect();
    if chars.len() <= 12 {
        return "••••".into();
    }
    let head: String = chars.iter().take(4).collect();
    let tail: String = chars.iter().skip(chars.len().saturating_sub(4)).collect();
    format!("{head}…{tail}")
}

pub fn apply_detected_auth_code(state: &AppState, code: &str) -> Result<()> {
    let normalized = normalize_oauth_code(code)
        .ok_or_else(|| anyhow::anyhow!("invalid or incomplete OAuth code"))?;

    {
        let auth = state.claude_auth.lock();
        if auth.code_submitted || is_authenticated() {
            return Ok(());
        }
    }

    submit_auth_code(state, &normalized)?;

    let mut auth = state.claude_auth.lock();
    auth.code_submitted = true;
    if auth.phase != "done" {
        auth.phase = "code_submitted".into();
    }
    Ok(())
}

pub fn submit_auth_code(state: &AppState, code: &str) -> Result<()> {
    let normalized = normalize_oauth_code(code)
        .ok_or_else(|| anyhow::anyhow!("invalid or incomplete OAuth code"))?;

    let auth = state.claude_auth.lock();
    if auth.code_submitted {
        return Ok(());
    }
    let term_id = auth
        .terminal_id
        .ok_or_else(|| anyhow::anyhow!("no active Claude auth flow"))?;
    drop(auth);

    write_auth_code_to_terminal(state, term_id, &normalized)?;
    Ok(())
}

fn write_auth_code_to_terminal(state: &AppState, term_id: Uuid, code: &str) -> Result<()> {
    if let Some(target) = state.terminals.tmux_target(term_id) {
        tmux::send_keys_literal(&target, code, true)?;
        state.terminals.refresh_display(term_id);
    } else {
        state.terminals.write(term_id, &format!("{code}\n"))?;
    }
    Ok(())
}

pub fn take_oauth_redirect_url(state: &AppState, token: Uuid) -> Option<String> {
    let auth = state.claude_auth.lock();
    if auth.oauth_redirect_token != Some(token) {
        return None;
    }
    auth.oauth_url.clone().filter(|u| oauth_url_is_complete(u))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_url_joins_wrapped_lines() {
        let text = "open:\nhttps://claude.com/cai/oauth/authorize?code=true&client_id=abc\n&scope=user%3Aprofile&redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback&state=xyz";
        let url = extract_oauth_url(text).expect("url");
        assert!(url.contains("scope="));
        assert!(url.contains("client_id=abc"));
        assert!(!url.contains('\n'));
    }

    #[test]
    fn rejects_url_without_scope() {
        let text = "https://claude.com/cai/oauth/authorize?code=true&client_id=abc";
        assert!(extract_oauth_url(text).is_none());
    }

    #[test]
    fn rejects_authorize_code_true_param() {
        let url = "https://claude.com/cai/oauth/authorize?code=true&client_id=abc";
        assert!(extract_oauth_code_from_callback_url(url).is_none());
    }

    #[test]
    fn parses_callback_code_from_url() {
        let url = "https://platform.claude.com/oauth/code/callback?code=abc123XYZ012345678901&state=foo";
        assert_eq!(
            extract_oauth_code_from_callback_url(url).as_deref(),
            Some("abc123XYZ012345678901")
        );
    }

    #[test]
    fn prefers_full_code_with_hash_suffix() {
        assert_eq!(
            prefer_full_oauth_code(
                Some("abc123XYZ012345678901#stateVerifier".into()),
                Some("abc123XYZ012345678901".into()),
            )
            .as_deref(),
            Some("abc123XYZ012345678901#stateVerifier")
        );
        assert!(prefer_full_oauth_code(
            None,
            Some("abc123XYZ012345678901".into()),
        )
        .is_none());
    }
}

pub fn doctor_check() -> (&'static str, bool, &'static str) {
    if is_authenticated() {
        ("Claude Code (signed in)", true, "")
    } else if is_installed() {
        ("Claude Code (not signed in)", false, "run claude login from the home page")
    } else {
        (
            "Claude Code",
            false,
            "install from the Bunny home page or: curl -fsSL https://claude.ai/install.sh | bash",
        )
    }
}
