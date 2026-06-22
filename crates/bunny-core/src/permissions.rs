use crate::types::Role;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    SessionCreate,
    SessionRead,
    SessionUpdate,
    SessionDelete,
    TerminalRead,
    TerminalWrite,
    TerminalRestart,
    BrowserView,
    BrowserControl,
    ConsoleView,
    NetworkView,
    NetworkBody,
    PreviewView,
    SessionShare,
    SessionStop,
    SessionReset,
    ClaudeInstall,
    VaultManage,
    SecretsInject,
    VoiceUse,
    VoiceRun,
    UsersManage,
    AuditView,
    DiscordLink,
    DiscordControl,
    DiscordAgentRun,
    DiscordApprove,
    DiscordWatch,
    IntegrationConnect,
    IntegrationManage,
    ActionApprove,
    ActionExecute,
    GitWorktreeManage,
}

pub fn role_can(role: Role, action: Action) -> bool {
    use Action::*;
    use Role::*;
    match (role, action) {
        (Owner, _) => true,
        (Admin, SecretsInject | NetworkBody) => false,
        (Admin, action) => matches!(
            action,
            SessionCreate
                | SessionRead
                | SessionUpdate
                | SessionDelete
                | TerminalRead
                | TerminalWrite
                | TerminalRestart
                | BrowserView
                | BrowserControl
                | ConsoleView
                | NetworkView
                | PreviewView
                | SessionShare
                | SessionStop
                | SessionReset
                | ClaudeInstall
                | VaultManage
                | VoiceUse
                | VoiceRun
                | UsersManage
                | AuditView
                | DiscordLink
                | DiscordControl
                | DiscordAgentRun
                | DiscordApprove
                | IntegrationConnect
                | IntegrationManage
                | ActionApprove
                | ActionExecute
                | GitWorktreeManage
        ),
        (Editor, SecretsInject | SessionShare | SessionStop | SessionReset | UsersManage | AuditView | NetworkBody | SessionDelete | ClaudeInstall | VaultManage | DiscordLink | DiscordApprove | IntegrationManage | ActionApprove) => false,
        (Editor, action) => matches!(
            action,
            SessionRead
                | SessionUpdate
                | TerminalRead
                | TerminalWrite
                | TerminalRestart
                | BrowserView
                | BrowserControl
                | ConsoleView
                | NetworkView
                | PreviewView
                | VoiceUse
                | VoiceRun
                | DiscordControl
                | DiscordAgentRun
                | IntegrationConnect
                | ActionExecute
                | GitWorktreeManage
        ),
        (Viewer, SessionRead | TerminalRead | BrowserView | ConsoleView | NetworkView | PreviewView | DiscordWatch) => true,
        (Viewer, _) => false,
        (Agent, SessionRead | TerminalRead | ConsoleView | NetworkView) => true,
        (Agent, _) => false,
    }
}

pub fn parse_role(s: &str) -> Option<Role> {
    match s.to_lowercase().as_str() {
        "owner" => Some(Role::Owner),
        "admin" => Some(Role::Admin),
        "editor" => Some(Role::Editor),
        "viewer" => Some(Role::Viewer),
        "agent" => Some(Role::Agent),
        _ => None,
    }
}
