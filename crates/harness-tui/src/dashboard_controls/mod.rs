mod intents;
mod lifecycle;
mod permissions;
mod question;

use std::fmt::{self, Display, Formatter};

use crate::dashboard::SelectionKey;

pub use intents::{
    Confirmation, ControlOperation, ControlResult, ControlVisual, ControlVisualState,
    CoordinatorIntent, CoordinatorOutcome, DashboardCommand, DashboardContext,
    DashboardControlResult, DashboardVisual, DashboardVisualState,
};
pub use lifecycle::{DashboardControlState, cancel, dispatch, rename, stop};
pub use permissions::{PermissionDecisionRequest, resolve_permission};
pub use question::{QuestionAnswerRequest, answer_question};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DashboardControlErrorKind {
    NoSelection,
    UnknownSession(SelectionKey),
    UnauthorizedSession(SelectionKey),
    ExpiredSession(SelectionKey),
    FinishedSession(SelectionKey),
    StaleSession(SelectionKey),
    ReplayReadOnly,
    InvalidTitle,
    InvalidTaskId,
    InvalidPermissionId,
    EmptyQuestionAnswers,
    MissingPermission(String),
    MissingQuestion(String),
    Duplicate(ControlOperation),
    StaleResponse(ControlOperation),
    ResponseSessionMismatch(ControlOperation),
}

impl Display for DashboardControlErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSelection => formatter.write_str("dashboard selection is unavailable"),
            Self::UnknownSession(key) => {
                write!(formatter, "dashboard session is stale: {}", key.as_str())
            }
            Self::UnauthorizedSession(key) => {
                write!(
                    formatter,
                    "dashboard session is unauthorized: {}",
                    key.as_str()
                )
            }
            Self::ExpiredSession(key) => {
                write!(formatter, "dashboard session has expired: {}", key.as_str())
            }
            Self::FinishedSession(key) => {
                write!(formatter, "dashboard session is finished: {}", key.as_str())
            }
            Self::StaleSession(key) => {
                write!(formatter, "dashboard session is stale: {}", key.as_str())
            }
            Self::ReplayReadOnly => {
                formatter.write_str("dashboard controls are disabled in replay")
            }
            Self::InvalidTitle => formatter.write_str("session title must not be empty"),
            Self::InvalidTaskId => formatter.write_str("task id must not be empty"),
            Self::InvalidPermissionId => formatter.write_str("permission id must not be empty"),
            Self::EmptyQuestionAnswers => formatter.write_str("question answers must not be empty"),
            Self::MissingPermission(id) => write!(formatter, "permission request is stale: {id}"),
            Self::MissingQuestion(id) => write!(formatter, "question request is stale: {id}"),
            Self::Duplicate(operation) => {
                write!(formatter, "duplicate {operation} response rejected")
            }
            Self::StaleResponse(operation) => {
                write!(formatter, "stale {operation} response rejected")
            }
            Self::ResponseSessionMismatch(operation) => {
                write!(
                    formatter,
                    "{operation} response targets a different session"
                )
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardControlError {
    kind: DashboardControlErrorKind,
    context: DashboardContext,
    visual: DashboardVisual,
}

impl DashboardControlError {
    pub fn kind(&self) -> &DashboardControlErrorKind {
        &self.kind
    }

    pub fn context(&self) -> &DashboardContext {
        &self.context
    }

    pub fn visual(&self) -> &DashboardVisual {
        &self.visual
    }

    pub(crate) fn new(
        context: &DashboardContext,
        operation: ControlOperation,
        kind: DashboardControlErrorKind,
    ) -> Self {
        Self {
            visual: DashboardVisual::failure(Some(operation), kind.to_string()),
            kind,
            context: context.clone(),
        }
    }
}

impl Display for DashboardControlError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        self.kind.fmt(formatter)
    }
}

impl std::error::Error for DashboardControlError {}

impl DashboardControlState {
    pub const fn with_replay_mode(mut self, replay_mode: bool) -> Self {
        self.replay_mode = replay_mode;
        self
    }

    pub fn with_session_authorized(mut self, session_id: SelectionKey, authorized: bool) -> Self {
        self.authorized_sessions.insert(session_id, authorized);
        self
    }

    pub fn with_session_expired(mut self, session_id: SelectionKey, expired: bool) -> Self {
        if expired {
            self.expired_sessions.insert(session_id);
        } else {
            self.expired_sessions.remove(&session_id);
        }
        self
    }

    pub fn settle(
        &self,
        intent: &CoordinatorIntent,
        outcome: CoordinatorOutcome,
    ) -> Result<DashboardControlResult, DashboardControlError> {
        let operation = intent.operation();
        let key = intent.key();
        if !self.pending_operations.contains(&key) {
            let kind = if self.resolved_operations.contains(&key) {
                DashboardControlErrorKind::Duplicate(operation)
            } else {
                DashboardControlErrorKind::StaleResponse(operation)
            };
            return Err(self.error(operation, kind));
        }
        let mut next = self.clone();
        next.pending_operations.remove(&key);
        match outcome {
            CoordinatorOutcome::Succeeded => {
                next.resolved_operations.insert(key);
                next.visual = DashboardVisual::success(operation);
            }
            CoordinatorOutcome::Failed(message) => {
                next.restore_pending_request(intent);
                let message = if message.trim().is_empty() {
                    "coordinator rejected intent"
                } else {
                    message.as_str()
                };
                next.visual = DashboardVisual::failure(Some(operation), message);
            }
        }
        Ok(DashboardControlResult {
            visual: next.visual.clone(),
            state: next,
            intent: None,
        })
    }

    fn restore_pending_request(&mut self, intent: &CoordinatorIntent) {
        match intent {
            CoordinatorIntent::ResolvePermission {
                session_id,
                permission_id,
                ..
            } => {
                self.pending_permissions
                    .insert(permission_id.clone(), session_id.clone());
            }
            CoordinatorIntent::AnswerQuestion {
                session_id,
                permission_id,
                ..
            } => {
                self.pending_questions
                    .insert(permission_id.clone(), session_id.clone());
            }
            CoordinatorIntent::RenameSession { .. }
            | CoordinatorIntent::StopSession { .. }
            | CoordinatorIntent::CancelTask { .. } => {}
        }
    }
}

pub fn settle(
    state: &DashboardControlState,
    intent: &CoordinatorIntent,
    outcome: CoordinatorOutcome,
) -> Result<DashboardControlResult, DashboardControlError> {
    state.settle(intent, outcome)
}
