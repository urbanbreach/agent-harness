use crate::dashboard::SelectionKey;
use harness_core::perm::{PermissionDecision, PermissionGrantScope};

use super::DashboardControlState;
use super::intents::{ControlOperation, ControlResult, CoordinatorIntent, OperationKey};
use super::{DashboardControlError, DashboardControlErrorKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionDecisionRequest {
    pub permission_id: String,
    pub decision: PermissionDecision,
    pub reason: Option<String>,
    pub grant_scope: Option<PermissionGrantScope>,
}

impl PermissionDecisionRequest {
    pub fn new(permission_id: impl Into<String>, decision: PermissionDecision) -> Self {
        Self {
            permission_id: permission_id.into(),
            decision,
            reason: None,
            grant_scope: None,
        }
    }

    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    pub const fn with_grant_scope(mut self, scope: PermissionGrantScope) -> Self {
        self.grant_scope = Some(scope);
        self
    }
}

impl DashboardControlState {
    pub fn with_pending_permission(mut self, permission_id: impl Into<String>) -> Self {
        if let Some(session_id) = self.context.selection.clone() {
            self.pending_permissions
                .insert(permission_id.into(), session_id);
        }
        self
    }

    pub fn with_pending_permission_for(
        mut self,
        permission_id: impl Into<String>,
        session_id: SelectionKey,
    ) -> Self {
        self.pending_permissions
            .insert(permission_id.into(), session_id);
        self
    }

    pub fn has_pending_permission(&self, permission_id: &str) -> bool {
        self.pending_permissions.contains_key(permission_id)
    }

    pub(crate) fn start_permission(
        &self,
        session_id: SelectionKey,
        request: PermissionDecisionRequest,
    ) -> Result<ControlResult, DashboardControlError> {
        let operation = ControlOperation::Permission;
        let permission_id = request.permission_id.trim();
        if permission_id.is_empty() {
            return Err(self.error(operation, DashboardControlErrorKind::InvalidPermissionId));
        }
        let key = OperationKey::Permission(session_id.clone(), permission_id.to_string());
        if self.pending_operations.contains(&key) || self.resolved_operations.contains(&key) {
            return Err(self.error(operation, DashboardControlErrorKind::Duplicate(operation)));
        }
        let Some(owner) = self.pending_permissions.get(permission_id) else {
            return Err(self.error(
                operation,
                DashboardControlErrorKind::MissingPermission(permission_id.to_string()),
            ));
        };
        if owner != &session_id {
            return Err(self.error(
                operation,
                DashboardControlErrorKind::ResponseSessionMismatch(operation),
            ));
        }
        self.start(
            CoordinatorIntent::ResolvePermission {
                session_id,
                permission_id: permission_id.to_string(),
                decision: request.decision,
                reason: request.reason,
                grant_scope: request.grant_scope,
            },
            operation,
        )
    }
}

pub fn resolve_permission(
    state: &DashboardControlState,
    request: PermissionDecisionRequest,
) -> Result<ControlResult, DashboardControlError> {
    let operation = ControlOperation::Permission;
    let session_id = state.validate_session(operation)?;
    state.start_permission(session_id, request)
}
