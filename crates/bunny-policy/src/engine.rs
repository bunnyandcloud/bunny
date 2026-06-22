use crate::action::{builtin_catalog, ActionDefinition};
use crate::approver;
use crate::types::*;
use bunny_core::permissions::role_can;
use std::collections::HashMap;

pub struct PolicyEngine {
    catalog: HashMap<String, ActionDefinition>,
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl PolicyEngine {
    pub fn new() -> Self {
        let mut catalog = HashMap::new();
        for def in builtin_catalog() {
            catalog.insert(def.id.0.clone(), def);
        }
        Self { catalog }
    }

    pub fn register(&mut self, def: ActionDefinition) {
        self.catalog.insert(def.id.0.clone(), def);
    }

    pub fn lookup(&self, action_id: &str) -> Option<&ActionDefinition> {
        self.catalog.get(action_id)
    }

    pub fn evaluate(&self, actor: &PolicyActor, action: &ProposedAction) -> PolicyEvaluation {
        if let Some(cmd) = &action.shell_command {
            let def = ActionDefinition::shell_run(cmd);
            return self.evaluate_definition(actor, action, &def);
        }

        let Some(def) = self.lookup(&action.action_id.0) else {
            return PolicyEvaluation {
                decision: PolicyDecision::Deny,
                reason: format!("unknown action: {}", action.action_id.0),
                approver_policy: None,
                risk: RiskLevel::High,
            };
        };
        self.evaluate_definition(actor, action, def)
    }

    fn evaluate_definition(
        &self,
        actor: &PolicyActor,
        action: &ProposedAction,
        def: &ActionDefinition,
    ) -> PolicyEvaluation {
        if !role_can(actor.role, def.required_bunny_action) {
            return PolicyEvaluation {
                decision: PolicyDecision::Deny,
                reason: format!(
                    "bunny role {:?} cannot {:?}",
                    actor.role, def.required_bunny_action
                ),
                approver_policy: None,
                risk: def.default_risk,
            };
        }

        if let Some(ref resource) = action.resource_ref {
            if !def.required_external_caps.is_empty()
                && !self.has_external_caps(actor, resource, &def.required_external_caps)
            {
                return PolicyEvaluation {
                    decision: PolicyDecision::Deny,
                    reason: "missing external capabilities".into(),
                    approver_policy: None,
                    risk: def.default_risk,
                };
            }
        }

        let approver_policy = def
            .approver_policy
            .clone()
            .or_else(|| Some(ApproverPolicy::default_for_risk(def.default_risk)));

        match def.approval_mode {
            ApprovalMode::Auto if def.default_risk == RiskLevel::Safe => PolicyEvaluation {
                decision: PolicyDecision::Allow,
                reason: "auto-approved".into(),
                approver_policy,
                risk: def.default_risk,
            },
            ApprovalMode::OwnerOnly => PolicyEvaluation {
                decision: if actor.role == bunny_core::types::Role::Owner {
                    PolicyDecision::NeedsApproval
                } else {
                    PolicyDecision::Deny
                },
                reason: "owner-only action".into(),
                approver_policy,
                risk: def.default_risk,
            },
            _ => PolicyEvaluation {
                decision: if def.default_risk == RiskLevel::Safe {
                    PolicyDecision::Allow
                } else {
                    PolicyDecision::NeedsApproval
                },
                reason: "approval required by policy".into(),
                approver_policy,
                risk: def.default_risk,
            },
        }
    }

    fn has_external_caps(
        &self,
        actor: &PolicyActor,
        resource: &ResourceRef,
        required: &[Capability],
    ) -> bool {
        actor.external_permissions.iter().any(|perm| {
            perm.resource_ref.to_key() == resource.to_key()
                && required.iter().all(|c| perm.capabilities.contains(c))
        })
    }

    pub fn can_user_approve(
        &self,
        approver: &PolicyActor,
        requester_id: uuid::Uuid,
        policy: &ApproverPolicy,
        resource: Option<&ResourceRef>,
    ) -> bool {
        approver::can_approve(approver, requester_id, policy, resource)
    }

    pub fn shell_needs_approval(&self, cmd: &str) -> bool {
        crate::risk::requires_approval(cmd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bunny_core::types::Role;
    use std::collections::HashSet;
    use uuid::Uuid;

    fn actor(role: Role) -> PolicyActor {
        PolicyActor {
            user_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            role,
            external_permissions: vec![],
        }
    }

    #[test]
    fn editor_can_auto_read_github_issue() {
        let engine = PolicyEngine::new();
        let a = actor(Role::Editor);
        let action = ProposedAction {
            id: Uuid::new_v4(),
            action_id: ActionId("github:issue.create".into()),
            session_id: a.session_id,
            requested_by: a.user_id,
            payload: serde_json::json!({}),
            resource_ref: Some(ResourceRef {
                provider: "github".into(),
                resource_type: "repo".into(),
                resource_id: "acme/app".into(),
            }),
            shell_command: None,
            source_bridge: None,
            source_conversation_id: None,
        };
        let mut a_with_caps = a.clone();
        a_with_caps.external_permissions.push(ExternalPermission {
            integration_id: "github".into(),
            external_user_id: "1".into(),
            resource_ref: action.resource_ref.clone().unwrap(),
            capabilities: HashSet::from([Capability::Write]),
            synced_at: chrono::Utc::now(),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
        });
        let ev = engine.evaluate(&a_with_caps, &action);
        assert_eq!(ev.decision, PolicyDecision::Allow);
    }

    #[test]
    fn viewer_denied_terminal_write_shell() {
        let engine = PolicyEngine::new();
        let a = actor(Role::Viewer);
        let action = ProposedAction {
            id: Uuid::new_v4(),
            action_id: ActionId("shell:safe".into()),
            session_id: a.session_id,
            requested_by: a.user_id,
            payload: serde_json::json!({}),
            resource_ref: None,
            shell_command: Some("ls".into()),
            source_bridge: None,
            source_conversation_id: None,
        };
        let ev = engine.evaluate(&a, &action);
        assert_eq!(ev.decision, PolicyDecision::Deny);
    }
}
