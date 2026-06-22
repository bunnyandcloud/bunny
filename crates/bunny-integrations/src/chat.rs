use crate::types::*;
use async_trait::async_trait;
use uuid::Uuid;

#[async_trait]
pub trait ChatBridge: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &str;
    fn bridge_auth_config(&self) -> BridgeAuthConfig;
    fn command_catalog(&self) -> &[ChatCommandDefinition];

    async fn resolve_user(&self, external_user_id: &str) -> anyhow::Result<Option<Uuid>>;

    async fn link_workspace(&self, _req: LinkWorkspaceRequest) -> anyhow::Result<ChatSessionLink> {
        anyhow::bail!("link_workspace not implemented for {}", self.id())
    }

    async fn bind_conversation(
        &self,
        _req: ConversationBindRequest,
    ) -> anyhow::Result<ConversationBinding> {
        anyhow::bail!("bind_conversation not implemented for {}", self.id())
    }

    async fn send_agent_message(
        &self,
        _ctx: &ConversationContext,
        _msg: &AgentMessage,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn send_approval_prompt(
        &self,
        _req: &ApprovalPrompt,
    ) -> anyhow::Result<ExternalMessageRef> {
        anyhow::bail!("send_approval_prompt not implemented for {}", self.id())
    }

    async fn send_choice_prompt(&self, _req: &ChoicePrompt) -> anyhow::Result<ExternalMessageRef> {
        anyhow::bail!("send_choice_prompt not implemented for {}", self.id())
    }

    async fn update_message(
        &self,
        _ref: &ExternalMessageRef,
        _content: &str,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

pub fn default_command_catalog() -> Vec<ChatCommandDefinition> {
    use bunny_core::permissions::Action;
    vec![
        ChatCommandDefinition {
            name: "ask".into(),
            description: "Read-only agent guidance".into(),
            required_bunny_action: Action::DiscordAgentRun,
        },
        ChatCommandDefinition {
            name: "plan".into(),
            description: "Plan without executing".into(),
            required_bunny_action: Action::DiscordAgentRun,
        },
        ChatCommandDefinition {
            name: "do".into(),
            description: "Execute agent task".into(),
            required_bunny_action: Action::DiscordAgentRun,
        },
        ChatCommandDefinition {
            name: "run".into(),
            description: "Run shell command".into(),
            required_bunny_action: Action::DiscordAgentRun,
        },
        ChatCommandDefinition {
            name: "git".into(),
            description: "Git operations".into(),
            required_bunny_action: Action::DiscordAgentRun,
        },
        ChatCommandDefinition {
            name: "link".into(),
            description: "Link channel to session".into(),
            required_bunny_action: Action::DiscordLink,
        },
        ChatCommandDefinition {
            name: "project".into(),
            description: "Set project directory".into(),
            required_bunny_action: Action::DiscordControl,
        },
        ChatCommandDefinition {
            name: "watch".into(),
            description: "Share watch link".into(),
            required_bunny_action: Action::DiscordWatch,
        },
    ]
}
