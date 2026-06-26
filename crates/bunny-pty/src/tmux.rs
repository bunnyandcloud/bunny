use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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

/// Per-pane: allow alternate-screen for full-screen TUIs (nvim, htop, installers).
pub fn configure_pane_for_interactive(target: &str) {
    if !target_alive(target) {
        return;
    }
    let _ = run(&[
        "set-option",
        "-p",
        "-t",
        target,
        "alternate-screen",
        "on",
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

/// Bash init file that re-exports Bunny PATH / terminal id (survives tmux session env drift).
pub fn write_shell_init_script(
    data_dir: &Path,
    terminal_id: Uuid,
    path_with_bin: &str,
) -> Result<PathBuf> {
    let dir = data_dir.join("git-identity");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("shell-init-{terminal_id}.sh"));
    let content = format!(
        r#"export PATH="{path_with_bin}"
export BUNNY_TERMINAL_ID="{terminal_id}"
if [ -f "$HOME/.bashrc" ]; then
  . "$HOME/.bashrc"
fi
"#,
    );
    std::fs::write(&path, content)?;
    Ok(path)
}

pub fn shell_command_with_init(shell: &str, init_path: &Path) -> String {
    if shell.contains("bash") {
        format!("{shell} --init-file {}", init_path.display())
    } else {
        shell.to_string()
    }
}

pub fn interactive_shell_command(
    data_dir: &Path,
    terminal_id: Uuid,
    shell: &str,
    session_env: &HashMap<String, String>,
) -> Result<String> {
    let path = session_env
        .get("PATH")
        .map(String::as_str)
        .unwrap_or("/usr/bin:/bin");
    let init = write_shell_init_script(data_dir, terminal_id, path)?;
    Ok(shell_command_with_init(shell, &init))
}

/// Dedicated tmux session for one web shell (recommended for multi-tab workspaces).
pub fn ensure_terminal_session(
    terminal_id: Uuid,
    cwd: &Path,
    init_command: Option<&str>,
    interactive_shell: &str,
    secret_env: &HashMap<String, String>,
) -> Result<String> {
    let name = terminal_session_name(terminal_id);
    if has_session(&name) {
        configure_session_for_web(&name);
        apply_session_env(&name, secret_env);
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
    } else {
        args.push(interactive_shell.to_string());
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

/// Match tmux pane geometry to the web xterm (attach SIGWINCH is not always enough).
pub fn resize_pane(target: &str, cols: u16, rows: u16) -> Result<()> {
    if !target_alive(target) {
        return Ok(());
    }
    run(&[
        "resize-pane",
        "-t",
        target,
        "-x",
        &cols.to_string(),
        "-y",
        &rows.to_string(),
    ])?;
    Ok(())
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

/// Stop the foreground job in a pane: Ctrl+C, then SIGTERM direct children if still busy.
pub fn interrupt_pane_foreground(target: &str) -> Result<()> {
    if !target_alive(target) {
        anyhow::bail!("tmux target not alive: {target}");
    }
    for _ in 0..2 {
        send_keys_key(target, "C-c")?;
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    std::thread::sleep(std::time::Duration::from_millis(400));
    if pane_foreground_busy(target) {
        terminate_pane_foreground_children(target);
        std::thread::sleep(std::time::Duration::from_millis(200));
        let _ = send_keys_key(target, "C-c");
    }
    Ok(())
}

fn pane_foreground_busy(target: &str) -> bool {
    if pane_has_non_shell_child(target) {
        return true;
    }
    let Some(cmd) = pane_current_command(target) else {
        return false;
    };
    let base = cmd
        .rsplit('/')
        .next()
        .unwrap_or(&cmd)
        .trim()
        .to_lowercase();
    const IDLE: &[&str] = &["bash", "zsh", "sh", "dash", "fish", "nu", "ksh", "tcsh", "-sh"];
    !IDLE.iter().any(|shell| base == *shell || base.ends_with(shell))
}

fn terminate_pane_foreground_children(target: &str) {
    let Some(shell_pid) = pane_pid(target) else {
        return;
    };
    let Ok(out) = Command::new("ps")
        .args([
            "--ppid",
            &shell_pid.to_string(),
            "-o",
            "pid=",
            "--no-headers",
        ])
        .output()
    else {
        return;
    };
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Ok(child_pid) = line.trim().parse::<u32>() else {
            continue;
        };
        if child_pid == 0 {
            continue;
        }
        let _ = Command::new("kill")
            .args(["-TERM", &child_pid.to_string()])
            .status();
    }
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

/// Name of the program running in the pane (empty when unknown / idle shell).
pub fn pane_current_command(target: &str) -> Option<String> {
    if !target_alive(target) {
        return None;
    }
    let out = Command::new("tmux")
        .args([
            "display-message",
            "-p",
            "-t",
            target,
            "-F",
            "#{pane_current_command}",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let cmd = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if cmd.is_empty() {
        None
    } else {
        Some(cmd)
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

/// Wipe tmux scrollback for a pane (visible screen is cleared separately via the shell).
pub fn clear_pane_history(target: &str) {
    if !target_alive(target) {
        return;
    }
    let _ = run(&["clear-history", "-t", target]);
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

/// Respawn the pane with session env (PATH, BUNNY_TERMINAL_ID, vault secrets).
pub fn reload_shell_env(
    target: &str,
    cwd: &Path,
    shell: &str,
    session_env: &HashMap<String, String>,
) -> Result<()> {
    if !target_alive(target) || session_env.is_empty() {
        return Ok(());
    }
    let session = session_name_from_target(target);
    configure_session_for_web(session);
    apply_session_env(session, session_env);
    // Keep the pane's live cwd (`cd` in notebook blocks) — do not reset to the DB snapshot.
    let effective_cwd = pane_cwd(target)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| cwd.to_path_buf());
    let cwd = effective_cwd.to_str().context("invalid cwd")?;
    tracing::info!(%target, %cwd, "reloading shell with Bunny session env");
    let mut args = vec![
        "respawn-pane".to_string(),
        "-k".to_string(),
        "-t".to_string(),
        target.to_string(),
        "-c".to_string(),
        cwd.to_string(),
    ];
    append_env_flags(&mut args, session_env);
    args.push(shell.to_string());
    run_owned(&args)?;
    Ok(())
}

/// Legacy alias.
pub fn reload_shell_secrets(
    target: &str,
    cwd: &Path,
    shell: &str,
    secret_env: &HashMap<String, String>,
) -> Result<()> {
    reload_shell_env(target, cwd, shell, secret_env)
}

/// Start a fresh shell in the target when the pane died (e.g. after agent stop + SIGHUP),
/// or respawn when the live pane is missing Bunny git env (PATH wrapper / BUNNY_TERMINAL_ID).
pub fn ensure_shell_running(
    target: &str,
    cwd: &Path,
    shell_cmd: &str,
    session_env: &HashMap<String, String>,
    preserve_user_session: bool,
) -> Result<()> {
    if !target_alive(target) {
        return Ok(());
    }
    let session = session_name_from_target(target);
    apply_session_env(session, session_env);
    if pane_is_dead(target) {
        return reload_shell_env(target, cwd, shell_cmd, session_env);
    }
    if preserve_user_session {
        return Ok(());
    }
    if pane_needs_env_reload(target, session_env) && shell_pane_is_idle(target, shell_cmd) {
        return reload_shell_env(target, cwd, shell_cmd, session_env);
    }
    Ok(())
}

fn shell_pane_is_idle(target: &str, shell_cmd: &str) -> bool {
    let Some(cmd) = pane_current_command(target) else {
        return true;
    };
    let shell_base = std::path::Path::new(
        shell_cmd
            .split_whitespace()
            .next()
            .unwrap_or(shell_cmd),
    )
    .file_name()
    .and_then(|s| s.to_str())
    .unwrap_or(shell_cmd);
    cmd == shell_base
        || cmd.ends_with("bash")
        || cmd.ends_with("sh")
        || cmd == "-sh"
        || cmd == "bash"
        || cmd == "sh"
}

fn pane_needs_env_reload(target: &str, session_env: &HashMap<String, String>) -> bool {
    let Some(pid) = pane_pid(target) else {
        return false;
    };
    if let Some(expected) = session_env.get("BUNNY_TERMINAL_ID") {
        match process_env_value(pid, "BUNNY_TERMINAL_ID") {
            Some(actual) if actual == *expected => {}
            Some(_) => return true,
            None => return false,
        }
    }
    if let Some(path_env) = session_env.get("PATH") {
        let bunny_bin = path_env.split(':').next().filter(|s| !s.is_empty());
        if let Some(expected_bin) = bunny_bin {
            let actual_path = match process_env_value(pid, "PATH") {
                Some(p) => p,
                None => return false,
            };
            if !actual_path
                .split(':')
                .any(|segment| segment == expected_bin)
            {
                return true;
            }
        }
    }
    false
}

fn pane_pid(target: &str) -> Option<u32> {
    let out = Command::new("tmux")
        .args(["display-message", "-p", "-t", target, "-F", "#{pane_pid}"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .ok()
}

/// True when the shell in this pane has a foreground child (e.g. bash running `htop` or `sleep`).
pub fn pane_has_non_shell_child(target: &str) -> bool {
    let Some(pid) = pane_pid(target) else {
        return false;
    };
    let Some(out) = Command::new("ps")
        .args([
            "--ppid",
            &pid.to_string(),
            "-o",
            "comm=",
            "--no-headers",
        ])
        .output()
        .ok()
    else {
        return false;
    };
    if !out.status.success() {
        return false;
    }
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let comm = line.trim();
        if comm.is_empty() {
            continue;
        }
        let base = comm
            .rsplit('/')
            .next()
            .unwrap_or(comm)
            .trim()
            .to_lowercase();
        const SHELL: &[&str] = &["bash", "zsh", "sh", "dash", "fish", "nu", "ksh", "tcsh"];
        if !SHELL.iter().any(|shell| base == *shell || base.ends_with(shell)) {
            return true;
        }
    }
    false
}

fn process_env_value(pid: u32, key: &str) -> Option<String> {
    let data = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
    let prefix = format!("{key}=");
    for entry in data.split(|&b| b == 0) {
        if entry.starts_with(prefix.as_bytes()) {
            let value = entry.get(prefix.len()..)?;
            if value.is_empty() {
                return None;
            }
            return String::from_utf8(value.to_vec()).ok();
        }
    }
    None
}

/// Read an environment variable from the live shell process in a tmux pane.
pub fn pane_shell_env_var(target: &str, key: &str) -> Option<String> {
    let pid = pane_pid(target)?;
    process_env_value(pid, key)
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

fn append_env_flags(args: &mut Vec<String>, session_env: &HashMap<String, String>) {
    for (key, value) in crate::locale::utf8_locale_vars() {
        args.push("-e".to_string());
        args.push(format!("{key}={value}"));
    }
    for (key, value) in session_env {
        if key.starts_with("BUNNY_SECRET_")
            || key == "BUNNY_TERMINAL_ID"
            || key == "PATH"
        {
            args.push("-e".to_string());
            args.push(format!("{key}={value}"));
        }
    }
}

fn apply_session_secrets(session: &str, session_env: &HashMap<String, String>) {
    for (key, value) in session_env {
        if key.starts_with("BUNNY_SECRET_")
            || key == "BUNNY_TERMINAL_ID"
            || key == "PATH"
        {
            let _ = run(&["set-environment", "-t", session, key, value]);
        }
    }
}

/// Refresh tmux session environment (PATH, BUNNY_TERMINAL_ID, vault secrets).
pub fn apply_session_env(session: &str, session_env: &HashMap<String, String>) {
    apply_session_secrets(session, session_env);
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
