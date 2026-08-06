use crate::dashboard::SelectionKey;

use super::intents::{ControlOperation, ControlResult, CoordinatorIntent, OperationKey};
use super::DashboardControlState;
use super::{DashboardControlError, DashboardControlErrorKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionAnswerRequest {
    pub permission_id: String,
    pub answers: Vec<Vec<String>>,
}

impl QuestionAnswerRequest {
    pub fn new(permission_id: impl Into<String>, answers: Vec<Vec<String>>) -> Self {
        Self {
            permission_id: permission_id.into(),
            answers,
        }
    }
}

impl DashboardControlState {
    pub fn with_pending_question(mut self, permission_id: impl Into<String>) -> Self {
        if let Some(session_id) = self.context.selection.clone() {
            self.pending_questions
                .insert(permission_id.into(), session_id);
        }
        self
    }

    pub fn with_pending_question_for(
        mut self,
        permission_id: impl Into<String>,
        session_id: SelectionKey,
    ) -> Self {
        self.pending_questions
            .insert(permission_id.into(), session_id);
        self
    }

    pub fn has_pending_question(&self, permission_id: &str) -> bool {
        self.pending_questions.contains_key(permission_id)
    }

    pub(crate) fn start_question(
        &self,
        session_id: SelectionKey,
        request: QuestionAnswerRequest,
    ) -> Result<ControlResult, DashboardControlError> {
        let operation = ControlOperation::Question;
        let permission_id = request.permission_id.trim();
        if permission_id.is_empty() {
            return Err(self.error(operation, DashboardControlErrorKind::InvalidPermissionId));
        }
        if request.answers.is_empty() {
            return Err(self.error(operation, DashboardControlErrorKind::EmptyQuestionAnswers));
        }
        let key = OperationKey::Question(session_id.clone(), permission_id.to_string());
        if self.pending_operations.contains(&key) || self.resolved_operations.contains(&key) {
            return Err(self.error(operation, DashboardControlErrorKind::Duplicate(operation)));
        }
        let Some(owner) = self.pending_questions.get(permission_id) else {
            return Err(self.error(
                operation,
                DashboardControlErrorKind::MissingQuestion(permission_id.to_string()),
            ));
        };
        if owner != &session_id {
            return Err(self.error(
                operation,
                DashboardControlErrorKind::ResponseSessionMismatch(operation),
            ));
        }
        self.start(
            CoordinatorIntent::AnswerQuestion {
                session_id,
                permission_id: permission_id.to_string(),
                answers: request.answers,
            },
            operation,
        )
    }
}

pub fn answer_question(
    state: &DashboardControlState,
    request: QuestionAnswerRequest,
) -> Result<ControlResult, DashboardControlError> {
    let session_id = state.validate_session(ControlOperation::Question)?;
    state.start_question(session_id, request)
}
