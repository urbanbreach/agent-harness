use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

use crate::prompt_queue_actions::{apply, QueueAction, QueueError, QueueLifecycle, QueueState};

use super::composer::{DashboardReplyComposer, ReplyComposerError, ReplyPayload};
use super::stale_target::{
    DispatchCapability, StaleTargetError, StaleTargetGuard, TargetIdentity, TargetSnapshot,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchAction {
    Queue,
    Interject,
    SendNow,
}

impl Display for DispatchAction {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Queue => "queue",
            Self::Interject => "interject",
            Self::SendNow => "send-now",
        };
        formatter.write_str(label)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchIntent {
    pub target: TargetIdentity,
    pub action: DispatchAction,
    pub text: String,
    pub attachments: Vec<super::composer::ReplyAttachment>,
    pub sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinatorValidationError {
    TargetMissing,
    LifecycleRejected,
    CapabilityRejected,
    Rejected,
}

impl Display for CoordinatorValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::TargetMissing => "coordinator target is missing",
            Self::LifecycleRejected => "coordinator rejected the target lifecycle",
            Self::CapabilityRejected => "coordinator rejected the target capability",
            Self::Rejected => "coordinator rejected the dashboard dispatch",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for CoordinatorValidationError {}

pub trait CoordinatorValidator {
    fn validate(&self, intent: &DispatchIntent) -> Result<(), CoordinatorValidationError>;
}

#[derive(Debug)]
pub enum DispatchError {
    NoSelectedTarget,
    UnknownTarget(String),
    StaleTarget(StaleTargetError),
    Composer(ReplyComposerError),
    Queue(QueueError),
    AttachmentCapability {
        capability: super::AttachmentCapability,
        mime: crate::attachment_lifecycle::MimeKind,
    },
    Coordinator(CoordinatorValidationError),
    CapabilityDenied {
        action: DispatchAction,
        target: TargetIdentity,
    },
}

impl Display for DispatchError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSelectedTarget => formatter.write_str("dashboard has no selected target"),
            Self::UnknownTarget(target) => write!(formatter, "unknown dashboard target: {target}"),
            Self::StaleTarget(error) => Display::fmt(error, formatter),
            Self::Composer(error) => Display::fmt(error, formatter),
            Self::Queue(error) => Display::fmt(error, formatter),
            Self::AttachmentCapability { capability, mime } => write!(
                formatter,
                "attachment MIME {} is not supported by {capability}",
                mime.as_str()
            ),
            Self::Coordinator(error) => Display::fmt(error, formatter),
            Self::CapabilityDenied { action, target } => {
                write!(formatter, "target {target} does not permit {action}")
            }
        }
    }
}

impl std::error::Error for DispatchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::StaleTarget(error) => Some(error),
            Self::Composer(error) => Some(error),
            Self::Queue(error) => Some(error),
            Self::Coordinator(error) => Some(error),
            Self::NoSelectedTarget
            | Self::UnknownTarget(_)
            | Self::AttachmentCapability { .. }
            | Self::CapabilityDenied { .. } => None,
        }
    }
}

impl From<StaleTargetError> for DispatchError {
    fn from(error: StaleTargetError) -> Self {
        Self::StaleTarget(error)
    }
}

impl From<ReplyComposerError> for DispatchError {
    fn from(error: ReplyComposerError) -> Self {
        match error {
            ReplyComposerError::AttachmentCapability { capability, mime } => {
                Self::AttachmentCapability { capability, mime }
            }
            other => Self::Composer(other),
        }
    }
}

impl From<QueueError> for DispatchError {
    fn from(error: QueueError) -> Self {
        Self::Queue(error)
    }
}

pub(crate) struct PreparedDispatch {
    pub(crate) intent: DispatchIntent,
    next_state: QueueState,
    next_sequence: u64,
}

#[derive(Debug)]
pub(crate) struct TargetDispatchQueue {
    state: QueueState,
    next_sequence: u64,
}

impl TargetDispatchQueue {
    pub(crate) fn new(snapshot: &TargetSnapshot) -> Self {
        Self {
            state: QueueState::new(snapshot.queue_lifecycle),
            next_sequence: 1,
        }
    }

    pub(crate) fn refresh(&mut self, lifecycle: QueueLifecycle) {
        self.state.lifecycle = lifecycle;
    }

    pub(crate) fn prepare(
        &self,
        snapshot: &TargetSnapshot,
        guard: &StaleTargetGuard,
        action: DispatchAction,
        payload: ReplyPayload,
    ) -> Result<PreparedDispatch, DispatchError> {
        guard.validate(Some(snapshot))?;
        let capability = match action {
            DispatchAction::Queue => DispatchCapability::Queue,
            DispatchAction::Interject => DispatchCapability::Interject,
            DispatchAction::SendNow => DispatchCapability::SendNow,
        };
        if !snapshot.capabilities.allows(capability) {
            return Err(DispatchError::CapabilityDenied {
                action,
                target: snapshot.identity.clone(),
            });
        }
        let sequence = self.next_sequence;
        let queued_id = format!("{}:{sequence}", snapshot.identity);
        let queue_action = match action {
            DispatchAction::Queue => QueueAction::Queue {
                queued_id,
                text: payload.text.clone(),
            },
            DispatchAction::Interject => QueueAction::Interject {
                queued_id,
                text: payload.text.clone(),
            },
            DispatchAction::SendNow => QueueAction::SendNow {
                text: payload.text.clone(),
            },
        };
        let next_state = apply(self.state.clone(), queue_action)?;
        Ok(PreparedDispatch {
            intent: DispatchIntent {
                target: snapshot.identity.clone(),
                action,
                text: payload.text,
                attachments: payload.attachments,
                sequence,
            },
            next_state,
            next_sequence: sequence.saturating_add(1),
        })
    }

    pub(crate) fn commit(&mut self, prepared: PreparedDispatch) -> DispatchIntent {
        self.state = prepared.next_state;
        self.next_sequence = prepared.next_sequence;
        prepared.intent
    }
}

pub(crate) fn composer_payload(
    composer: &DashboardReplyComposer,
) -> Result<ReplyPayload, DispatchError> {
    composer.payload().map_err(DispatchError::from)
}

pub(crate) type QueueMap = BTreeMap<TargetIdentity, TargetDispatchQueue>;
