use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::Once;
use uuid::Uuid;

pub fn available() -> bool {
    tmux_version_output("tmux").is_some()
        || ["/usr/bin/tmux", "/bin/tmux", "/usr/local/bin/tmux"]
            .into_iter()
            .any(|p| tmux_version_output(p).is_some())
}

fn tmux_version_output(bin: &str) -> Option<std::process::Output> {
    Command::new(bin)
        .arg("-V")
        .output()
        .ok()
        .filter(|o| o.status.success())
}

/// Legacy: one tmux session per workspace with a window per shell.
pub fn stream_session_name(stream_session_id: Uuid) -> String {
    format!("bunny-{}", stream_session_id.as_simple())
}

/// One tmux session per shell — avoids multiple `tmux attach` clients fighting over the active window.
pub fn terminal_session_name(terminal_id: Uuid) -> String {
    format!("bunny-t-{}", terminal_id.as_simple())
}

pub fn terminal_window_name(terminal_id: Uuid) -> String {
    format!("t{}", terminal_id.as_simple())
}

pub fn target_spec(session: &str, window: &str) -> String {
    format!("{session}:{window}")
}

/// Resolve attach target from DB or heuristics (new per-terminal session, then legacy window).
pub fn inferred_target(stream_session_id: Uuid, terminal_id: Uuid) -> String {
    let per_terminal = terminal_session_name(terminal_id);
    if has_session(&per_terminal) {
        return per_terminal;
    }
    target_spec(
        &stream_session_name(stream_session_id),
        &terminal_window_name(terminal_id),
    )
}

