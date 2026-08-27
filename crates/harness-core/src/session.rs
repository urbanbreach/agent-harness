#![allow(
    clippy::mod_module_files,
    reason = "The canonical session facade intentionally groups focused sibling modules"
)]

mod entry;
mod error;
pub mod legacy;
mod model;
mod projection;
mod provider_view;
mod record;
pub mod reducer;

pub use entry::{
    AssistantPart, AssistantToolCall, CompactionPreservedState, ProviderProvenance, SessionEntry,
    SessionEntryPayload, ToolResultStatus,
};
pub use error::SessionError;
pub use model::{CanonicalSession, RunAttempt, RunStatus, SessionMetadata, SessionStatus};
pub use projection::{CanonicalSessionProjection, CanonicalSessionProjectionError};
pub(crate) use provider_view::select_active_path as select_provider_active_path;
pub use provider_view::{
    CanonicalAttachment, CanonicalCompactionSummary, CanonicalPendingPrompt, CanonicalProviderView,
    CanonicalRuntimeSelection, CanonicalToolPair, CanonicalUsageBoundary, OwnedSession,
    ProviderViewError, ProviderViewInput, ProviderViewOwner, UsageBoundaryKind,
};
pub use record::{CanonicalRecord, CanonicalRecordKind, RecordSequence};
