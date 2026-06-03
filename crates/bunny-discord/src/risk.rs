/// Classify shell/command risk for approval flows (shared with voice).
pub fn classify_command_risk(cmd: &str) -> &'static str {
    let lower = cmd.to_lowercase();
    if lower.contains("rm -rf")
        || lower.contains("sudo")
        || lower.contains("chmod 777")
        || lower.contains("mkfs")
        || lower.contains("dd if=")
        || lower.contains("> /dev/")
        || lower.contains("curl") && lower.contains("| sh")
        || lower.contains("wget") && lower.contains("| sh")
    {
        return "dangerous";
    }
    if lower.contains("git push")
        || lower.contains("npm install")
        || lower.contains("pip install")
        || lower.contains("cargo install")
        || lower.contains("apt install")
        || lower.contains("apt-get install")
        || lower.contains("env ")
        || lower.contains("printenv")
        || lower.contains("secret")
        || lower.contains(".env")
    {
        return "medium";
    }
    "safe"
}

pub fn requires_approval(cmd: &str) -> bool {
    classify_command_risk(cmd) != "safe"
}

/// Commands that need a real TTY — cannot run via Discord `/bunny run` (non-interactive).
pub fn is_interactive_discord_command(cmd: &str) -> bool {
    let lower = cmd.trim().to_lowercase();
    // Headless Claude Code (`-p` / `--print`) runs as a subprocess, not in the tmux pane.
    if lower.starts_with("claude -p")
        || lower.starts_with("claude --print")
        || lower.contains(" claude -p ")
        || lower.contains(" claude --print ")
    {
        return false;
    }
    let first = lower.split_whitespace().next().unwrap_or("");
    (matches!(
        first,
        "nvim" | "vim" | "vi" | "nano" | "emacs" | "less" | "more" | "man" | "htop" | "top"
            | "btop" | "claude" | "ssh" | "mysql" | "psql"
    ) && !lower.contains(" -c "))
        || lower.starts_with("sudo ")
}
