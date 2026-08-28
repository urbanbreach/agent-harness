use harness_providers::CompletionUsage;
use serde::{Deserialize, Serialize};

use super::{CanonicalSession, SessionError};
use crate::event::{SessionCompactionEvent, UiIntentReceivedEvent};
use crate::ids::{EntryId, RunId};

mod adapter;
mod compaction;
mod facts;
mod projection;
mod provider_fragments;
mod recovery;

pub use recovery::{recover_event_history, LegacyHistoryRecovery, LegacyHistoryRecoveryError};

pub use super::EventIdentityNamespace as LegacyIdentityNamespace;
pub(crate) use compaction::{
    classify_compatibility_event, latest_legacy_compaction, legacy_projection_update_for_event,
    CompatibilityEvent, CompatibilityEventLifecycle,
};
pub use compaction::{CanonicalLegacyCompaction, CanonicalLegacyCompactionStatus};
pub use provider_fragments::{
    canonical_provider_fragment_for_event, CanonicalProviderFragment, CanonicalProviderFragmentKind,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct LegacyEventLogAdapter;

impl LegacyEventLogAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[derive(Debug, Clone)]
pub(super) struct LegacyCompactionFact {
    pub summary: String,
    pub first_kept_event_seq: u64,
    pub first_kept_entry_id: Option<EntryId>,
    pub tokens_after: Option<u32>,
    pub summary_usage: Option<CompletionUsage>,
    pub summary_provider_id: Option<String>,
    pub summary_model_id: Option<String>,
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
    pub current_intent: Option<UiIntentReceivedEvent>,
}

impl From<&SessionCompactionEvent> for LegacyCompactionFact {
    fn from(event: &SessionCompactionEvent) -> Self {
        Self {
            summary: event.summary.clone(),
            first_kept_event_seq: event.first_kept_event_seq,
            first_kept_entry_id: event.first_kept_entry_id.clone(),
            tokens_after: event.tokens_after,
            summary_usage: event.summary_usage.clone(),
            summary_provider_id: event.summary_provider_id.clone(),
            summary_model_id: event.summary_model_id.clone(),
            read_files: event.read_files.clone(),
            modified_files: event.modified_files.clone(),
            current_intent: event.current_intent.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LegacySessionSnapshot {
    pub session: CanonicalSession,
    pub provenance: LegacyProvenance,
    pub warnings: Vec<LegacyWarning>,
    pub audit_timeline: Vec<LegacyAuditReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyProvenance {
    pub schema_version: u16,
    pub source_run_id: RunId,
    pub source_event_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyAuditReference {
    pub sequence: u64,
    pub event_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LegacyWarning {
    InferredSessionIdentity,
    InferredTurnIdentity { correlation_id: Option<String> },
    MissingFinalAssistantContent { request_id: String },
    MissingAttachmentAssociation { request_id: String },
    MissingCompactionBoundary { first_kept_event_seq: u64 },
    MissingProviderFinish { request_id: String },
    MissingProviderAssociation { tool_call_id: String },
    MissingToolRequest { tool_call_id: String },
    DuplicateToolIdentity { tool_call_id: String },
    UnsupportedLegacyVariant { event_id: String },
    RecoveredCorruptFinalLine { line_number: usize },
}

impl From<super::journal::JournalRecoveryWarning> for LegacyWarning {
    fn from(warning: super::journal::JournalRecoveryWarning) -> Self {
        match warning {
            super::journal::JournalRecoveryWarning::RecoveredCorruptFinalLine { line_number } => {
                Self::RecoveredCorruptFinalLine { line_number }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LegacyAdapterError {
    #[error("legacy event history is empty")]
    EmptyInput,
    #[error("legacy schema version {actual} is unsupported; expected {expected}")]
    UnsupportedSchema { expected: u16, actual: u16 },
    #[error("legacy sequence {actual} is not contiguous after {expected_previous}")]
    NonContiguousSequence { expected_previous: u64, actual: u64 },
    #[error("legacy history mixes run {actual} into {expected}")]
    MixedRun { expected: RunId, actual: RunId },
    #[error("legacy event id {event_id} is duplicated")]
    DuplicateEvent { event_id: String },
    #[error("legacy event {event_id} has a malformed or foreign identity relationship")]
    InvalidIdentityRelationship { event_id: String },
    #[error("missing user message for completed request `{request_id}`")]
    MissingUserMessage { request_id: String },
    #[error(
        "missing user message for completed request `{request_id}` and prompt_summary is truncated"
    )]
    TruncatedUserPromptSummary { request_id: String },
    #[error("legacy facts violate the canonical session contract: {0}")]
    CanonicalProjection(#[from] SessionError),
}
