use std::fmt;

pub use crate::design_contract::LifecycleState;

pub struct TransitionTable;

impl TransitionTable {
    pub const fn new() -> Self {
        Self
    }

    pub fn is_valid(from: LifecycleState, to: LifecycleState) -> bool {
        from == to
            || match from {
                LifecycleState::Idle => matches!(
                    to,
                    LifecycleState::Drafting
                        | LifecycleState::Submitting
                        | LifecycleState::Compacting
                ),
                LifecycleState::Drafting => {
                    matches!(to, LifecycleState::Submitting | LifecycleState::Idle)
                }
                LifecycleState::Submitting => matches!(
                    to,
                    LifecycleState::Streaming | LifecycleState::Failed | LifecycleState::Idle
                ),
                LifecycleState::Streaming => matches!(
                    to,
                    LifecycleState::Thinking
                        | LifecycleState::Tool
                        | LifecycleState::Diff
                        | LifecycleState::Permission
                        | LifecycleState::Question
                        | LifecycleState::Completed
                        | LifecycleState::Failed
                        | LifecycleState::Cancelling
                        | LifecycleState::Recovering
                        | LifecycleState::Compacting
                ),
                LifecycleState::Thinking => matches!(
                    to,
                    LifecycleState::Streaming
                        | LifecycleState::Tool
                        | LifecycleState::Permission
                        | LifecycleState::Question
                ),
                LifecycleState::Tool => matches!(
                    to,
                    LifecycleState::Streaming
                        | LifecycleState::Diff
                        | LifecycleState::Permission
                        | LifecycleState::Question
                        | LifecycleState::Completed
                        | LifecycleState::Failed
                ),
                LifecycleState::Diff => matches!(
                    to,
                    LifecycleState::Streaming | LifecycleState::Tool | LifecycleState::Completed
                ),
                LifecycleState::Permission => matches!(
                    to,
                    LifecycleState::Streaming
                        | LifecycleState::Tool
                        | LifecycleState::Cancelling
                        | LifecycleState::Failed
                ),
                LifecycleState::Question => matches!(
                    to,
                    LifecycleState::Streaming | LifecycleState::Tool | LifecycleState::Permission
                ),
                LifecycleState::Queued => {
                    matches!(to, LifecycleState::Submitting | LifecycleState::Idle)
                }
                LifecycleState::Interjected => {
                    matches!(to, LifecycleState::Streaming | LifecycleState::Cancelling)
                }
                LifecycleState::Cancelling => {
                    matches!(to, LifecycleState::Idle | LifecycleState::Failed)
                }
                LifecycleState::Recovering => matches!(
                    to,
                    LifecycleState::Streaming | LifecycleState::Idle | LifecycleState::Failed
                ),
                LifecycleState::Failed => matches!(
                    to,
                    LifecycleState::Idle | LifecycleState::Recovering | LifecycleState::Drafting
                ),
                LifecycleState::Completed => matches!(
                    to,
                    LifecycleState::Idle | LifecycleState::Drafting | LifecycleState::Compacting
                ),
                LifecycleState::Compacting => {
                    matches!(to, LifecycleState::Idle | LifecycleState::Streaming)
                }
            }
    }

    pub fn valid_targets(from: LifecycleState) -> Vec<LifecycleState> {
        LifecycleState::ALL
            .into_iter()
            .filter(|&to| Self::is_valid(from, to))
            .collect()
    }
}

impl Default for TransitionTable {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionError {
    InvalidTransition {
        from: LifecycleState,
        to: LifecycleState,
    },
}

impl fmt::Display for TransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTransition { from, to } => {
                write!(
                    formatter,
                    "invalid lifecycle transition: {} -> {}",
                    from.as_str(),
                    to.as_str()
                )
            }
        }
    }
}

impl std::error::Error for TransitionError {}
