use crate::chat::{default_command_catalog, ChatBridge};
use crate::types::*;
use async_trait::async_trait;
use std::sync::Arc;
use uuid::Uuid;

use crate::db::IntegrationsDb;

pub struct TeamsChatBridge {
    db_path: Arc<String>,
}

impl TeamsChatBridge {
    pub fn new(db_path: Arc<String>) -> Self {
        Self { db_path }
    }
}

#[async_trait]
impl ChatBridge for TeamsChatBridge {
    fn id(&self) -> &'static str {
        "teams"
    }

    fn display_name(&self) -> &str {
        "Microsoft Teams"
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
        db.get_chat_account_link("teams", external_user_id)
    }
}
