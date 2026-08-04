use std::collections::{BTreeMap, BTreeSet};

use crate::dashboard::{DashboardReadModel, DashboardStatus, SelectionKey};

use super::intents::{
    Confirmation, ControlOperation, ControlResult, CoordinatorIntent, DashboardCommand,
    DashboardContext, DashboardVisual, OperationKey,
};
use super::{DashboardControlError, DashboardControlErrorKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardControlState {
    pub dashboard: DashboardReadModel,
    pub context: DashboardContext,
    pub replay_mode: bool,
    pub visual: DashboardVisual,
    pub(crate) authorized_sessions: BTreeMap<SelectionKey, bool>,
    pub(crate) expired_sessions: BTreeSet<SelectionKey>,
    pub(crate) pending_permissions: BTreeMap<String, SelectionKey>,
    pub(crate) pending_questions: BTreeMap<String, SelectionKey>,
    pub(crate) pending_operations: BTreeSet<OperationKey>,
    pub(crate) resolved_operations: BTreeSet<OperationKey>,
}

impl DashboardControlState {
    pub fn new(
        dashboard: DashboardReadModel,
        selection: Option<SelectionKey>,
        draft: impl Into<String>,
    ) -> Self {
        let authorized_sessions = dashboard
            .all_rows
            .iter()
            .map(|row| {
                (
                    row.selection_key.clone(),
                    row.eligibility.is_eligible && !row.relationship.is_foreign,
                )
            })
            .collect();
        Self {
            dashboard,
            context: DashboardContext::new(selection, draft),
            replay_mode: false,
            visual: DashboardVisual::idle(),
            authorized_sessions,
            expired_sessions: BTreeSet::new(),
            pending_permissions: BTreeMap::new(),
            pending_questions: BTreeMap::new(),
            pending_operations: BTreeSet::new(),
            resolved_operations: BTreeSet::new(),
        }
    }

    pub fn dispatch(
        &self,
        command: DashboardCommand,
    ) -> Result<ControlResult, DashboardControlError> {
        let operation = command.operation();
        let session_id = self.validate_session(operation)?;
        match command {
            DashboardCommand::RenameSession {
                title,
                confirmation,
            } => {
                let title = title.trim();
                if title.is_empty() {
                    return Err(self.error(operation, DashboardControlErrorKind::InvalidTitle));
                }
                if confirmation == Confirmation::Required {
                    return Ok(self.confirmation(operation));
                }
                self.start(
                    CoordinatorIntent::RenameSession {
                        session_id,
                        title: title.to_string(),
                    },
                    operation,
                )
            }
            DashboardCommand::StopSession { confirmation } => {
                if confirmation == Confirmation::Required {
                    return Ok(self.confirmation(operation));
                }
                self.start(CoordinatorIntent::StopSession { session_id }, operation)
            }
            DashboardCommand::CancelTask {
                task_id,
                confirmation,
            } => {
                let task_id = task_id.trim();
                if task_id.is_empty() {
                    return Err(self.error(operation, DashboardControlErrorKind::InvalidTaskId));
                }
                if confirmation == Confirmation::Required {
                    return Ok(self.confirmation(operation));
                }
                self.start(
                    CoordinatorIntent::CancelTask {
                        session_id,
                        task_id: task_id.to_string(),
                    },
                    operation,
                )
            }
            DashboardCommand::ResolvePermission(request) => {
                self.start_permission(session_id, request)
            }
            DashboardCommand::AnswerQuestion(request) => self.start_question(session_id, request),
        }
    }

    pub(crate) fn validate_session(
        &self,
        operation: ControlOperation,
    ) -> Result<SelectionKey, DashboardControlError> {
        let Some(session_id) = self.context.selection.as_ref() else {
            return Err(self.error(operation, DashboardControlErrorKind::NoSelection));
        };
        let Some(row) = self.dashboard.row(session_id.as_str()) else {
            return Err(self.error(
                operation,
                DashboardControlErrorKind::UnknownSession(session_id.clone()),
            ));
        };
        if !matches!(self.authorized_sessions.get(session_id), Some(true)) {
            return Err(self.error(
                operation,
                DashboardControlErrorKind::UnauthorizedSession(session_id.clone()),
            ));
        }
        if self.expired_sessions.contains(session_id) {
            return Err(self.error(
                operation,
                DashboardControlErrorKind::ExpiredSession(session_id.clone()),
            ));
        }
        match row.status {
            DashboardStatus::Running | DashboardStatus::Queued | DashboardStatus::Streaming => {}
            DashboardStatus::Completed | DashboardStatus::Failed | DashboardStatus::Cancelled => {
                return Err(self.error(
                    operation,
                    DashboardControlErrorKind::FinishedSession(session_id.clone()),
                ));
            }
            DashboardStatus::Stale => {
                return Err(self.error(
                    operation,
                    DashboardControlErrorKind::StaleSession(session_id.clone()),
                ));
            }
        }
        if self.replay_mode {
            return Err(self.error(operation, DashboardControlErrorKind::ReplayReadOnly));
        }
        Ok(session_id.clone())
    }

    pub(crate) fn start(
        &self,
        intent: CoordinatorIntent,
        operation: ControlOperation,
    ) -> Result<ControlResult, DashboardControlError> {
        let key = intent.key();
        if self.pending_operations.contains(&key) || self.resolved_operations.contains(&key) {
            return Err(self.error(operation, DashboardControlErrorKind::Duplicate(operation)));
        }
        let mut next = self.clone();
        next.pending_operations.insert(key);
        match &intent {
            CoordinatorIntent::ResolvePermission { permission_id, .. } => {
                next.pending_permissions.remove(permission_id);
            }
            CoordinatorIntent::AnswerQuestion { permission_id, .. } => {
                next.pending_questions.remove(permission_id);
            }
            CoordinatorIntent::RenameSession { .. }
            | CoordinatorIntent::StopSession { .. }
            | CoordinatorIntent::CancelTask { .. } => {}
        }
        next.visual = DashboardVisual::pending(operation);
        Ok(ControlResult {
            visual: next.visual.clone(),
            state: next,
            intent: Some(intent),
        })
    }

    pub(crate) fn confirmation(&self, operation: ControlOperation) -> ControlResult {
        let mut next = self.clone();
        next.visual = DashboardVisual::confirmation(operation);
        ControlResult {
            visual: next.visual.clone(),
            state: next,
            intent: None,
        }
    }

    pub(crate) fn error(
        &self,
        operation: ControlOperation,
        kind: DashboardControlErrorKind,
    ) -> DashboardControlError {
        DashboardControlError::new(&self.context, operation, kind)
    }
}

pub fn rename(
    state: &DashboardControlState,
    title: impl Into<String>,
    confirmation: Confirmation,
) -> Result<ControlResult, DashboardControlError> {
    state.dispatch(DashboardCommand::RenameSession {
        title: title.into(),
        confirmation,
    })
}

pub fn stop(
    state: &DashboardControlState,
    confirmation: Confirmation,
) -> Result<ControlResult, DashboardControlError> {
    state.dispatch(DashboardCommand::StopSession { confirmation })
}

pub fn cancel(
    state: &DashboardControlState,
    task_id: impl Into<String>,
    confirmation: Confirmation,
) -> Result<ControlResult, DashboardControlError> {
    state.dispatch(DashboardCommand::CancelTask {
        task_id: task_id.into(),
        confirmation,
    })
}

pub fn dispatch(
    state: &DashboardControlState,
    command: DashboardCommand,
) -> Result<ControlResult, DashboardControlError> {
    state.dispatch(command)
}
