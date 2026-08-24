use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{RecordSequence, SessionEntry, SessionError};
use crate::ids::{EntryId, RunId, SessionId};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalSession {
    pub(crate) session_id: SessionId,
    pub(crate) watermark: Option<RecordSequence>,
    pub(crate) entries: BTreeMap<EntryId, SessionEntry>,
    pub(crate) active_leaf: Option<EntryId>,
    pub(crate) run_attempts: BTreeMap<RunId, RunAttempt>,
    pub(crate) status: SessionStatus,
    pub(crate) metadata: SessionMetadata,
}

impl CanonicalSession {
    pub fn empty(session_id: SessionId) -> Self {
        Self {
            session_id,
            watermark: None,
            entries: BTreeMap::new(),
            active_leaf: None,
            run_attempts: BTreeMap::new(),
            status: SessionStatus::Active,
            metadata: SessionMetadata {
                title: None,
                custom: BTreeMap::new(),
            },
        }
    }

    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    pub const fn watermark(&self) -> Option<RecordSequence> {
        self.watermark
    }

    pub fn entries(&self) -> &BTreeMap<EntryId, SessionEntry> {
        &self.entries
    }

    pub fn active_leaf(&self) -> Option<&EntryId> {
        self.active_leaf.as_ref()
    }

    pub fn run_attempts(&self) -> &BTreeMap<RunId, RunAttempt> {
        &self.run_attempts
    }

    pub const fn status(&self) -> SessionStatus {
        self.status
    }

    pub const fn metadata(&self) -> &SessionMetadata {
        &self.metadata
    }

    pub fn active_path(&self) -> Result<Vec<&SessionEntry>, SessionError> {
        super::reducer::active_path(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunAttempt {
    pub run_id: RunId,
    pub status: RunStatus,
    pub legacy_run_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Active,
    Completed,
    Failed,
    Cancelled,
}

impl RunStatus {
    pub const fn is_terminal(self) -> bool {
        match self {
            Self::Active => false,
            Self::Completed | Self::Failed | Self::Cancelled => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    Completed,
    Failed,
    Cancelled,
}

impl SessionStatus {
    pub const fn is_terminal(self) -> bool {
        match self {
            Self::Active => false,
            Self::Completed | Self::Failed | Self::Cancelled => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub title: Option<String>,
    pub custom: BTreeMap<String, Value>,
}
