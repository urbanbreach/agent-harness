use std::fmt::{self, Display, Formatter};

use harness_core::perm::{PermissionDecision, PermissionGrantScope};

use crate::dashboard::SelectionKey;

use super::lifecycle::DashboardControlState;
use super::permissions::PermissionDecisionRequest;
use super::question::QuestionAnswerRequest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardContext {
    pub selection: Option<SelectionKey>,
    pub draft: String,
}

impl DashboardContext {
    pub fn new(selection: Option<SelectionKey>, draft: impl Into<String>) -> Self {
        Self {
            selection,
            draft: draft.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ControlOperation {
    Rename,
    Stop,
    Cancel,
    Permission,
    Question,
}

impl Display for ControlOperation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Rename => "rename",
            Self::Stop => "stop",
            Self::Cancel => "cancel",
            Self::Permission => "permission",
            Self::Question => "question",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlVisual {
    Idle,
    Confirming,
    Pending,
    Success,
    Failure,
}

impl ControlVisual {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Confirming => "confirming",
            Self::Pending => "pending",
            Self::Success => "success",
            Self::Failure => "failure",
        }
    }

    pub const fn is_pending(self) -> bool {
        matches!(self, Self::Confirming | Self::Pending)
    }
}

pub type ControlVisualState = ControlVisual;
pub type DashboardVisualState = ControlVisual;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardVisual {
    pub state: ControlVisual,
    pub operation: Option<ControlOperation>,
    pub message: Option<String>,
}

impl DashboardVisual {
    pub(crate) fn idle() -> Self {
        Self {
            state: ControlVisual::Idle,
            operation: None,
            message: None,
        }
    }

    pub(crate) fn confirmation(operation: ControlOperation) -> Self {
        Self {
            state: ControlVisual::Confirming,
            operation: Some(operation),
            message: Some("explicit confirmation required".to_string()),
        }
    }

    pub(crate) fn pending(operation: ControlOperation) -> Self {
        Self {
            state: ControlVisual::Pending,
            operation: Some(operation),
            message: Some("awaiting coordinator confirmation".to_string()),
        }
    }

    pub(crate) fn success(operation: ControlOperation) -> Self {
        Self {
            state: ControlVisual::Success,
            operation: Some(operation),
            message: Some("coordinator confirmed success".to_string()),
        }
    }

    pub(crate) fn failure(operation: Option<ControlOperation>, message: impl Into<String>) -> Self {
        Self {
            state: ControlVisual::Failure,
            operation,
            message: Some(message.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirmation {
    Required,
    Confirmed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DashboardCommand {
    RenameSession {
        title: String,
        confirmation: Confirmation,
    },
    StopSession {
        confirmation: Confirmation,
    },
    CancelTask {
        task_id: String,
        confirmation: Confirmation,
    },
    ResolvePermission(PermissionDecisionRequest),
    AnswerQuestion(QuestionAnswerRequest),
}

impl DashboardCommand {
    pub(crate) const fn operation(&self) -> ControlOperation {
        match self {
            Self::RenameSession { .. } => ControlOperation::Rename,
            Self::StopSession { .. } => ControlOperation::Stop,
            Self::CancelTask { .. } => ControlOperation::Cancel,
            Self::ResolvePermission(_) => ControlOperation::Permission,
            Self::AnswerQuestion(_) => ControlOperation::Question,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoordinatorIntent {
    RenameSession {
        session_id: SelectionKey,
        title: String,
    },
    StopSession {
        session_id: SelectionKey,
    },
    CancelTask {
        session_id: SelectionKey,
        task_id: String,
    },
    ResolvePermission {
        session_id: SelectionKey,
        permission_id: String,
        decision: PermissionDecision,
        reason: Option<String>,
        grant_scope: Option<PermissionGrantScope>,
    },
    AnswerQuestion {
        session_id: SelectionKey,
        permission_id: String,
        answers: Vec<Vec<String>>,
    },
}

impl CoordinatorIntent {
    pub const fn operation(&self) -> ControlOperation {
        match self {
            Self::RenameSession { .. } => ControlOperation::Rename,
            Self::StopSession { .. } => ControlOperation::Stop,
            Self::CancelTask { .. } => ControlOperation::Cancel,
            Self::ResolvePermission { .. } => ControlOperation::Permission,
            Self::AnswerQuestion { .. } => ControlOperation::Question,
        }
    }

    pub(crate) fn key(&self) -> OperationKey {
        match self {
            Self::RenameSession { session_id, .. } => OperationKey::Rename(session_id.clone()),
            Self::StopSession { session_id } => OperationKey::Stop(session_id.clone()),
            Self::CancelTask {
                session_id,
                task_id,
            } => OperationKey::Cancel(session_id.clone(), task_id.clone()),
            Self::ResolvePermission {
                session_id,
                permission_id,
                ..
            } => OperationKey::Permission(session_id.clone(), permission_id.clone()),
            Self::AnswerQuestion {
                session_id,
                permission_id,
                ..
            } => OperationKey::Question(session_id.clone(), permission_id.clone()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum OperationKey {
    Rename(SelectionKey),
    Stop(SelectionKey),
    Cancel(SelectionKey, String),
    Permission(SelectionKey, String),
    Question(SelectionKey, String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoordinatorOutcome {
    Succeeded,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlResult {
    pub state: DashboardControlState,
    pub intent: Option<CoordinatorIntent>,
    pub visual: DashboardVisual,
}

pub type DashboardControlResult = ControlResult;
