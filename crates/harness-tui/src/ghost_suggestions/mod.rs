#![allow(
    clippy::mod_module_files,
    reason = "The ghost suggestion facade intentionally groups focused sibling modules"
)]

use std::fmt::{Display, Formatter};

use crate::composer_editing::EditingError;

mod controller;
mod debounce;
mod generation;
mod render;
mod secret_safe;

pub use controller::{Suggestion, SuggestionController};
pub use debounce::{Debouncer, Request, SuggestionContext, DEFAULT_DEBOUNCE_MS};
pub use generation::{GenerationError, Invalidation, SuggestionGeneration};
pub use render::{muted_style, render_ghost};
pub use secret_safe::SecretSuggestionSink;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuggestionError {
    Generation(GenerationError),
    StaleGeneration {
        expected: SuggestionGeneration,
        received: SuggestionGeneration,
    },
    RequestMismatch,
    NoCurrentSuggestion,
    ZeroPartialUnits,
    TooManyPartialUnits {
        requested: usize,
        available: usize,
    },
    Composer(EditingError),
    PersistenceForbidden,
}

impl Display for SuggestionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Generation(error) => Display::fmt(error, formatter),
            Self::StaleGeneration { expected, received } => write!(
                formatter,
                "stale suggestion generation {}; expected {}",
                received.value(),
                expected.value()
            ),
            Self::RequestMismatch => {
                formatter.write_str("suggestion response does not match the pending request")
            }
            Self::NoCurrentSuggestion => formatter.write_str("no current ghost suggestion"),
            Self::ZeroPartialUnits => {
                formatter.write_str("partial acceptance requires at least one grapheme")
            }
            Self::TooManyPartialUnits {
                requested,
                available,
            } => write!(
                formatter,
                "partial acceptance requested {requested} graphemes, but only {available} remain"
            ),
            Self::Composer(error) => Display::fmt(error, formatter),
            Self::PersistenceForbidden => formatter.write_str("ghost suggestions are memory-only"),
        }
    }
}

impl std::error::Error for SuggestionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Generation(error) => Some(error),
            Self::Composer(error) => Some(error),
            Self::StaleGeneration { .. }
            | Self::RequestMismatch
            | Self::NoCurrentSuggestion
            | Self::ZeroPartialUnits
            | Self::TooManyPartialUnits { .. }
            | Self::PersistenceForbidden => None,
        }
    }
}

impl From<GenerationError> for SuggestionError {
    fn from(error: GenerationError) -> Self {
        Self::Generation(error)
    }
}

impl From<EditingError> for SuggestionError {
    fn from(error: EditingError) -> Self {
        Self::Composer(error)
    }
}
