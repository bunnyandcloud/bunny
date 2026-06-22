use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeContext {
    pub bridge_id: String,
    pub workspace_id: String,
    pub channel_id: String,
    pub conversation_id: Option<String>,
    pub external_user_id: String,
    pub external_message_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalMessageRef {
    pub bridge_id: String,
    pub workspace_id: String,
    pub channel_id: String,
    pub message_id: String,
    pub conversation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthConfig {
    pub authorize_url: String,
    pub token_url: String,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeAuthConfig {
    pub bridge_token_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSessionLink {
    pub bridge_id: String,
    pub workspace_id: String,
    pub channel_id: String,
    pub session_id: Uuid,
    pub project_cwd_override: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkWorkspaceRequest {
    pub bridge_id: String,
    pub workspace_id: String,
    pub channel_id: String,
    pub session_id: Uuid,
    pub link_code: String,
    pub created_by_user_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCommandDefinition {
    pub name: String,
    pub description: String,
    pub required_bunny_action: bunny_core::permissions::Action,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationBindRequest {
    pub bridge_id: String,
    pub workspace_id: String,
    pub channel_id: String,
    pub conversation_id: String,
    pub session_id: Uuid,
    pub goal_text: String,
    pub bunny_user_id: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationStatus {
    Active,
    Goal,
    Cancelled,
}

impl ConversationStatus {
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
pub struct ConversationBinding {
    pub id: Uuid,
    pub bridge_id: String,
    pub workspace_id: String,
    pub channel_id: String,
    pub conversation_id: String,
    pub session_id: Uuid,
    pub task_id: Uuid,
    pub term_id: Uuid,
    pub project_cwd: String,
    pub git_lease_id: Option<Uuid>,
    pub status: ConversationStatus,
    pub goal_text: Option<String>,
    pub git_enabled: bool,
    pub base_branch: Option<String>,
    pub thread_branch: Option<String>,
    pub start_commit: Option<String>,
    pub claude_session_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationContext {
    pub binding: ConversationBinding,
    pub bridge_ctx: BridgeContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    pub content: String,
    pub role: ConversationMessageRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationMessageRole {
    User,
    Assistant,
    System,
    Discussion,
}

impl ConversationMessageRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::System => "system",
            Self::Discussion => "discussion",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "assistant" => Self::Assistant,
            "system" => Self::System,
            "discussion" => Self::Discussion,
            _ => Self::User,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub id: Uuid,
    pub conversation_id: String,
    pub role: ConversationMessageRole,
    pub bunny_user_id: Option<Uuid>,
    pub author_name: Option<String>,
    pub content: String,
    pub external_message_ref: Option<ExternalMessageRef>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalPrompt {
    pub approval_id: Uuid,
    pub session_id: Uuid,
    pub summary: String,
    pub reason: String,
    pub bridge_ctx: BridgeContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChoiceOption {
    pub label: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChoicePrompt {
    pub question: String,
    pub options: Vec<ChoiceOption>,
    pub bridge_ctx: BridgeContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkContext {
    pub user_id: Uuid,
    pub oauth_code: String,
    pub redirect_uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalIdentity {
    pub provider: String,
    pub external_user_id: String,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceBinding {
    pub id: Uuid,
    pub session_id: Uuid,
    pub installation_id: Uuid,
    pub resource_type: String,
    pub resource_ref: String,
    pub config: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionSnapshot {
    pub capabilities: Vec<bunny_policy::Capability>,
    pub synced_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DryRunResult {
    pub ok: bool,
    pub summary: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    pub ok: bool,
    pub summary: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionActivityEntry {
    pub id: Uuid,
    pub session_id: Uuid,
    pub kind: String,
    pub summary: String,
    pub ref_type: Option<String>,
    pub ref_id: Option<String>,
    pub bridge_id: Option<String>,
    pub ts: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposedActionRecord {
    pub id: Uuid,
    pub task_id: Option<Uuid>,
    pub action_id: String,
    pub session_id: Uuid,
    pub requested_by: Uuid,
    pub payload_json: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtendedApprovalRequest {
    pub id: Uuid,
    pub task_id: Uuid,
    pub session_id: Uuid,
    pub proposed_action_id: Option<Uuid>,
    pub action_summary: String,
    pub reason: String,
    pub status: String,
    pub approver_policy_json: Option<String>,
    pub channels_notified: Vec<String>,
    pub resolved_by_user_id: Option<Uuid>,
    pub external_message_id: Option<String>,
    pub source_bridge: Option<String>,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}
