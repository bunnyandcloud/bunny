//! Unified approval service — channel-agnostic policy checks and notifications.

use crate::api::ApiError;
use crate::state::AppState;
use crate::task_runner::{self, ApprovalResolveOutcome};
use bunny_policy::{
    ApproverPolicy, PolicyActor, PolicyDecision, PolicyEngine, ProposedAction,
};
use bunny_integrations::{SessionActivityEntry, ProposedActionRecord};
use chrono::Utc;
use serde_json;
use std::sync::Arc;
use uuid::Uuid;

pub struct ApprovalService;

impl ApprovalService {
    pub fn policy_engine() -> PolicyEngine {
        PolicyEngine::new()
    }

    pub fn build_actor(state: &AppState, session_id: Uuid, user_id: Uuid) -> Result<PolicyActor, ApiError> {
        let role = state
            .auth
            .member_role(session_id, user_id)?
            .ok_or_else(|| ApiError::forbidden("not a session member"))?;
        Ok(PolicyActor {
            user_id,
            session_id,
            role,
            external_permissions: vec![],
        })
    }

    pub fn evaluate_shell(
        state: &AppState,
        session_id: Uuid,
        user_id: Uuid,
        command: &str,
    ) -> Result<bunny_policy::PolicyEvaluation, ApiError> {
        let actor = Self::build_actor(state, session_id, user_id)?;
        let action = ProposedAction {
            id: Uuid::new_v4(),
            action_id: bunny_policy::ActionId(format!(
                "shell:{}",
                bunny_policy::classify_shell_risk(command).as_str()
            )),
            session_id,
            requested_by: user_id,
            payload: serde_json::json!({ "command": command }),
            resource_ref: None,
            shell_command: Some(command.to_string()),
            source_bridge: None,
            source_conversation_id: None,
        };
        Ok(Self::policy_engine().evaluate(&actor, &action))
    }

    pub fn shell_needs_approval(state: &AppState, session_id: Uuid, user_id: Uuid, command: &str) -> bool {
        Self::evaluate_shell(state, session_id, user_id, command)
            .map(|e| e.decision == PolicyDecision::NeedsApproval)
            .unwrap_or_else(|_| bunny_policy::requires_approval(command))
    }

    pub fn can_user_approve(
        state: &AppState,
        session_id: Uuid,
        approver_id: Uuid,
        requester_id: Uuid,
        policy: &ApproverPolicy,
    ) -> Result<bool, ApiError> {
        let approver = Self::build_actor(state, session_id, approver_id)?;
        Ok(PolicyEngine::new().can_user_approve(
            &approver,
            requester_id,
            policy,
            None,
        ))
    }

    pub fn resolve(
        state: &AppState,
        approval_id: Uuid,
        approve: bool,
        bunny_user_id: Uuid,
    ) -> Result<ApprovalResolveOutcome, ApiError> {
        let approval = state
            .discord
            .lock()
            .get_approval(approval_id)?
            .ok_or_else(|| ApiError::not_found("approval not found"))?;

        if let Some(ref policy_json) = state
            .integrations
            .lock()
            .get_approval_policy(approval_id)
            .ok()
            .flatten()
        {
            if let Ok(policy) = serde_json::from_str::<ApproverPolicy>(policy_json) {
                let requester = state
                    .discord
                    .lock()
                    .get_task(approval.task_id)?
                    .and_then(|t| t.requested_by_user_id)
                    .unwrap_or(Uuid::nil());
                if !Self::can_user_approve(state, approval.session_id, bunny_user_id, requester, &policy)? {
                    return Err(ApiError::forbidden("cannot approve this action"));
                }
            }
        } else {
            let role = state
                .auth
                .member_role(approval.session_id, bunny_user_id)?
                .ok_or_else(|| ApiError::forbidden("not a member"))?;
            if !bunny_core::permissions::role_can(role, bunny_core::permissions::Action::ActionApprove)
                && !bunny_core::permissions::role_can(role, bunny_core::permissions::Action::DiscordApprove)
            {
                return Err(ApiError::forbidden("cannot approve"));
            }
        }

        let outcome = task_runner::resolve_approval(state, approval_id, approve, bunny_user_id)
            .map_err(|e| ApiError::validation(&e.to_string()))?;

        if approve {
            let _ = state.integrations.lock().record_activity(&SessionActivityEntry {
                id: Uuid::new_v4(),
                session_id: approval.session_id,
                kind: "approval.resolved".into(),
                summary: approval.action_summary.clone(),
                ref_type: Some("approval".into()),
                ref_id: Some(approval_id.to_string()),
                bridge_id: None,
                ts: Utc::now(),
            });
        }

        Ok(outcome)
    }

    pub fn record_proposed_action(
        state: &AppState,
        rec: ProposedActionRecord,
    ) -> Result<(), ApiError> {
        state
            .integrations
            .lock()
            .insert_proposed_action(&rec)
            .map_err(|e| ApiError::validation(&e.to_string()))
    }

    pub fn notify_session_channels(state: &Arc<AppState>, session_id: Uuid, approval_id: Uuid) {
        let links = state
            .integrations
            .lock()
            .list_session_chat_links(session_id)
            .unwrap_or_default();
        let channels: Vec<String> = links
            .iter()
            .map(|l| format!("{}:{}", l.bridge_id, l.channel_id))
            .collect();
        let _ = state.integrations.lock().set_approval_channels_notified(
            approval_id,
            &serde_json::to_string(&channels).unwrap_or_else(|_| "[]".into()),
        );
        let _ = state.record_timeline(
            session_id,
            "approval",
            "approval.pending",
            serde_json::json!({ "approvalId": approval_id.to_string(), "channels": channels }),
        );
        // Push notifications require registered device tokens (mobile app).
        let _ = approval_id;
    }
}
