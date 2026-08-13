#![allow(
    clippy::module_name_repetitions,
    clippy::mod_module_files,
    reason = "the composer integration facade groups the task-owned composition modules"
)]

mod controllers;
mod hit_map;
mod motion;
mod presentation;
mod presentation_adapter;
mod presentation_error;
mod slice;
mod submission;
mod view_helpers;
mod view_model;

pub use crate::app::interaction_reducer::UiIntent as InteractionUiIntent;
pub use crate::design_contract::ViewportId;
pub use hit_map::{ComposerHitMap, ComposerHitRegion, ComposerHitTarget};
pub use motion::{ComposerMotion, ComposerMotionFrame};
pub use presentation::{
    ComposerChrome, ComposerEditorModel, ComposerPresentation, ComposerPresentationConfig,
    ComposerSurface, ComposerTone,
};
pub use presentation_error::ComposerPresentationError;
pub use slice::{AttachmentEntry, ComposerSlice, ComposerSliceError};
pub use submission::{ComposerUiIntent, SubmissionAttachment, SubmissionError, UiIntent};
pub use view_model::{
    AttachmentPreviewViewModel, CompletionViewModel, ComposerBorderViewModel, ComposerViewModel,
    GhostSuggestionViewModel,
};

#[cfg(test)]
mod presentation_tests;
