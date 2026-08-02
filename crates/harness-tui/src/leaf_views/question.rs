//! Question leaf view.

/// Deterministic view state for a question prompt row.
///
/// No app-state or registry dependency — a plain `Copy` value type.
/// Captures the question text, answer state, and whether the question
/// must not render edit-permission allow chrome (a question is never an
/// edit-permission gate).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct QuestionLeafView {
    pub question: &'static str,
    pub state: QuestionStateLeaf,
    pub answer: Option<&'static str>,
}

/// Lightweight question lifecycle state mirroring
/// `app::question_prompt` without the full modal dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QuestionStateLeaf {
    #[default]
    Pending,
    Answered,
    Cancelled,
    TimedOut,
}

impl QuestionLeafView {
    pub const fn new(question: &'static str) -> Self {
        Self {
            question,
            state: QuestionStateLeaf::Pending,
            answer: None,
        }
    }

    /// Mark the question as answered.
    pub const fn answered(mut self, answer: &'static str) -> Self {
        self.state = QuestionStateLeaf::Answered;
        self.answer = Some(answer);
        self
    }

    /// Mark the question as cancelled.
    pub const fn cancelled(mut self) -> Self {
        self.state = QuestionStateLeaf::Cancelled;
        self
    }

    /// Returns true when the question is still pending (awaiting user input).
    pub fn is_pending(&self) -> bool {
        matches!(self.state, QuestionStateLeaf::Pending)
    }

    /// A question must never render edit-permission allow chrome.
    /// This invariant is enforced structurally: `QuestionLeafView` has no
    /// `PermissionKindLeaf` field and no `granted`/`denied` transitions.
    pub fn renders_no_permission_chrome(&self) -> bool {
        true
    }
}
