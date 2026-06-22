pub mod discord;
pub mod github;
pub mod gitlab;
pub mod jira;
pub mod linear;
pub mod slack;
pub mod supabase;
pub mod teams;

pub use discord::DiscordChatBridge;
pub use github::GitHubProvider;
pub use gitlab::GitLabProvider;
pub use jira::JiraProvider;
pub use linear::LinearProvider;
pub use slack::SlackChatBridge;
pub use supabase::SupabaseProvider;
pub use teams::TeamsChatBridge;

use crate::registry::IntegrationRegistry;
use crate::{ChatBridgeHub, GitWorkspaceManager};
use std::path::Path;
use std::sync::Arc;

pub fn build_registry(db_path: Arc<String>) -> IntegrationRegistry {
    let mut registry = IntegrationRegistry::new();
    registry.register_chat_bridge(Arc::new(DiscordChatBridge::new(db_path.clone())));
    registry.register_chat_bridge(Arc::new(SlackChatBridge::new(db_path.clone())));
    registry.register_chat_bridge(Arc::new(TeamsChatBridge::new(db_path)));
    registry.register_tool_provider(Arc::new(GitHubProvider::new()));
    registry.register_tool_provider(Arc::new(GitLabProvider::new()));
    registry.register_tool_provider(Arc::new(JiraProvider::new()));
    registry.register_tool_provider(Arc::new(LinearProvider::new()));
    registry.register_tool_provider(Arc::new(SupabaseProvider::new()));
    registry
}

pub fn build_hub(db_path: Arc<String>) -> ChatBridgeHub {
    ChatBridgeHub::new(build_registry(db_path))
}

pub fn build_git_manager(data_dir: impl AsRef<Path>, db_path: impl Into<String>) -> GitWorkspaceManager {
    GitWorkspaceManager::new(data_dir, db_path)
}
