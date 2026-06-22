use crate::chat::ChatBridge;
use crate::tool::ToolProvider;
use std::collections::HashMap;
use std::sync::Arc;

pub struct IntegrationRegistry {
    chat_bridges: HashMap<String, Arc<dyn ChatBridge>>,
    tool_providers: HashMap<String, Arc<dyn ToolProvider>>,
}

impl Default for IntegrationRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl IntegrationRegistry {
    pub fn new() -> Self {
        Self {
            chat_bridges: HashMap::new(),
            tool_providers: HashMap::new(),
        }
    }

    pub fn register_chat_bridge(&mut self, bridge: Arc<dyn ChatBridge>) {
        self.chat_bridges.insert(bridge.id().to_string(), bridge);
    }

    pub fn register_tool_provider(&mut self, provider: Arc<dyn ToolProvider>) {
        self.tool_providers
            .insert(provider.id().to_string(), provider);
    }

    pub fn chat_bridge(&self, id: &str) -> Option<Arc<dyn ChatBridge>> {
        self.chat_bridges.get(id).cloned()
    }

    pub fn tool_provider(&self, id: &str) -> Option<Arc<dyn ToolProvider>> {
        self.tool_providers.get(id).cloned()
    }

    pub fn list_chat_bridges(&self) -> Vec<String> {
        self.chat_bridges.keys().cloned().collect()
    }

    pub fn list_tool_providers(&self) -> Vec<String> {
        self.tool_providers.keys().cloned().collect()
    }
}

pub struct ChatBridgeHub {
    registry: IntegrationRegistry,
}

impl ChatBridgeHub {
    pub fn new(registry: IntegrationRegistry) -> Self {
        Self { registry }
    }

    pub fn registry(&self) -> &IntegrationRegistry {
        &self.registry
    }

    pub fn bridge(&self, id: &str) -> Option<Arc<dyn ChatBridge>> {
        self.registry.chat_bridge(id)
    }
}
