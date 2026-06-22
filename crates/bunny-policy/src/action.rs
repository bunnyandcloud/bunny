use crate::types::*;
use bunny_core::permissions::Action as BunnyAction;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionDefinition {
    pub id: ActionId,
    pub integration: Option<String>,
    pub domain: ActionDomain,
    pub verb: ActionVerb,
    pub default_risk: RiskLevel,
    pub approval_mode: ApprovalMode,
    pub required_external_caps: Vec<Capability>,
    pub required_bunny_action: BunnyAction,
    pub approver_policy: Option<ApproverPolicy>,
}

impl ActionDefinition {
    pub fn shell_run(cmd: &str) -> Self {
        let risk = crate::risk::classify_shell_risk(cmd);
        Self {
            id: ActionId(format!("shell:{}", risk.as_str())),
            integration: None,
            domain: ActionDomain::Shell,
            verb: ActionVerb::Execute,
            default_risk: risk,
            approval_mode: if risk == RiskLevel::Safe {
                ApprovalMode::Auto
            } else {
                ApprovalMode::Required
            },
            required_external_caps: vec![],
            required_bunny_action: BunnyAction::TerminalWrite,
            approver_policy: Some(ApproverPolicy::default_for_risk(risk)),
        }
    }
}

impl RiskLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

pub fn builtin_catalog() -> Vec<ActionDefinition> {
    vec![
        ActionDefinition {
            id: ActionId("github:pr.create".into()),
            integration: Some("github".into()),
            domain: ActionDomain::Git,
            verb: ActionVerb::Create,
            default_risk: RiskLevel::Medium,
            approval_mode: ApprovalMode::Required,
            required_external_caps: vec![Capability::Write],
            required_bunny_action: BunnyAction::ActionExecute,
            approver_policy: Some(ApproverPolicy::CompositeAny {
                policies: vec![
                    ApproverPolicy::MinBunnyRole {
                        role: bunny_core::types::Role::Editor,
                    },
                    ApproverPolicy::RequesterCannotSelfApprove,
                ],
            }),
        },
        ActionDefinition {
            id: ActionId("github:pr.merge".into()),
            integration: Some("github".into()),
            domain: ActionDomain::Git,
            verb: ActionVerb::Merge,
            default_risk: RiskLevel::High,
            approval_mode: ApprovalMode::Required,
            required_external_caps: vec![Capability::Merge],
            required_bunny_action: BunnyAction::ActionExecute,
            approver_policy: Some(ApproverPolicy::CompositeAny {
                policies: vec![
                    ApproverPolicy::MinBunnyRole {
                        role: bunny_core::types::Role::Admin,
                    },
                    ApproverPolicy::ExternalCapability {
                        capability: Capability::Admin,
                    },
                ],
            }),
        },
        ActionDefinition {
            id: ActionId("github:push".into()),
            integration: Some("github".into()),
            domain: ActionDomain::Git,
            verb: ActionVerb::Push,
            default_risk: RiskLevel::Medium,
            approval_mode: ApprovalMode::Required,
            required_external_caps: vec![Capability::Write],
            required_bunny_action: BunnyAction::ActionExecute,
            approver_policy: Some(ApproverPolicy::MinBunnyRole {
                role: bunny_core::types::Role::Editor,
            }),
        },
        ActionDefinition {
            id: ActionId("github:issue.create".into()),
            integration: Some("github".into()),
            domain: ActionDomain::Issues,
            verb: ActionVerb::Create,
            default_risk: RiskLevel::Safe,
            approval_mode: ApprovalMode::Auto,
            required_external_caps: vec![Capability::Write],
            required_bunny_action: BunnyAction::ActionExecute,
            approver_policy: None,
        },
        ActionDefinition {
            id: ActionId("gitlab:mr.merge".into()),
            integration: Some("gitlab".into()),
            domain: ActionDomain::Git,
            verb: ActionVerb::Merge,
            default_risk: RiskLevel::High,
            approval_mode: ApprovalMode::Required,
            required_external_caps: vec![Capability::Merge],
            required_bunny_action: BunnyAction::ActionExecute,
            approver_policy: Some(ApproverPolicy::MinBunnyRole {
                role: bunny_core::types::Role::Admin,
            }),
        },
        ActionDefinition {
            id: ActionId("jira:issue.transition".into()),
            integration: Some("jira".into()),
            domain: ActionDomain::Issues,
            verb: ActionVerb::Update,
            default_risk: RiskLevel::Medium,
            approval_mode: ApprovalMode::Required,
            required_external_caps: vec![Capability::Transition],
            required_bunny_action: BunnyAction::ActionExecute,
            approver_policy: Some(ApproverPolicy::MinBunnyRole {
                role: bunny_core::types::Role::Editor,
            }),
        },
        ActionDefinition {
            id: ActionId("linear:issue.create".into()),
            integration: Some("linear".into()),
            domain: ActionDomain::Issues,
            verb: ActionVerb::Create,
            default_risk: RiskLevel::Safe,
            approval_mode: ApprovalMode::Auto,
            required_external_caps: vec![Capability::Write],
            required_bunny_action: BunnyAction::ActionExecute,
            approver_policy: None,
        },
        ActionDefinition {
            id: ActionId("supabase:migration.apply".into()),
            integration: Some("supabase".into()),
            domain: ActionDomain::Deploy,
            verb: ActionVerb::Execute,
            default_risk: RiskLevel::Critical,
            approval_mode: ApprovalMode::OwnerOnly,
            required_external_caps: vec![Capability::Admin],
            required_bunny_action: BunnyAction::ActionExecute,
            approver_policy: Some(ApproverPolicy::MinBunnyRole {
                role: bunny_core::types::Role::Owner,
            }),
        },
        ActionDefinition {
            id: ActionId("git:worktree.manage".into()),
            integration: None,
            domain: ActionDomain::Git,
            verb: ActionVerb::Execute,
            default_risk: RiskLevel::Safe,
            approval_mode: ApprovalMode::Auto,
            required_external_caps: vec![],
            required_bunny_action: BunnyAction::GitWorktreeManage,
            approver_policy: None,
        },
    ]
}