pub fn has_session(name: &str) -> bool {
    Command::new("tmux")
        .args(["has-session", "-t", name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn has_window(session: &str, window: &str) -> bool {
    let Ok(out) = Command::new("tmux")
        .args(["list-windows", "-t", session, "-F", "#{window_name}"])
        .output()
    else {
        return false;
    };
    if !out.status.success() {
        return false;
    }
    let names = String::from_utf8_lossy(&out.stdout);
    names.lines().any(|l| l == window)
}

/// Hide tmux status bar / messages for embedded web UI; keep sessions alive across attach clients.
pub fn configure_session_for_web(session: &str) {
    static GLOBAL_ONCE: Once = Once::new();
    GLOBAL_ONCE.call_once(|| {
        apply_utf8_locale_global();
        for args in [
            vec!["set-option", "-g", "exit-empty-time", "0"],
            vec!["set-option", "-g", "destroy-unattached", "off"],
        ] {
            let _ = run(&args);
        }
    });
    for args in [
        vec!["set-option", "-t", session, "focus-events", "on"],
        vec!["set-option", "-t", session, "status", "off"],
        vec!["set-option", "-t", session, "status-position", "off"],
        vec!["set-option", "-t", session, "message", "off"],
        vec!["set-option", "-t", session, "message-command", "off"],
        vec!["set-option", "-t", session, "aggressive-resize", "on"],
        vec!["set-option", "-t", session, "assume-default-size", "on"],
        vec!["set-option", "-t", session, "remain-on-exit", "off"],
        // Keep app output in the main buffer so the Web UI can scroll full history (npm run dev, etc.).
        vec!["set-option", "-t", session, "alternate-screen", "off"],
    ] {
        let _ = run(&args);
    }
    apply_utf8_locale(session);
}

/// Per-pane: ignore application alternate-screen requests (scrollback stays in the web terminal).
pub fn configure_pane_for_web(target: &str) {
    if !target_alive(target) {
        return;
    }
    let _ = run(&[
        "set-option",
        "-p",
        "-t",
        target,
        "alternate-screen",
        "off",
    ]);
}

pub fn apply_utf8_locale_global() {
    for (key, value) in crate::locale::utf8_locale_vars() {
        let _ = run(&["set-environment", "-g", key, value]);
    }
}

pub fn apply_utf8_locale(session: &str) {
    for (key, value) in crate::locale::utf8_locale_vars() {
        let _ = run(&["set-environment", "-t", session, key, value]);
    }
}

pub fn session_name_from_target(target: &str) -> &str {
    target.split_once(':').map(|(s, _)| s).unwrap_or(target)
}

pub fn ensure_session(session: &str, cwd: &Path) -> Result<()> {
    if has_session(session) {
        configure_session_for_web(session);
        return Ok(());
    }
    let cwd = cwd.to_str().context("invalid cwd")?;
    run(&["new-session", "-d", "-s", session, "-c", cwd])?;
    configure_session_for_web(session);
    Ok(())
}

/// Dedicated tmux session for one web shell (recommended for multi-tab workspaces).
pub fn ensure_terminal_session(
    terminal_id: Uuid,
    cwd: &Path,
    init_command: Option<&str>,
    secret_env: &HashMap<String, String>,
) -> Result<String> {
    let name = terminal_session_name(terminal_id);
    if has_session(&name) {
        configure_session_for_web(&name);
        apply_session_secrets(&name, secret_env);
        return Ok(name);
    }
    let cwd = cwd.to_str().context("invalid cwd")?;
    let mut args = vec![
        "new-session".to_string(),
        "-d".to_string(),
        "-s".to_string(),
        name.clone(),
        "-c".to_string(),
        cwd.to_string(),
    ];
    append_env_flags(&mut args, secret_env);
    if let Some(cmd) = init_command {
        args.push(cmd.to_string());
    }
    run_owned(&args)?;
    configure_session_for_web(&name);
    Ok(name)
}

/// Legacy: add a window to the stream session.
pub fn create_window(
    session: &str,
    window: &str,
    cwd: &Path,
    init_command: Option<&str>,
) -> Result<()> {
    ensure_session(session, cwd)?;
    if has_window(session, window) {
        return Ok(());
    }
    let cwd = cwd.to_str().context("invalid cwd")?;
    let mut args = vec!["new-window", "-t", session, "-n", window, "-c", cwd];
    if let Some(cmd) = init_command {
        args.push(cmd);
    }
    run(&args)?;
    Ok(())
}

pub fn refresh_client(target: &str) {
    let _ = run(&["refresh-client", "-t", target]);
}

/// Send literal keystrokes to a tmux pane (bypasses web xterm / host clipboard).
pub fn send_keys_literal(target: &str, text: &str, enter: bool) -> Result<()> {
    if !target_alive(target) {
        anyhow::bail!("tmux target not alive: {target}");
    }
    run(&["send-keys", "-t", target, "-l", "--", text])?;
    if enter {
        submit_line(target)?;
    }
    Ok(())
}

/// Submit the current line in a pane (Enter). Prefer C-m — more reliable than the Enter key name in headless tmux.
pub fn submit_line(target: &str) -> Result<()> {
    if !target_alive(target) {
        anyhow::bail!("tmux target not alive: {target}");
    }
    std::thread::sleep(std::time::Duration::from_millis(80));
    run(&["send-keys", "-t", target, "C-m"])?;
    Ok(())
}

/// Send a special key (e.g. `Escape`, `C-c`) to a tmux pane.
pub fn send_keys_key(target: &str, key: &str) -> Result<()> {
    if !target_alive(target) {
        anyhow::bail!("tmux target not alive: {target}");
    }
    run(&["send-keys", "-t", target, key])?;
    Ok(())
}

/// Visible pane + scrollback (use before respawn or recreating a session).
/// Current working directory of the tmux pane (if session still exists).
pub fn pane_cwd(target: &str) -> Option<String> {
    let out = Command::new("tmux")
        .args([
            "display-message",
            "-p",
            "-t",
            target,
            "#{pane_current_path}",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let cwd = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if cwd.is_empty() {
        None
    } else {
        Some(cwd)
    }
}

pub fn capture_pane(target: &str) -> Result<String> {
    let out = Command::new("tmux")
        .args(["capture-pane", "-p", "-t", target, "-S", "-10000"])
        .output()
        .with_context(|| format!("capture-pane -t {target}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("tmux capture-pane failed: {stderr}");
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Visible pane only (no scrollback) — matches what you see on screen (vim, htop, etc.).
pub fn capture_pane_visible(target: &str) -> Result<String> {
    let out = Command::new("tmux")
        .args(["capture-pane", "-p", "-t", target])
        .output()
        .with_context(|| format!("capture-pane (visible) -t {target}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("tmux capture-pane failed: {stderr}");
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// True when the pane process has exited (scrollback may still show `logout`, etc.).
pub fn pane_is_dead(target: &str) -> bool {
    let Ok(out) = Command::new("tmux")
        .args(["list-panes", "-t", target, "-F", "#{pane_dead}"])
        .output()
    else {
        return false;
    };
    if !out.status.success() {
        return false;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .any(|l| l.trim() == "1")
}

/// Respawn the pane with `BUNNY_SECRET_*` in the process environment (values never echoed).
pub fn reload_shell_secrets(
    target: &str,
    cwd: &Path,
    shell: &str,
    secret_env: &HashMap<String, String>,
) -> Result<()> {
    if !target_alive(target) || secret_env.is_empty() {
        return Ok(());
    }
    let session = session_name_from_target(target);
    configure_session_for_web(session);
    apply_session_secrets(session, secret_env);
    let cwd = cwd.to_str().context("invalid cwd")?;
    tracing::info!(%target, "reloading shell with vault secrets");
    let mut args = vec![
        "respawn-pane".to_string(),
        "-k".to_string(),
        "-t".to_string(),
        target.to_string(),
        "-c".to_string(),
        cwd.to_string(),
    ];
    append_env_flags(&mut args, secret_env);
    args.push(shell.to_string());
    run_owned(&args)?;
    Ok(())
}

/// Start a fresh shell in the target when the pane died (e.g. after agent stop + SIGHUP).
pub fn ensure_shell_running(
    target: &str,
    cwd: &Path,
    shell: &str,
    secret_env: &HashMap<String, String>,
) -> Result<()> {
    if !target_alive(target) {
        return Ok(());
    }
    if !pane_is_dead(target) {
        apply_session_secrets(session_name_from_target(target), secret_env);
        return Ok(());
    }
    reload_shell_secrets(target, cwd, shell, secret_env)
}

pub fn kill_window(session: &str, window: &str) {
    if has_window(session, window) {
        let target = target_spec(session, window);
        let _ = run(&["kill-window", "-t", &target]);
    }
}

pub fn kill_session(name: &str) {
    if has_session(name) {
        let _ = run(&["kill-session", "-t", name]);
    }
}

pub fn kill_target(target: &str) {
    if let Some((session, window)) = parse_target(target) {
        kill_window(&session, &window);
    } else {
        kill_session(target);
    }
}

pub fn kill_terminal_session(terminal_id: Uuid) {
    kill_session(&terminal_session_name(terminal_id));
}

pub fn kill_stream_session(stream_session_id: Uuid) {
    kill_session(&stream_session_name(stream_session_id));
}

pub fn parse_target(target: &str) -> Option<(String, String)> {
    let (session, window) = target.split_once(':')?;
    if session.is_empty() || window.is_empty() {
        return None;
    }
    Some((session.to_string(), window.to_string()))
}

pub fn target_alive(target: &str) -> bool {
    if let Some((session, window)) = parse_target(target) {
        has_session(&session) && has_window(&session, &window)
    } else {
        has_session(target)
    }
}

fn append_env_flags(args: &mut Vec<String>, secret_env: &HashMap<String, String>) {
    for (key, value) in crate::locale::utf8_locale_vars() {
        args.push("-e".to_string());
        args.push(format!("{key}={value}"));
    }
    for (key, value) in secret_env {
        if key.starts_with("BUNNY_SECRET_") {
            args.push("-e".to_string());
            args.push(format!("{key}={value}"));
        }
    }
}

fn apply_session_secrets(session: &str, secret_env: &HashMap<String, String>) {
    for (key, value) in secret_env {
        if key.starts_with("BUNNY_SECRET_") {
            let _ = run(&["set-environment", "-t", session, key, value]);
        }
    }
}

fn run_owned(args: &[String]) -> Result<()> {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run(&refs)
}

fn run(args: &[&str]) -> Result<()> {
    let out = Command::new("tmux")
        .args(args)
        .output()
        .with_context(|| format!("failed to run tmux {}", args.join(" ")))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    anyhow::bail!("tmux {} failed: {stderr}", args.join(" "));
}
