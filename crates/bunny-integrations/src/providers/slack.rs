use crate::chat::{default_command_catalog, ChatBridge};
use crate::types::*;
use async_trait::async_trait;
use std::sync::Arc;
use uuid::Uuid;

use crate::db::IntegrationsDb;

pub struct SlackChatBridge {
    db_path: Arc<String>,
}

impl SlackChatBridge {
    pub fn new(db_path: Arc<String>) -> Self {
        Self { db_path }
    }
}

#[async_trait]
impl ChatBridge for SlackChatBridge {
    fn id(&self) -> &'static str {
        "slack"
    }

    fn display_name(&self) -> &str {
        "Slack"
    }

    fn bridge_auth_config(&self) -> BridgeAuthConfig {
        BridgeAuthConfig {
            bridge_token_hash: None,
        }
    }

    fn command_catalog(&self) -> &[ChatCommandDefinition] {
        static CATALOG: std::sync::OnceLock<Vec<ChatCommandDefinition>> = std::sync::OnceLock::new();
        CATALOG.get_or_init(default_command_catalog).as_slice()
    }

    async fn resolve_user(&self, external_user_id: &str) -> anyhow::Result<Option<Uuid>> {
        let db = IntegrationsDb::open(&self.db_path)?;
        db.get_chat_account_link("slack", external_user_id)
    }
}
