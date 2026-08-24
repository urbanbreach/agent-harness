use serde::{Deserialize, Serialize};

use super::{RunAttempt, RunStatus, SessionEntry, SessionMetadata, SessionStatus};
use crate::ids::{EntryId, RunId, SessionId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RecordSequence(u64);

impl RecordSequence {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalRecord {
    pub session_id: SessionId,
    pub sequence: RecordSequence,
    pub kind: CanonicalRecordKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CanonicalRecordKind {
    RunStarted { attempt: RunAttempt },
    RunStatusChanged { run_id: RunId, status: RunStatus },
    EntryCommitted { entry: SessionEntry },
    ActiveLeafSelected { entry_id: EntryId },
    SessionMetadataUpdated { metadata: SessionMetadata },
    SessionStatusChanged { status: SessionStatus },
}
