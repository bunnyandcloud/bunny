//! Shell/command risk classification — delegates to bunny-policy.

pub use bunny_policy::{
    classify_shell_risk, is_interactive_discord_command, is_long_running_discord_shell_command,
    requires_approval, RiskLevel,
};
