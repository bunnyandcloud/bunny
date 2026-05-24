use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;
use uuid::Uuid;

pub fn available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
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
    for args in [
        vec!["set-option", "-t", session, "-g", "status", "off"],
        vec!["set-option", "-t", session, "-g", "status-position", "off"],
        vec!["set-option", "-t", session, "-g", "message", "off"],
        vec!["set-option", "-t", session, "-g", "message-command", "off"],
        vec!["set-option", "-t", session, "-g", "aggressive-resize", "on"],
        vec!["set-option", "-t", session, "-g", "assume-default-size", "on"],
        // Keep bash sessions when the web agent disconnects its tmux attach client.
        vec!["set-option", "-t", session, "-g", "exit-empty-time", "0"],
        vec!["set-option", "-t", session, "-g", "destroy-unattached", "off"],
        vec!["set-option", "-t", session, "-g", "remain-on-exit", "off"],
    ] {
        let _ = run(&args);
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
) -> Result<String> {
    let name = terminal_session_name(terminal_id);
    if has_session(&name) {
        configure_session_for_web(&name);
        return Ok(name);
    }
    let cwd = cwd.to_str().context("invalid cwd")?;
    if let Some(cmd) = init_command {
        run(&["new-session", "-d", "-s", &name, "-c", cwd, cmd])?;
    } else {
        run(&["new-session", "-d", "-s", &name, "-c", cwd])?;
    }
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

/// Start a fresh shell in the target when the pane died (e.g. after agent stop + SIGHUP).
pub fn ensure_shell_running(target: &str, cwd: &Path, shell: &str) -> Result<()> {
    if !target_alive(target) {
        return Ok(());
    }
    if !pane_is_dead(target) {
        return Ok(());
    }
    let session = session_name_from_target(target);
    configure_session_for_web(session);
    let cwd = cwd.to_str().context("invalid cwd")?;
    tracing::info!(%target, "respawning dead tmux pane");
    run(&["respawn-pane", "-k", "-t", target, "-c", cwd, shell])?;
    Ok(())
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
