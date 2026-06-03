use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordInstallation {
    pub guild_id: String,
    pub installed_by_user_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordSessionLink {
    pub guild_id: String,
    pub channel_id: String,
    pub session_id: Uuid,
    pub created_by_user_id: Option<Uuid>,
    pub status: String,
    pub project_cwd_override: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscordThreadStatus {
    Active,
    Goal,
    Cancelled,
}

impl DiscordThreadStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Goal => "goal",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "goal" => Self::Goal,
            "cancelled" => Self::Cancelled,
            _ => Self::Active,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordThreadBinding {
    pub guild_id: String,
    pub channel_id: String,
    pub thread_id: String,
    pub session_id: Uuid,
    pub task_id: Uuid,
    pub term_id: Uuid,
    pub project_cwd: String,
    pub status: DiscordThreadStatus,
    pub goal_text: Option<String>,
    pub git_enabled: bool,
    pub base_branch: Option<String>,
    pub thread_branch: Option<String>,
    pub start_commit: Option<String>,
    pub last_pane_marker: usize,
    pub last_pane_snapshot: String,
    pub last_input_discord_message_id: Option<String>,
    pub claude_session_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscordThreadMessageRole {
    User,
    Assistant,
    Discussion,
}

impl DiscordThreadMessageRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Discussion => "discussion",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "assistant" => Self::Assistant,
            "discussion" => Self::Discussion,
            _ => Self::User,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordThreadMessage {
    pub id: Uuid,
    pub thread_id: String,
    pub role: DiscordThreadMessageRole,
    pub discord_user_id: Option<String>,
    pub author_name: Option<String>,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordThreadDiscussion {
    pub id: Uuid,
    pub thread_id: String,
    pub discord_user_id: String,
    pub author_name: Option<String>,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

/// One multiple-choice item from Claude `AskUserQuestion` (Discord buttons).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskUserQuestionOption {
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskUserQuestionItem {
    pub question: String,
    #[serde(default)]
    pub header: Option<String>,
    #[serde(default, rename = "multiSelect", alias = "multi_select")]
    pub multi_select: bool,
    pub options: Vec<AskUserQuestionOption>,
}

/// Pending AskUserQuestion round for a Discord thread (answered via buttons).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordThreadPendingQuestions {
    pub id: Uuid,
    pub thread_id: String,
    pub questions: Vec<AskUserQuestionItem>,
    /// Maps question text → selected option label(s), comma-separated if multi-select.
    pub answers: std::collections::HashMap<String, String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordLinkCode {
    pub code: String,
    pub session_id: Uuid,
    pub created_by_user_id: Uuid,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTaskMode {
    Ask,
    Plan,
    Do,
    Shell,
    Browser,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTaskStatus {
    Queued,
    Running,
    WaitingApproval,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTask {
    pub id: Uuid,
    pub session_id: Uuid,
    pub source: String,
    pub discord_thread_id: Option<String>,
    pub requested_by_discord_id: Option<String>,
    pub requested_by_user_id: Option<Uuid>,
    pub agent: String,
    pub mode: AgentTaskMode,
    pub status: AgentTaskStatus,
    pub prompt: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: Uuid,
    pub task_id: Uuid,
    pub session_id: Uuid,
    pub action_summary: String,
    pub reason: String,
    pub status: String,
    pub discord_message_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchSession {
    pub id: Uuid,
    pub token: String,
    pub session_id: Uuid,
    pub guild_id: String,
    pub channel_id: String,
    pub thread_id: Option<String>,
    pub layout: String,
    pub visibility: String,
    pub mode: String,
    pub status: String,
    pub required_role_ids: Vec<String>,
    pub browser_id: Option<Uuid>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordUserLink {
    pub discord_user_id: String,
    pub user_id: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordAuditEntry {
    pub id: Uuid,
    pub discord_user_id: Option<String>,
    pub bunny_user_id: Option<Uuid>,
    pub guild_id: Option<String>,
    pub channel_id: Option<String>,
    pub thread_id: Option<String>,
    pub session_id: Option<Uuid>,
    pub command: String,
    pub action_executed: String,
    pub agent: Option<String>,
    pub shell_id: Option<Uuid>,
    pub browser_id: Option<Uuid>,
    pub approval_id: Option<Uuid>,
    pub result: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordFollow {
    pub id: Uuid,
    pub guild_id: String,
    pub channel_id: String,
    pub session_id: Uuid,
    pub target: String,
    pub shell_name: Option<String>,
    pub interval_secs: u64,
    pub active: bool,
    pub created_at: DateTime<Utc>,
}
