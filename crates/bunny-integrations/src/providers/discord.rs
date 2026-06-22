use crate::chat::{default_command_catalog, ChatBridge};
use crate::types::*;
use async_trait::async_trait;
use std::sync::Arc;
use uuid::Uuid;

pub struct DiscordChatBridge {
    db_path: Arc<String>,
}

use crate::db::IntegrationsDb;

impl DiscordChatBridge {
    pub fn new(db_path: Arc<String>) -> Self {
        Self { db_path }
    }
}

#[async_trait]
impl ChatBridge for DiscordChatBridge {
    fn id(&self) -> &'static str {
        "discord"
    }

    fn display_name(&self) -> &str {
        "Discord"
    }

    fn bridge_auth_config(&self) -> BridgeAuthConfig {
        BridgeAuthConfig {
            bridge_token_hash: None,
        }
    }

    fn command_catalog(&self) -> &[ChatCommandDefinition] {
        // Stored in static — use leak for simplicity in trait
        static CATALOG: std::sync::OnceLock<Vec<ChatCommandDefinition>> = std::sync::OnceLock::new();
        CATALOG.get_or_init(default_command_catalog).as_slice()
    }

    async fn resolve_user(&self, external_user_id: &str) -> anyhow::Result<Option<Uuid>> {
        let db = IntegrationsDb::open(&self.db_path)?;
        db.get_chat_account_link("discord", external_user_id)
    }
}
