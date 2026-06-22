use crate::types::*;
use async_trait::async_trait;
use bunny_policy::{ActionDefinition, ProposedAction};

#[async_trait]
pub trait ToolProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &str;
    fn oauth_config(&self) -> OAuthConfig;
    fn action_catalog(&self) -> &[ActionDefinition];

    async fn link_account(&self, ctx: &LinkContext) -> anyhow::Result<ExternalIdentity>;

    async fn sync_permissions(
        &self,
        binding: &ResourceBinding,
        creds: &Credentials,
    ) -> anyhow::Result<PermissionSnapshot>;

    async fn dry_run(&self, action: &ProposedAction, creds: &Credentials) -> anyhow::Result<DryRunResult>;

    async fn execute(
        &self,
        action: &ProposedAction,
        creds: &Credentials,
    ) -> anyhow::Result<ActionResult>;
}
