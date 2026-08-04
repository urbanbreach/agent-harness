//! Unified completion state for slash, file, shell, and history suggestions.

use std::fmt::{Display, Formatter};

use crate::composer_atoms::{AtomBufferError, AtomId};

mod controller;
mod geometry;
mod insertion;
mod precedence;
mod trigger;

pub use controller::{
    CompletionAcceptance, CompletionController, CompletionGeneration, CompletionItem,
    CompletionRequest, CompletionStatus, SelectionDirection,
};
pub use geometry::{CompletionDropdownGeometry, CompletionGeometryInput, ShellCompletionGeometry};
pub use insertion::insert_completion;
pub use precedence::{choose_preferred_trigger, precedence_table};
pub use trigger::{CompletionRange, CompletionSource, CompletionTrigger};

/// Errors raised when a completion request, selection, or insertion is stale or invalid.
#[derive(Debug)]
pub enum CompletionError {
    InvalidRange {
        start: usize,
        end: usize,
    },
    StaleResults {
        expected: CompletionGeneration,
        received: CompletionGeneration,
    },
    NoActiveCompletion,
    NoSelection,
    SelectionOutOfBounds {
        index: usize,
        len: usize,
    },
    ProtectedAtom(AtomId),
    AtomBuffer(AtomBufferError),
}

impl Display for CompletionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRange { start, end } => {
                write!(formatter, "completion range is reversed: {start}..{end}")
            }
            Self::StaleResults { expected, received } => {
                write!(
                    formatter,
                    "stale completion generation {received}; expected {expected}"
                )
            }
            Self::NoActiveCompletion => formatter.write_str("no active completion"),
            Self::NoSelection => formatter.write_str("no completion item is selected"),
            Self::SelectionOutOfBounds { index, len } => {
                write!(
                    formatter,
                    "completion selection {index} is outside {len} items"
                )
            }
            Self::ProtectedAtom(id) => {
                write!(
                    formatter,
                    "completion range contains protected atom {}",
                    id.get()
                )
            }
            Self::AtomBuffer(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for CompletionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::AtomBuffer(error) => Some(error),
            Self::InvalidRange { .. }
            | Self::StaleResults { .. }
            | Self::NoActiveCompletion
            | Self::NoSelection
            | Self::SelectionOutOfBounds { .. }
            | Self::ProtectedAtom(_) => None,
        }
    }
}

impl From<AtomBufferError> for CompletionError {
    fn from(error: AtomBufferError) -> Self {
        Self::AtomBuffer(error)
    }
}
