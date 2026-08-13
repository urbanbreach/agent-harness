use std::fmt::{Display, Formatter};

use crate::app::interaction_reducer::{
    InteractionState, ScreenMode, TransitionError, TransitionTable, UiIntent as InteractionIntent,
};
use crate::app::Focus;
use crate::attachment_lifecycle::Attachment;
use crate::completion_controller::{
    CompletionController, CompletionError, CompletionItem, CompletionRequest, CompletionTrigger,
};
use crate::composer_atoms::AttachmentId;
use crate::composer_editing::{ComposerEditor, EditingError};
use crate::ghost_suggestions::{SuggestionController, SuggestionError};
use crate::prompt_queue_actions::{
    apply as apply_queue_action, QueueAction, QueueError, QueueLifecycle, QueueState,
};
use crate::scheduling::DualClock;
use crate::shell_geometry::ShellState;

use super::hit_map::{build as build_hit_map, ComposerHitMap};
use super::submission::{build as build_submission, ComposerUiIntent, SubmissionError};
use super::view_model::{build as build_view_model, ComposerViewModel};
use crate::design_contract::ViewportId;

pub struct AttachmentEntry {
    pub id: AttachmentId,
    pub(super) atom_id: u64,
    pub attachment: Attachment,
}

impl AttachmentEntry {
    pub(super) fn atom_id(&self) -> crate::composer_atoms::AtomId {
        crate::composer_atoms::AtomId::new(self.atom_id)
    }
}

pub struct ComposerSlice {
    pub(super) editor: ComposerEditor,
    pub(super) completion: CompletionController,
    pub(super) completion_trigger: Option<CompletionTrigger>,
    pub(super) completion_items: Vec<CompletionItem>,
    pub(super) suggestions: SuggestionController,
    pub(super) attachments: Vec<AttachmentEntry>,
    pub(super) queue: QueueState,
    interaction: InteractionState,
    transitions: TransitionTable,
    pub(super) clock: DualClock,
}

#[derive(Debug)]
pub enum ComposerSliceError {
    Editing(EditingError),
    Completion(CompletionError),
    Suggestion(SuggestionError),
    Queue(QueueError),
    Transition(TransitionError),
    DuplicateAttachment(AttachmentId),
    Submission(SubmissionError),
}

impl Display for ComposerSliceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Editing(error) => Display::fmt(error, formatter),
            Self::Completion(error) => Display::fmt(error, formatter),
            Self::Suggestion(error) => Display::fmt(error, formatter),
            Self::Queue(error) => Display::fmt(error, formatter),
            Self::Transition(error) => Display::fmt(error, formatter),
            Self::DuplicateAttachment(id) => {
                write!(formatter, "attachment id {} already exists", id.get())
            }
            Self::Submission(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for ComposerSliceError {}

macro_rules! from_error {
    ($source:ty, $variant:ident) => {
        impl From<$source> for ComposerSliceError {
            fn from(error: $source) -> Self {
                Self::$variant(error)
            }
        }
    };
}

from_error!(EditingError, Editing);
from_error!(CompletionError, Completion);
from_error!(SuggestionError, Suggestion);
from_error!(QueueError, Queue);
from_error!(TransitionError, Transition);
from_error!(SubmissionError, Submission);

impl ComposerSlice {
    pub fn new() -> Self {
        Self::with_editor(ComposerEditor::new())
    }

    pub fn from_text(text: &str) -> Self {
        Self::with_editor(ComposerEditor::from_text(text))
    }

    fn with_editor(editor: ComposerEditor) -> Self {
        let queue = QueueState::default().with_draft(editor.text());
        Self {
            editor,
            completion: CompletionController::new(),
            completion_trigger: None,
            completion_items: Vec::new(),
            suggestions: SuggestionController::default(),
            attachments: Vec::new(),
            queue,
            interaction: InteractionState::new(ScreenMode::Live, Focus::Prompt),
            transitions: TransitionTable,
            clock: DualClock::new(),
        }
    }

    pub fn editor(&self) -> &ComposerEditor {
        &self.editor
    }

    pub fn completion(&self) -> &CompletionController {
        &self.completion
    }

    pub fn suggestions(&self) -> &SuggestionController {
        &self.suggestions
    }

    pub fn attachments(&self) -> &[AttachmentEntry] {
        &self.attachments
    }

    pub const fn queue_state(&self) -> &QueueState {
        &self.queue
    }

    pub const fn interaction_state(&self) -> &InteractionState {
        &self.interaction
    }

    pub(super) fn is_prompt_focused(&self) -> bool {
        self.interaction.focus == Focus::Prompt
    }

    pub const fn clock(&self) -> &DualClock {
        &self.clock
    }

    pub fn set_queue_state(&mut self, mut state: QueueState) -> Result<(), ComposerSliceError> {
        state.draft = self.editor.text();
        self.queue = state;
        self.cancel_completion();
        self.suggestions.on_state_change()?;
        Ok(())
    }

    pub fn apply_queue_action(&mut self, action: QueueAction) -> Result<(), ComposerSliceError> {
        self.queue.draft = self.editor.text();
        self.queue = apply_queue_action(self.queue.clone(), action)?;
        Ok(())
    }

    pub fn apply_interaction(
        &mut self,
        intent: InteractionIntent,
    ) -> Result<(), ComposerSliceError> {
        let focus = self.interaction.focus;
        self.interaction = self.transitions.apply(self.interaction.clone(), intent)?;
        if self.interaction.focus != focus {
            self.suggestions.on_focus_change()?;
        }
        Ok(())
    }

    pub fn submit(&self) -> Result<ComposerUiIntent, ComposerSliceError> {
        Ok(build_submission(self)?)
    }

    pub fn view_model(&self, viewport: ViewportId) -> ComposerViewModel {
        build_view_model(self, viewport)
    }

    pub fn hit_map(&self, viewport: ViewportId) -> ComposerHitMap {
        build_hit_map(self, viewport)
    }

    pub(super) fn shell_state(&self) -> ShellState {
        match self.queue.lifecycle {
            QueueLifecycle::Idle => {
                if self.editor.text().is_empty() {
                    ShellState::Idle
                } else {
                    ShellState::Drafting
                }
            }
            QueueLifecycle::Streaming | QueueLifecycle::Tool => ShellState::Streaming,
            QueueLifecycle::Waiting => ShellState::Queued,
            QueueLifecycle::Cancelling => ShellState::Cancelling,
            QueueLifecycle::Completed => ShellState::Completed,
            QueueLifecycle::Failed => ShellState::Failed,
        }
    }
}

impl Default for ComposerSlice {
    fn default() -> Self {
        Self::new()
    }
}
