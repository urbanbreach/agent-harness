use std::fmt;

use super::cancel::{apply_cancel, CancelError, CancelStage};
use super::drafts::preserve_draft;
use super::stale::{reject_stale, StaleError};
use super::state::{QueueLifecycle, QueueState, QueuedItem};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueAction {
    Submit { text: String },
    Queue { queued_id: String, text: String },
    Interject { queued_id: String, text: String },
    SendNow { text: String },
    Edit { queued_id: String, text: String },
    Remove { queued_id: String },
    Cancel(CancelStage),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueError {
    EmptyText,
    DuplicateQueuedId(String),
    Disabled {
        action: &'static str,
        lifecycle: QueueLifecycle,
    },
    Busy {
        action: &'static str,
        lifecycle: QueueLifecycle,
    },
    Stale(StaleError),
    Cancel(CancelError),
}

impl fmt::Display for QueueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyText => formatter.write_str("prompt text must be non-empty after trim"),
            Self::DuplicateQueuedId(id) => {
                write!(formatter, "queued prompt id already exists: {id}")
            }
            Self::Disabled { action, lifecycle } => {
                write!(formatter, "{action} is disabled during {lifecycle}")
            }
            Self::Busy { action, lifecycle } => {
                write!(
                    formatter,
                    "{action} is disabled while lifecycle is {lifecycle}"
                )
            }
            Self::Stale(error) => error.fmt(formatter),
            Self::Cancel(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for QueueError {}

impl From<StaleError> for QueueError {
    fn from(error: StaleError) -> Self {
        Self::Stale(error)
    }
}

impl From<CancelError> for QueueError {
    fn from(error: CancelError) -> Self {
        Self::Cancel(error)
    }
}

pub fn apply(state: QueueState, action: QueueAction) -> Result<QueueState, QueueError> {
    match action {
        QueueAction::Submit { text } => {
            ensure_composer_action(&state, "submit")?;
            validate_text(&text)?;
            Ok(state)
        }
        QueueAction::Queue { queued_id, text } => {
            ensure_queue_action(&state)?;
            validate_new_id(&state, &queued_id)?;
            let mut next = state.clone();
            next.queued.push(QueuedItem::new(queued_id, trimmed(&text)));
            Ok(preserve_draft(&state, next))
        }
        QueueAction::Interject { queued_id, text } => {
            if state.lifecycle != QueueLifecycle::Streaming {
                return Err(QueueError::Disabled {
                    action: "interject",
                    lifecycle: state.lifecycle,
                });
            }
            validate_new_id(&state, &queued_id)?;
            validate_text(&text)?;
            let mut item = QueuedItem::new(queued_id, trimmed(&text));
            item.is_interjection = true;
            let mut next = state.clone();
            next.queued.insert(0, item);
            Ok(preserve_draft(&state, next))
        }
        QueueAction::SendNow { text } => {
            ensure_composer_action(&state, "send-now")?;
            validate_text(&text)?;
            Ok(state)
        }
        QueueAction::Edit { queued_id, text } => {
            ensure_edit_action(&state, "edit")?;
            reject_stale(&state, &queued_id)?;
            validate_text(&text)?;
            let mut next = state.clone();
            let Some(item) = next.queued.iter_mut().find(|item| item.id == queued_id) else {
                return Err(StaleError::MissingQueuedId(queued_id).into());
            };
            item.text = trimmed(&text);
            Ok(preserve_draft(&state, next))
        }
        QueueAction::Remove { queued_id } => {
            ensure_edit_action(&state, "remove")?;
            reject_stale(&state, &queued_id)?;
            let mut next = state.clone();
            let Some(index) = next.queued.iter().position(|item| item.id == queued_id) else {
                return Err(StaleError::MissingQueuedId(queued_id).into());
            };
            next.queued.remove(index);
            Ok(preserve_draft(&state, next))
        }
        QueueAction::Cancel(stage) => Ok(apply_cancel(state, stage)?),
    }
}

fn ensure_composer_action(state: &QueueState, action: &'static str) -> Result<(), QueueError> {
    if matches!(
        state.lifecycle,
        QueueLifecycle::Tool | QueueLifecycle::Cancelling
    ) {
        Err(QueueError::Disabled {
            action,
            lifecycle: state.lifecycle,
        })
    } else {
        Ok(())
    }
}

fn ensure_queue_action(state: &QueueState) -> Result<(), QueueError> {
    if state.lifecycle == QueueLifecycle::Cancelling {
        Err(QueueError::Disabled {
            action: "queue",
            lifecycle: state.lifecycle,
        })
    } else {
        Ok(())
    }
}

fn ensure_edit_action(state: &QueueState, action: &'static str) -> Result<(), QueueError> {
    if matches!(
        state.lifecycle,
        QueueLifecycle::Tool | QueueLifecycle::Cancelling
    ) {
        Err(QueueError::Busy {
            action,
            lifecycle: state.lifecycle,
        })
    } else {
        Ok(())
    }
}

fn validate_new_id(state: &QueueState, queued_id: &str) -> Result<(), QueueError> {
    if state.queued.iter().any(|item| item.id == queued_id) {
        Err(QueueError::DuplicateQueuedId(queued_id.to_owned()))
    } else {
        Ok(())
    }
}

fn validate_text(text: &str) -> Result<(), QueueError> {
    if text.trim().is_empty() {
        Err(QueueError::EmptyText)
    } else {
        Ok(())
    }
}

fn trimmed(text: &str) -> String {
    text.trim().to_owned()
}
