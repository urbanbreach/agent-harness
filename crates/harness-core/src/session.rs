#![allow(
    clippy::mod_module_files,
    reason = "The canonical session facade intentionally groups focused sibling modules"
)]

mod entry;
mod error;
pub mod legacy;
mod model;
mod record;
pub mod reducer;

pub use entry::{
    AssistantPart, AssistantToolCall, CompactionPreservedState, ProviderProvenance, SessionEntry,
    SessionEntryPayload, ToolResultStatus,
};
pub use error::SessionError;
pub use model::{CanonicalSession, RunAttempt, RunStatus, SessionMetadata, SessionStatus};
pub use record::{CanonicalRecord, CanonicalRecordKind, RecordSequence};
