use crate::tool::ToolProvider;
use crate::types::*;
use async_trait::async_trait;
use bunny_policy::{builtin_catalog, ActionDefinition, ProposedAction};
use chrono::{Duration, Utc};

pub struct SupabaseProvider {
    catalog: Vec<ActionDefinition>,
}

impl SupabaseProvider {
    pub fn new() -> Self {
        let catalog = builtin_catalog()
            .into_iter()
            .filter(|d| d.integration.as_deref() == Some("supabase"))
            .collect();
        Self { catalog }
    }
}

impl Default for SupabaseProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolProvider for SupabaseProvider {
    fn id(&self) -> &'static str {
        "supabase"
    }

    fn display_name(&self) -> &str {
        "Supabase"
    }

    fn oauth_config(&self) -> OAuthConfig {
        OAuthConfig {
            authorize_url: "https://api.supabase.com/v1/oauth/authorize".into(),
            token_url: "https://api.supabase.com/v1/oauth/token".into(),
            scopes: vec!["projects:read".into()],
        }
    }

    fn action_catalog(&self) -> &[ActionDefinition] {
        &self.catalog
    }

    async fn link_account(&self, ctx: &LinkContext) -> anyhow::Result<ExternalIdentity> {
        Ok(ExternalIdentity {
            provider: "supabase".into(),
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
            capabilities: vec![Capability::Read],
            synced_at: now,
            expires_at: now + Duration::minutes(30),
        })
    }

    async fn dry_run(&self, action: &ProposedAction, _creds: &Credentials) -> anyhow::Result<DryRunResult> {
        Ok(DryRunResult {
            ok: true,
            summary: format!("supabase dry-run {}", action.action_id.0),
            warnings: vec![],
        })
    }

    async fn execute(&self, action: &ProposedAction, _creds: &Credentials) -> anyhow::Result<ActionResult> {
        Ok(ActionResult {
            ok: false,
            summary: format!("supabase execute not yet wired: {}", action.action_id.0),
            data: serde_json::json!({}),
        })
    }
}
