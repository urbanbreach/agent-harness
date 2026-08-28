#![allow(
    clippy::mod_module_files,
    reason = "The canonical session facade intentionally groups focused sibling modules"
)]

mod entry;
mod error;
pub mod history_index;
mod identity;
pub mod journal;
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
pub use identity::EventIdentityNamespace;
pub use legacy::LegacyWarning as CompatibilityWarning;
pub use legacy::{
    canonical_provider_fragment_for_event, canonical_provider_fragment_payload,
    CanonicalLegacyCompaction, CanonicalLegacyCompactionStatus, CanonicalProviderFragment,
    CanonicalProviderFragmentKind, CanonicalProviderFragmentPayload,
};
pub(crate) use legacy::{
    classify_compatibility_event, CompatibilityEvent, CompatibilityEventLifecycle,
};
pub use model::{CanonicalSession, RunAttempt, RunStatus, SessionMetadata, SessionStatus};
pub use projection::{
    canonical_projection_update_for_event, CanonicalBackgroundNotification, CanonicalEditEvent,
    CanonicalEditPayload, CanonicalProjectionUpdate, CanonicalProviderRequestFinish,
    CanonicalProviderRequestStart, CanonicalSessionProjection, CanonicalSessionProjectionError,
    CanonicalStaleDetection,
};
pub(crate) use provider_view::select_active_path as select_provider_active_path;
pub use provider_view::{
    CanonicalAttachment, CanonicalCompactionSummary, CanonicalPendingPrompt, CanonicalProviderView,
    CanonicalRuntimeSelection, CanonicalToolPair, CanonicalUsageBoundary, OwnedSession,
    ProviderViewError, ProviderViewInput, ProviderViewOwner, UsageBoundaryKind,
};
pub use record::{CanonicalRecord, CanonicalRecordKind, RecordSequence};
