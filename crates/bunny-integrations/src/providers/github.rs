use crate::tool::ToolProvider;
use crate::types::*;
use async_trait::async_trait;
use bunny_policy::{builtin_catalog, ActionDefinition, ProposedAction};

pub struct GitHubProvider {
    catalog: Vec<ActionDefinition>,
}

impl GitHubProvider {
    pub fn new() -> Self {
        let catalog = builtin_catalog()
            .into_iter()
            .filter(|d| d.integration.as_deref() == Some("github"))
            .collect();
        Self { catalog }
    }
}

impl Default for GitHubProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolProvider for GitHubProvider {
    fn id(&self) -> &'static str {
        "github"
    }

    fn display_name(&self) -> &str {
        "GitHub"
    }

    fn oauth_config(&self) -> OAuthConfig {
        OAuthConfig {
            authorize_url: "https://github.com/login/oauth/authorize".into(),
            token_url: "https://github.com/login/oauth/access_token".into(),
            scopes: vec![
                "read:user".into(),
                "repo".into(),
                "read:org".into(),
            ],
        }
    }

    fn action_catalog(&self) -> &[ActionDefinition] {
        &self.catalog
    }

    async fn link_account(&self, ctx: &LinkContext) -> anyhow::Result<ExternalIdentity> {
        let client_id = std::env::var("BUNNY_GITHUB_CLIENT_ID")
            .or_else(|_| std::env::var("GITHUB_CLIENT_ID"))
            .unwrap_or_default();
        let client_secret = std::env::var("BUNNY_GITHUB_CLIENT_SECRET")
            .or_else(|_| std::env::var("GITHUB_CLIENT_SECRET"))
            .unwrap_or_default();
        if client_id.is_empty() || client_secret.is_empty() {
            anyhow::bail!("GitHub OAuth not configured (BUNNY_GITHUB_CLIENT_ID/SECRET)");
        }
        let client = reqwest::Client::new();
        let resp = client
            .post("https://github.com/login/oauth/access_token")
            .header("Accept", "application/json")
            .form(&[
                ("client_id", client_id.as_str()),
                ("client_secret", client_secret.as_str()),
                ("code", ctx.oauth_code.as_str()),
                ("redirect_uri", ctx.redirect_uri.as_str()),
            ])
            .send()
            .await?;
        let token: serde_json::Value = resp.json().await?;
        let access = token["access_token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("no access_token from GitHub"))?;
        let user_resp = client
            .get("https://api.github.com/user")
            .header("Authorization", format!("Bearer {access}"))
            .header("User-Agent", "bunny-agent")
            .send()
            .await?;
        let user: serde_json::Value = user_resp.json().await?;
        Ok(ExternalIdentity {
            provider: "github".into(),
            external_user_id: user["id"].to_string(),
            username: user["login"].as_str().map(str::to_string),
            display_name: user["name"].as_str().map(str::to_string),
            email: user["email"].as_str().map(str::to_string),
        })
    }

    async fn sync_permissions(
        &self,
        binding: &ResourceBinding,
        creds: &Credentials,
    ) -> anyhow::Result<PermissionSnapshot> {
        use bunny_policy::Capability;
        use chrono::{Duration, Utc};
        use std::collections::HashSet;

        let client = reqwest::Client::new();
        let repo = binding.resource_ref.strip_prefix("repo:").unwrap_or(&binding.resource_ref);
        let url = format!("https://api.github.com/repos/{repo}/collaborators/{{username}}/permission");
        // Fallback: check repo access via /repos/{owner}/{repo}
        let check_url = format!("https://api.github.com/repos/{repo}");
        let resp = client
            .get(&check_url)
            .header("Authorization", format!("Bearer {}", creds.access_token))
            .header("User-Agent", "bunny-agent")
            .send()
            .await?;
        let mut caps = HashSet::new();
        if resp.status().is_success() {
            caps.insert(Capability::Read);
            caps.insert(Capability::Write);
            let perm_resp = client
                .get(url.replace("{username}", ""))
                .header("Authorization", format!("Bearer {}", creds.access_token))
                .header("User-Agent", "bunny-agent")
                .send()
                .await;
            if let Ok(p) = perm_resp {
                if let Ok(body) = p.json::<serde_json::Value>().await {
                    if body["permission"].as_str() == Some("admin") {
                        caps.insert(Capability::Admin);
                        caps.insert(Capability::Merge);
                    }
                }
            }
        }
        let now = Utc::now();
        Ok(PermissionSnapshot {
            capabilities: caps.into_iter().collect(),
            synced_at: now,
            expires_at: now + Duration::minutes(30),
        })
    }

    async fn dry_run(&self, action: &ProposedAction, _creds: &Credentials) -> anyhow::Result<DryRunResult> {
        Ok(DryRunResult {
            ok: true,
            summary: format!("would execute {}", action.action_id.0),
            warnings: vec![],
        })
    }

    async fn execute(&self, action: &ProposedAction, creds: &Credentials) -> anyhow::Result<ActionResult> {
        let client = reqwest::Client::new();
        let id = &action.action_id.0;
        if id == "github:issue.create" {
            let repo = action
                .resource_ref
                .as_ref()
                .map(|r| r.resource_id.clone())
                .unwrap_or_default();
            let title = action.payload["title"].as_str().unwrap_or("Bunny agent issue");
            let body = action.payload["body"].as_str().unwrap_or("");
            let resp = client
                .post(format!("https://api.github.com/repos/{repo}/issues"))
                .header("Authorization", format!("Bearer {}", creds.access_token))
                .header("User-Agent", "bunny-agent")
                .json(&serde_json::json!({ "title": title, "body": body }))
                .send()
                .await?;
            let status = resp.status();
            let data: serde_json::Value = resp.json().await?;
            return Ok(ActionResult {
                ok: status.is_success(),
                summary: format!("issue #{}", data["number"]),
                data,
            });
        }
        Ok(ActionResult {
            ok: false,
            summary: format!("unsupported action {id}"),
            data: serde_json::json!({}),
        })
    }
}
