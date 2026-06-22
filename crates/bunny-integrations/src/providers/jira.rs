use crate::tool::ToolProvider;
use crate::types::*;
use async_trait::async_trait;
use bunny_policy::{builtin_catalog, ActionDefinition, ProposedAction};
use chrono::{Duration, Utc};

pub struct JiraProvider {
    catalog: Vec<ActionDefinition>,
}

impl JiraProvider {
    pub fn new() -> Self {
        let catalog = builtin_catalog()
            .into_iter()
            .filter(|d| d.integration.as_deref() == Some("jira"))
            .collect();
        Self { catalog }
    }
}

impl Default for JiraProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolProvider for JiraProvider {
    fn id(&self) -> &'static str {
        "jira"
    }

    fn display_name(&self) -> &str {
        "Jira"
    }

    fn oauth_config(&self) -> OAuthConfig {
        OAuthConfig {
            authorize_url: "https://auth.atlassian.com/authorize".into(),
            token_url: "https://auth.atlassian.com/oauth/token".into(),
            scopes: vec![
                "read:jira-work".into(),
                "write:jira-work".into(),
                "offline_access".into(),
            ],
        }
    }

    fn action_catalog(&self) -> &[ActionDefinition] {
        &self.catalog
    }

    async fn link_account(&self, ctx: &LinkContext) -> anyhow::Result<ExternalIdentity> {
        Ok(ExternalIdentity {
            provider: "jira".into(),
            external_user_id: ctx.oauth_code.clone(),
            username: None,
            display_name: None,
            email: None,
        })
    }

    async fn sync_permissions(
        &self,
        _binding: &ResourceBinding,
        _creds: &Credentials,
    ) -> anyhow::Result<PermissionSnapshot> {
        use bunny_policy::Capability;
        let now = Utc::now();
        Ok(PermissionSnapshot {
            capabilities: vec![Capability::Read, Capability::Transition],
            synced_at: now,
            expires_at: now + Duration::minutes(30),
        })
    }

    async fn dry_run(&self, action: &ProposedAction, _creds: &Credentials) -> anyhow::Result<DryRunResult> {
        Ok(DryRunResult {
            ok: true,
            summary: format!("jira dry-run {}", action.action_id.0),
            warnings: vec![],
        })
    }

    async fn execute(&self, action: &ProposedAction, _creds: &Credentials) -> anyhow::Result<ActionResult> {
        Ok(ActionResult {
            ok: false,
            summary: format!("jira execute not yet wired: {}", action.action_id.0),
            data: serde_json::json!({}),
        })
    }
}
