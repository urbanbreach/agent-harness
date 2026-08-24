use serde::{Deserialize, Serialize};

use super::{CanonicalSession, SessionError};
use crate::digest::digest32;
use crate::ids::{EntryId, ProviderRequestId, RunId, SessionId, TurnId};

mod adapter;
mod facts;
mod projection;

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
