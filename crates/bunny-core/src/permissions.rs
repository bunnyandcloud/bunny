use crate::types::Role;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
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
            TerminalRead
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
                | VoiceUse
                | VoiceRun
                | UsersManage
                | AuditView
        ),
        (Editor, SecretsInject | SessionShare | SessionStop | SessionReset | UsersManage | AuditView | NetworkBody) => false,
        (Editor, action) => matches!(
            action,
            TerminalRead
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
        (Viewer, TerminalRead | BrowserView | ConsoleView | NetworkView | PreviewView) => true,
        (Viewer, _) => false,
        (Agent, TerminalRead | ConsoleView | NetworkView) => true,
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
