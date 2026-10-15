//! Typed outcomes of a manual pass that did not commit: rejections and cancellations.
use crate::coord::{CancelReason, CoordinatorError};
use std::fmt;

/// Why a manual pass was refused or found nothing to do. These are rejections, not
/// successes: the interrupted turn is still resumed after the no-op kinds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManualRejection {
    /// Another manual compaction or handoff owns the agent.
    InProgress,
    NoModel,
    /// The branch already ends in a compaction.
    AlreadyCompacted,
    SessionTooSmall,
    NothingToHandOff(HandoffRejection),
    /// No configured method can run for a manual request.
    NoMethod,
    /// A no-op reason produced by a method with its own wording.
    Other(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandoffRejection {
    NoMessages,
    AlreadyCompacted,
}

impl fmt::Display for ManualRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InProgress => "Compaction already in progress",
            Self::NoModel => "No model selected",
            Self::AlreadyCompacted => "Already compacted",
            Self::SessionTooSmall => "Nothing to compact (session too small)",
            Self::NothingToHandOff(HandoffRejection::NoMessages) => {
                "Nothing to hand off (no messages yet)"
            }
            Self::NothingToHandOff(HandoffRejection::AlreadyCompacted) => {
                "Nothing to hand off (already compacted)"
            }
            Self::NoMethod => "No configured compaction method can run manually.",
            Self::Other(text) => text,
        })
    }
}

impl ManualRejection {
    /// Classifies a pass's no-op reason; wording that is not one of the known categories
    /// stays as its own text.
    pub fn from_text(text: &str) -> Self {
        [
            Self::InProgress,
            Self::NoModel,
            Self::AlreadyCompacted,
            Self::SessionTooSmall,
            Self::NothingToHandOff(HandoffRejection::NoMessages),
            Self::NothingToHandOff(HandoffRejection::AlreadyCompacted),
            Self::NoMethod,
        ]
        .into_iter()
        .find(|known| known.to_string() == text)
        .unwrap_or_else(|| Self::Other(text.to_owned()))
    }
}

/// The error a before-compaction hook veto ends a pass with. It is a typed cancellation, but
/// not a user interrupt, so its reason is not [`CancelReason::Manual`].
pub(in crate::coord) fn hook_veto(agent: &str) -> CoordinatorError {
    CoordinatorError::CompactionCancelled {
        agent_id: agent.to_owned(),
        reason: CancelReason::Other("vetoed by a compaction hook".into()),
    }
}

/// Turns a cancellation-shaped error into the typed cancellation while the pass's own token
/// is cancelled. Every other error keeps its identity, even when the token fired: a network,
/// parsing or server failure is not relabelled as a cancellation.
pub(in crate::coord) fn translate_cancellation(
    error: CoordinatorError,
    cancelled: bool,
    agent: &str,
) -> CoordinatorError {
    match error {
        CoordinatorError::Cancelled(_) if cancelled => CoordinatorError::CompactionCancelled {
            agent_id: agent.to_owned(),
            reason: CancelReason::Manual,
        },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_cancellation_shaped_errors_become_typed_cancellations() {
        for (error, cancelled, typed) in [
            (CoordinatorError::Cancelled("abort".into()), true, true),
            (CoordinatorError::Cancelled("abort".into()), false, false),
            (
                CoordinatorError::Invalid("socket closed".into()),
                true,
                false,
            ),
            (
                CoordinatorError::CompactionCancelled {
                    agent_id: "a".into(),
                    reason: CancelReason::Superseded,
                },
                true,
                true,
            ),
        ] {
            let label = error.to_string();
            let translated = translate_cancellation(error, cancelled, "a");
            assert_eq!(
                matches!(translated, CoordinatorError::CompactionCancelled { .. }),
                typed,
                "{label} cancelled={cancelled}"
            );
        }
    }

    #[test]
    fn rejection_text_round_trips_through_its_category() {
        for rejection in [
            ManualRejection::InProgress,
            ManualRejection::NoModel,
            ManualRejection::AlreadyCompacted,
            ManualRejection::SessionTooSmall,
            ManualRejection::NothingToHandOff(HandoffRejection::NoMessages),
            ManualRejection::NothingToHandOff(HandoffRejection::AlreadyCompacted),
            ManualRejection::NoMethod,
            ManualRejection::Other("method specific".into()),
        ] {
            assert_eq!(
                ManualRejection::from_text(&rejection.to_string()),
                rejection
            );
        }
    }
}
