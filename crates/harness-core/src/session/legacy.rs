use harness_providers::CompletionUsage;
use serde::{Deserialize, Serialize};

use super::{CanonicalSession, SessionError};
use crate::digest::digest32;
use crate::event::{EventEnvelopeV1, EventV1, SessionCompactionEvent, UiIntentReceivedEvent};
use crate::ids::{EntryId, ProviderRequestId, RunId, SessionId, TurnId};

mod adapter;
mod compaction;
mod facts;
mod projection;

pub(crate) use compaction::{
    checkpoint_artifact as legacy_checkpoint_artifact,
    compaction_lifecycle as legacy_compaction_lifecycle,
    discover_applied_checkpoints as discover_legacy_applied_checkpoints,
    event_type_name as legacy_compaction_event_type_name,
    is_compaction_event as is_legacy_compaction_event, load_checkpoint as load_legacy_checkpoint,
    LegacyCheckpointRecord, LegacyCompactionLifecycle,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct LegacyEventLogAdapter;

impl LegacyEventLogAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[derive(Debug, Clone)]
pub struct LegacyIdentityNamespace<'a> {
    run_id: &'a RunId,
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

impl<'a> LegacyIdentityNamespace<'a> {
    pub const fn new(run_id: &'a RunId) -> Self {
        Self { run_id }
    }

    pub fn session_id(&self) -> SessionId {
        SessionId::new(format!(
            "legacy-session-{}",
            digest32(format!("session\0{}", self.run_id).as_bytes())
        ))
    }

    pub fn entry_id(&self, sequence: u64, event_id: &str, semantic_kind: &str) -> EntryId {
        EntryId::new(format!(
            "legacy-entry-{}",
            digest32(
                format!(
                    "entry\0{}\0{sequence}\0{event_id}\0{semantic_kind}",
                    self.run_id
                )
                .as_bytes()
            )
        ))
    }

    #[expect(
        deprecated,
        reason = "the V1 compatibility boundary must exhaust deprecated V1 variants"
    )]
    pub(crate) fn compaction_boundary_entry_id(
        &self,
        events: &[EventEnvelopeV1],
        sequence: u64,
    ) -> Option<EntryId> {
        let boundary = events.iter().find(|event| event.seq == sequence)?;
        match &boundary.payload {
            EventV1::UserMessageSubmitted(_) => {
                Some(self.entry_id(boundary.seq, &boundary.event_id, "user_message"))
            }
            EventV1::AssistantMessageFinished(finished) => events
                .iter()
                .find(|event| {
                    matches!(
                        &event.payload,
                        EventV1::ProviderRequestStarted(started)
                            if started.request_id.as_str() == finished.request_id.as_str()
                    )
                })
                .map(|started| self.entry_id(started.seq, &started.event_id, "assistant_message")),
            EventV1::RunStarted(_)
            | EventV1::SessionTitleUpdated(_)
            | EventV1::RunFinished(_)
            | EventV1::RunFailed(_)
            | EventV1::AgentSpawned(_)
            | EventV1::AgentStopped(_)
            | EventV1::TaskScheduled(_)
            | EventV1::TaskCancelled(_)
            | EventV1::TaskCompleted(_)
            | EventV1::TaskResultLate(_)
            | EventV1::BackgroundTaskNotification(_)
            | EventV1::StaleDetected(_)
            | EventV1::PromptAttachmentsSubmitted(_)
            | EventV1::ProviderRequestStarted(_)
            | EventV1::ProviderStreamDelta(_)
            | EventV1::ProviderReasoningDelta(_)
            | EventV1::ProviderRequestFinished(_)
            | EventV1::CompactionRequested(_)
            | EventV1::CompactionWritten(_)
            | EventV1::CompactionApplied(_)
            | EventV1::CompactionFailed(_)
            | EventV1::SessionCompaction(_)
            | EventV1::BranchSummary(_)
            | EventV1::ToolCallRequested(_)
            | EventV1::ToolCallStarted(_)
            | EventV1::ToolCallFinished(_)
            | EventV1::PermissionRequested(_)
            | EventV1::PermissionGrantRecorded(_)
            | EventV1::PermissionResolved(_)
            | EventV1::EditProposed(_)
            | EventV1::EditApplied(_)
            | EventV1::EditRejected(_)
            | EventV1::ArtifactWritten(_)
            | EventV1::PolicyViolationDetected(_)
            | EventV1::UiIntentReceived(_)
            | EventV1::WorkspaceSnapshot(_)
            | EventV1::WorkspaceReverted(_) => None,
        }
    }

    pub fn turn_id(&self, correlation_id: &str) -> TurnId {
        TurnId::new(format!(
            "legacy-turn-{}",
            digest32(format!("turn\0{}\0{correlation_id}", self.run_id).as_bytes())
        ))
    }

    pub fn provider_request_id(&self, request_id: &str) -> ProviderRequestId {
        ProviderRequestId::new(format!(
            "legacy-provider-request-{}",
            digest32(format!("provider-request\0{}\0{request_id}", self.run_id).as_bytes())
        ))
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
    UnsupportedLegacyVariant { event_id: String },
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
    #[error("legacy facts violate the canonical session contract: {0}")]
    CanonicalProjection(#[from] SessionError),
}
