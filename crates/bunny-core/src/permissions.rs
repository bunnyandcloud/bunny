use crate::types::Role;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
        ),
        (Editor, SecretsInject | SessionShare | SessionStop | SessionReset | UsersManage | AuditView | NetworkBody | SessionDelete | ClaudeInstall | VaultManage) => false,
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
        ),
        (Viewer, SessionRead | TerminalRead | BrowserView | ConsoleView | NetworkView | PreviewView) => true,
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
