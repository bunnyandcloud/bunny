use bunny_core::types::Role;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ActionId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionDomain {
    Git,
    Issues,
    Deploy,
    Shell,
    Secrets,
    Integration,
    Chat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionVerb {
    Read,
    Create,
    Update,
    Delete,
    Merge,
    Push,
    Execute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Safe,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    Auto,
    Required,
    OwnerOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyDecision {
    Allow,
    Deny,
    NeedsApproval,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Read,
    Write,
    Admin,
    Merge,
    Delete,
    Transition,
    Deploy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRef {
    pub provider: String,
    pub resource_type: String,
    pub resource_id: String,
}

impl ResourceRef {
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.splitn(3, ':').collect();
        if parts.len() == 2 {
            return Some(Self {
                provider: String::new(),
                resource_type: parts[0].to_string(),
                resource_id: parts[1].to_string(),
            });
        }
        if parts.len() == 3 {
            return Some(Self {
                provider: parts[0].to_string(),
                resource_type: parts[1].to_string(),
                resource_id: parts[2].to_string(),
            });
        }
        None
    }

    pub fn to_key(&self) -> String {
        if self.provider.is_empty() {
            format!("{}:{}", self.resource_type, self.resource_id)
        } else {
            format!(
                "{}:{}:{}",
                self.provider, self.resource_type, self.resource_id
            )
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalPermission {
    pub integration_id: String,
    pub external_user_id: String,
    pub resource_ref: ResourceRef,
    pub capabilities: HashSet<Capability>,
    pub synced_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyActor {
    pub user_id: Uuid,
    pub session_id: Uuid,
    pub role: Role,
    pub external_permissions: Vec<ExternalPermission>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ApproverPolicy {
    MinBunnyRole { role: Role },
    ExternalCapability { capability: Capability },
    RequesterCannotSelfApprove,
    CompositeAny { policies: Vec<ApproverPolicy> },
    CompositeAll { policies: Vec<ApproverPolicy> },
}

impl ApproverPolicy {
    pub fn default_for_risk(risk: RiskLevel) -> Self {
        match risk {
            RiskLevel::Safe => ApproverPolicy::MinBunnyRole { role: Role::Editor },
            RiskLevel::Medium => ApproverPolicy::CompositeAny {
                policies: vec![
                    ApproverPolicy::MinBunnyRole { role: Role::Admin },
                    ApproverPolicy::RequesterCannotSelfApprove,
                ],
            },
            RiskLevel::High | RiskLevel::Critical => ApproverPolicy::CompositeAll {
                policies: vec![
                    ApproverPolicy::MinBunnyRole { role: Role::Admin },
                    ApproverPolicy::RequesterCannotSelfApprove,
                ],
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposedAction {
    pub id: Uuid,
    pub action_id: ActionId,
    pub session_id: Uuid,
    pub requested_by: Uuid,
    pub payload: serde_json::Value,
    pub resource_ref: Option<ResourceRef>,
    pub shell_command: Option<String>,
    pub source_bridge: Option<String>,
    pub source_conversation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyEvaluation {
    pub decision: PolicyDecision,
    pub reason: String,
    pub approver_policy: Option<ApproverPolicy>,
    pub risk: RiskLevel,
}

pub fn role_at_least(role: Role, min: Role) -> bool {
    use Role::*;
    let rank = |r: Role| match r {
        Viewer => 0,
        Agent => 1,
        Editor => 2,
        Admin => 3,
        Owner => 4,
    };
    rank(role) >= rank(min)
}
