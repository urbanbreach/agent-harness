use std::fmt::{Display, Formatter};

use crate::attachment_lifecycle::MimeKind;
use crate::dashboard::SelectionKey;
use crate::prompt_queue_actions::QueueLifecycle;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TargetIdentity(SelectionKey);

impl TargetIdentity {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self(SelectionKey::new(session_id))
    }

    pub fn from_selection_key(session_id: SelectionKey) -> Self {
        Self(session_id)
    }

    pub fn session_id(&self) -> &str {
        self.0.as_str()
    }

    pub fn selection_key(&self) -> &SelectionKey {
        &self.0
    }
}

impl Display for TargetIdentity {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.session_id())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetLifecycle {
    Idle,
    Working,
    Finished,
    Failed,
    Stale,
    Removed,
}

impl TargetLifecycle {
    pub const fn is_closed(self) -> bool {
        matches!(
            self,
            Self::Finished | Self::Failed | Self::Stale | Self::Removed
        )
    }

    pub const fn queue_lifecycle(self) -> QueueLifecycle {
        match self {
            Self::Idle => QueueLifecycle::Idle,
            Self::Working => QueueLifecycle::Streaming,
            Self::Finished => QueueLifecycle::Completed,
            Self::Failed | Self::Stale | Self::Removed => QueueLifecycle::Failed,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentCapability {
    None,
    Images,
    ImagesAndText,
}

impl AttachmentCapability {
    pub const fn supports(self, mime: MimeKind) -> bool {
        match self {
            Self::None => false,
            Self::Images => matches!(mime, MimeKind::Png | MimeKind::Jpeg),
            Self::ImagesAndText => {
                matches!(mime, MimeKind::Png | MimeKind::Jpeg | MimeKind::PlainText)
            }
        }
    }
}

impl Display for AttachmentCapability {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::None => "none",
            Self::Images => "images",
            Self::ImagesAndText => "images-and-text",
        };
        formatter.write_str(label)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetCapabilities {
    pub can_reply: bool,
    pub can_queue: bool,
    pub can_interject: bool,
    pub can_send_now: bool,
    pub attachment: AttachmentCapability,
}

impl TargetCapabilities {
    pub const fn interactive() -> Self {
        Self {
            can_reply: true,
            can_queue: true,
            can_interject: false,
            can_send_now: true,
            attachment: AttachmentCapability::ImagesAndText,
        }
    }

    pub const fn with_attachment_capability(self, attachment: AttachmentCapability) -> Self {
        Self { attachment, ..self }
    }

    pub const fn allows(self, action: DispatchCapability) -> bool {
        match action {
            DispatchCapability::Reply => self.can_reply,
            DispatchCapability::Queue => self.can_queue,
            DispatchCapability::Interject => self.can_interject,
            DispatchCapability::SendNow => self.can_send_now,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchCapability {
    Reply,
    Queue,
    Interject,
    SendNow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetSnapshot {
    pub identity: TargetIdentity,
    pub lifecycle: TargetLifecycle,
    pub revision: u64,
    pub queue_lifecycle: QueueLifecycle,
    pub capabilities: TargetCapabilities,
}

impl TargetSnapshot {
    pub fn new(identity: TargetIdentity, lifecycle: TargetLifecycle, revision: u64) -> Self {
        let mut capabilities = TargetCapabilities::interactive();
        if lifecycle.is_closed() {
            capabilities = TargetCapabilities {
                can_reply: false,
                can_queue: false,
                can_interject: false,
                can_send_now: false,
                attachment: AttachmentCapability::None,
            };
        } else if lifecycle == TargetLifecycle::Working {
            capabilities.can_interject = true;
        }
        Self {
            identity,
            lifecycle,
            revision,
            queue_lifecycle: lifecycle.queue_lifecycle(),
            capabilities,
        }
    }

    pub fn with_capabilities(self, capabilities: TargetCapabilities) -> Self {
        Self {
            capabilities,
            ..self
        }
    }

    pub fn with_queue_lifecycle(self, queue_lifecycle: QueueLifecycle) -> Self {
        Self {
            queue_lifecycle,
            ..self
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleTargetGuard {
    identity: TargetIdentity,
    revision: u64,
}

impl StaleTargetGuard {
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub fn from_snapshot(snapshot: &TargetSnapshot) -> Self {
        Self {
            identity: snapshot.identity.clone(),
            revision: snapshot.revision,
        }
    }

    pub fn validate(&self, current: Option<&TargetSnapshot>) -> Result<(), StaleTargetError> {
        let Some(current) = current else {
            return Err(StaleTargetError::Removed {
                target: self.identity.clone(),
            });
        };
        if current.identity != self.identity {
            return Err(StaleTargetError::IdentityChanged {
                expected: self.identity.clone(),
                actual: current.identity.clone(),
            });
        }
        match current.lifecycle {
            TargetLifecycle::Finished => Err(StaleTargetError::Finished {
                target: self.identity.clone(),
            }),
            TargetLifecycle::Failed => Err(StaleTargetError::Failed {
                target: self.identity.clone(),
            }),
            TargetLifecycle::Stale => Err(StaleTargetError::Stale {
                target: self.identity.clone(),
            }),
            TargetLifecycle::Removed => Err(StaleTargetError::Removed {
                target: self.identity.clone(),
            }),
            TargetLifecycle::Idle | TargetLifecycle::Working => {
                if current.revision != self.revision {
                    Err(StaleTargetError::RevisionChanged {
                        target: self.identity.clone(),
                        expected: self.revision,
                        actual: current.revision,
                    })
                } else {
                    Ok(())
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaleTargetError {
    Finished {
        target: TargetIdentity,
    },
    Failed {
        target: TargetIdentity,
    },
    Stale {
        target: TargetIdentity,
    },
    Removed {
        target: TargetIdentity,
    },
    IdentityChanged {
        expected: TargetIdentity,
        actual: TargetIdentity,
    },
    RevisionChanged {
        target: TargetIdentity,
        expected: u64,
        actual: u64,
    },
}

impl Display for StaleTargetError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Finished { target } => write!(formatter, "target {target} has finished"),
            Self::Failed { target } => write!(formatter, "target {target} has failed"),
            Self::Stale { target } => write!(formatter, "target {target} is stale"),
            Self::Removed { target } => write!(formatter, "target {target} was removed"),
            Self::IdentityChanged { expected, actual } => {
                write!(formatter, "target changed from {expected} to {actual}")
            }
            Self::RevisionChanged {
                target,
                expected,
                actual,
            } => write!(
                formatter,
                "target {target} revision changed from {expected} to {actual}"
            ),
        }
    }
}

impl std::error::Error for StaleTargetError {}
