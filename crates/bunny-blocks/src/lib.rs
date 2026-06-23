use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    UserCommand,
    DiscordCommand,
    Output,
    ProcessRun,
    SystemEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorSource {
    Web,
    Discord,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalBlock {
    pub id: Uuid,
    pub terminal_id: Uuid,
    pub seq: i64,
    pub kind: BlockKind,
    pub author_user_id: Option<Uuid>,
    pub author_display: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_git_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_git_email: Option<String>,
    pub author_source: AuthorSource,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub command: Option<String>,
    pub content: String,
    pub exit_code: Option<i32>,
    pub status: BlockStatus,
    pub parent_block_id: Option<Uuid>,
    pub meta: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BlockPatch {
    pub status: Option<BlockStatus>,
    pub content_delta: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_replace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
    pub exit_code: Option<i32>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl TerminalBlock {
    pub fn author_initial(&self) -> String {
        self.author_display
            .chars()
            .find(|c| c.is_alphanumeric())
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_else(|| "?".into())
    }
}
