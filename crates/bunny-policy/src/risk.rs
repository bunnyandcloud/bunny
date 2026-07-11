use crate::types::RiskLevel;

/// Classify shell/command risk (shared with voice and Discord fallback).
pub fn classify_shell_risk(cmd: &str) -> RiskLevel {
    let lower = cmd.to_lowercase();
    if lower.contains("rm -rf")
        || lower.contains("sudo")
        || lower.contains("chmod 777")
        || lower.contains("mkfs")
        || lower.contains("dd if=")
        || lower.contains("> /dev/")
        || (lower.contains("curl") && lower.contains("| sh"))
        || (lower.contains("wget") && lower.contains("| sh"))
    {
        return RiskLevel::Critical;
    }
    if lower.contains("git push")
        || lower.contains("git merge")
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
        return RiskLevel::Medium;
    }
    "safe";
    RiskLevel::Safe
}

pub fn requires_approval(cmd: &str) -> bool {
    let lower = cmd.trim().to_lowercase();
    // `claude -p '…prompt…'` embeds user text and transcript — never gate the wrapper;
    // risky shell runs are approved via Claude Bash permission_denials in Discord threads.
    if lower.starts_with("claude -p")
        || lower.starts_with("claude --print")
        || lower.contains(" claude -p ")
        || lower.contains(" claude --print ")
    {
        return false;
    }
    classify_shell_risk(cmd) != RiskLevel::Safe
}

/// Foreground servers/daemons that never exit — must be backgrounded for Discord subprocess runs.
pub fn is_long_running_discord_shell_command(cmd: &str) -> bool {
    let lower = cmd.trim().to_lowercase();
    // `claude -p '…prompt…'` embeds the full thread transcript in the shell line — never
    // background it because a prior turn may mention `npm run dev` in the prompt text.
    if lower.starts_with("claude -p")
        || lower.starts_with("claude --print")
        || lower.contains(" claude -p ")
        || lower.contains(" claude --print ")
    {
        return false;
    }
    if lower.ends_with(" &") || lower.contains(" & ") || lower.starts_with("nohup ") {
        return false;
    }
    lower.contains("http.server")
        || lower.contains(" -m simplehttpserver")
        || lower.contains("php -s ")
        || lower.contains("php -s")
        || lower.contains("php -S ")
        || lower.starts_with("uvicorn ")
        || lower.contains(" gunicorn ")
        || lower.contains("npm run dev")
        || lower.contains("npm start")
        || lower.contains("yarn dev")
        || lower.contains("pnpm dev")
        || lower.contains("next dev")
        || lower.contains("npx serve")
        || lower.starts_with("vite")
        || lower.contains(" vite ")
        || lower.contains("cargo watch")
        || lower.contains("tail -f")
        || lower.starts_with("flask run")
        || lower.contains(" flask run")
        || lower.contains(" rails server")
        || lower.contains("bin/dev")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_print_command_is_never_long_running() {
        let cmd = concat!(
            "claude -p --output-format json --resume abc ",
            "'prior turn mentioned npm run dev and next dev'"
        );
        assert!(!is_long_running_discord_shell_command(cmd));
    }

    #[test]
    fn bare_dev_server_command_is_long_running() {
        assert!(is_long_running_discord_shell_command(
            "npm run dev --prefix /root/my-app"
        ));
    }

    #[test]
    fn claude_print_wrapper_never_requires_pre_approval() {
        let cmd = concat!(
            "claude -p '# Agent mode\nUser: exécute git push -u origin main\n'",
        );
        assert!(!requires_approval(cmd));
        assert!(requires_approval("git push -u origin main"));
    }
}

/// Commands that need a real TTY — cannot run via non-interactive chat bridge.
pub fn is_interactive_discord_command(cmd: &str) -> bool {
    let lower = cmd.trim().to_lowercase();
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
