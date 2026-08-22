use std::fmt::{Display, Formatter};

use crate::attachment_lifecycle::MimeKind;
use crate::composer_atoms::AttachmentId;
use crate::composer_integration::interaction::UiIntent as InteractionUiIntent;
use crate::keybindings::Action;
use crate::prompt_queue_actions::{apply, QueueAction, QueueError};

use super::slice::ComposerSlice;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmissionAttachment {
    pub id: AttachmentId,
    pub mime: MimeKind,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerUiIntent {
    pub interaction: InteractionUiIntent,
    pub text: String,
    pub attachments: Vec<SubmissionAttachment>,
}

pub type UiIntent = ComposerUiIntent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmissionError {
    Queue(QueueError),
}

impl Display for SubmissionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Queue(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for SubmissionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Queue(error) => Some(error),
        }
    }
}

impl From<QueueError> for SubmissionError {
    fn from(error: QueueError) -> Self {
        Self::Queue(error)
    }
}

pub(super) fn build(slice: &ComposerSlice) -> Result<ComposerUiIntent, SubmissionError> {
    let text = slice.editor.text();
    apply(
        slice.queue.clone(),
        QueueAction::Submit { text: text.clone() },
    )?;
    let attachments = slice
        .attachments
        .iter()
        .map(|entry| SubmissionAttachment {
            id: entry.id,
            mime: entry.attachment.mime(),
            bytes: entry.attachment.bytes().to_vec(),
        })
        .collect();
    Ok(ComposerUiIntent {
        interaction: InteractionUiIntent::DispatchAction(Action::SubmitPrompt),
        text,
        attachments,
    })
}
