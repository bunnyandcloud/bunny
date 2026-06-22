use crate::types::*;
use bunny_core::types::Role;

pub fn can_approve(
    approver: &PolicyActor,
    requester_id: uuid::Uuid,
    policy: &ApproverPolicy,
    resource: Option<&ResourceRef>,
) -> bool {
    match policy {
        ApproverPolicy::MinBunnyRole { role } => role_at_least(approver.role, *role),
        ApproverPolicy::ExternalCapability { capability } => {
            if let Some(res) = resource {
                approver.external_permissions.iter().any(|p| {
                    p.resource_ref.to_key() == res.to_key() && p.capabilities.contains(capability)
                })
            } else {
                false
            }
        }
        ApproverPolicy::RequesterCannotSelfApprove => approver.user_id != requester_id,
        ApproverPolicy::CompositeAny { policies } => policies
            .iter()
            .any(|p| can_approve(approver, requester_id, p, resource)),
        ApproverPolicy::CompositeAll { policies } => {
            let non_self = policies
                .iter()
                .any(|p| matches!(p, ApproverPolicy::RequesterCannotSelfApprove));
            if non_self && approver.user_id == requester_id {
                return false;
            }
            policies
                .iter()
                .filter(|p| !matches!(p, ApproverPolicy::RequesterCannotSelfApprove))
                .all(|p| can_approve(approver, requester_id, p, resource))
        }
    }
}

pub fn min_role_label(role: Role) -> &'static str {
    match role {
        Role::Owner => "owner",
        Role::Admin => "admin",
        Role::Editor => "editor",
        Role::Viewer => "viewer",
        Role::Agent => "agent",
    }
}
