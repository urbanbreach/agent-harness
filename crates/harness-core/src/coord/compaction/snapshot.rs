use std::collections::BTreeMap;

use crate::ids::{EntryId, SessionId, TurnId};
use crate::session::{SessionEntry, SessionError};

/// Identifies the agent and root/child session that owns a compaction snapshot.
pub type CompactionOwner = crate::session::ProviderViewOwner;
pub type OwnedSession = crate::session::OwnedSession;

/// Provider and model selected for the request that may follow compaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentCompactionModel {
    pub provider_id: String,
    pub model_id: String,
}

impl CurrentCompactionModel {
    pub fn new(provider_id: impl Into<String>, model_id: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id.into(),
            model_id: model_id.into(),
        }
    }
}

/// User input waiting to be appended after the historical snapshot.
pub type PendingCompactionPrompt = crate::session::CanonicalPendingPrompt;

/// Optional legacy event sequence annotations keyed by canonical entry identity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LegacySourceSequences(BTreeMap<EntryId, u64>);

impl LegacySourceSequences {
    /// Builds a deterministic identity-to-sequence map.
    ///
    /// # Errors
    /// Returns [`CompactionSnapshotError::DuplicateLegacySourceSequence`] when an entry identity
    /// appears more than once, even if both values are equal.
    pub fn new(
        entries: impl IntoIterator<Item = (EntryId, u64)>,
    ) -> Result<Self, CompactionSnapshotError> {
        let mut sequences = BTreeMap::new();
        for (entry_id, sequence) in entries {
            if sequences.insert(entry_id.clone(), sequence).is_some() {
                return Err(CompactionSnapshotError::DuplicateLegacySourceSequence { entry_id });
            }
        }
        Ok(Self(sequences))
    }

    pub fn sequence_for(&self, entry_id: &EntryId) -> Option<u64> {
        self.0.get(entry_id).copied()
    }
}

/// Inputs required to derive one canonical active-path snapshot.
pub struct ActivePathCompactionSnapshotInput<'a> {
    pub session: &'a crate::session::CanonicalSession,
    pub owner: CompactionOwner,
    pub legacy_source_sequences: &'a LegacySourceSequences,
    pub pending_prompt: Option<PendingCompactionPrompt>,
    pub current_model: CurrentCompactionModel,
}

/// Canonical identity of the selected branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveCompactionBranch {
    pub leaf_entry_id: Option<EntryId>,
    pub entry_ids: Vec<EntryId>,
}

/// A protocol-safe active-path entry annotated only with optional legacy output sequence.
#[derive(Debug, Clone, PartialEq)]
pub struct CompactionSnapshotEntry {
    pub entry: SessionEntry,
    pub legacy_source_sequence: Option<u64>,
    pub tool_pairs: Vec<ToolPairIdentity>,
}

/// Stable identity shared by a canonical tool call and its sole result.
pub type ToolPairIdentity = crate::session::CanonicalToolPair;

/// The latest active compaction summary, separated from history to prevent old-summary replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorActiveCompactionSummary {
    pub entry_id: EntryId,
    pub summary: String,
    pub first_kept_entry_id: EntryId,
    pub legacy_source_sequence: Option<u64>,
}

/// Immutable compaction input projected from exactly one canonical active path.
#[derive(Debug, Clone, PartialEq)]
pub struct ActivePathCompactionSnapshot {
    pub owner: CompactionOwner,
    pub active_branch: ActiveCompactionBranch,
    pub entries: Vec<CompactionSnapshotEntry>,
    pub pending_prompt: Option<PendingCompactionPrompt>,
    pub prior_active_summary: Option<PriorActiveCompactionSummary>,
    pub current_model: CurrentCompactionModel,
}

impl ActivePathCompactionSnapshot {
    /// Selects a typed first-kept boundary without consulting legacy history.
    ///
    /// # Errors
    /// Returns [`CompactionSnapshotError::BoundaryOffActivePath`] when the entry is absent from the
    /// protocol-safe snapshot.
    pub fn into_plan(
        self,
        first_kept_entry_id: &EntryId,
    ) -> Result<ActivePathCompactionPlan, CompactionSnapshotError> {
        let first_kept = self
            .entries
            .iter()
            .find(|candidate| candidate.entry.id == *first_kept_entry_id)
            .map(|candidate| CompactionPlanBoundary {
                entry_id: candidate.entry.id.clone(),
                turn_id: candidate.entry.turn_id.clone(),
                legacy_source_sequence: candidate.legacy_source_sequence,
            })
            .ok_or_else(|| CompactionSnapshotError::BoundaryOffActivePath {
                entry_id: first_kept_entry_id.clone(),
            })?;
        Ok(ActivePathCompactionPlan {
            snapshot: self,
            first_kept,
        })
    }
}

/// Typed plan seam consumed by cut selection and legacy event output.
#[derive(Debug, Clone, PartialEq)]
pub struct ActivePathCompactionPlan {
    pub snapshot: ActivePathCompactionSnapshot,
    pub first_kept: CompactionPlanBoundary,
}

/// Canonical first-kept identity plus optional legacy output coordinate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionPlanBoundary {
    pub entry_id: EntryId,
    pub turn_id: Option<TurnId>,
    pub legacy_source_sequence: Option<u64>,
}

/// Pure snapshot-construction failures.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CompactionSnapshotError {
    #[error(transparent)]
    InvalidSession(#[from] SessionError),
    #[error("compaction owner session {actual} does not match canonical session {expected}")]
    OwnerSessionMismatch {
        expected: SessionId,
        actual: SessionId,
    },
    #[error(transparent)]
    InvalidProviderView(crate::session::ProviderViewError),
    #[error("legacy source sequence repeats canonical entry {entry_id}")]
    DuplicateLegacySourceSequence { entry_id: EntryId },
    #[error("compaction boundary entry {entry_id} is not on the protocol-safe active path")]
    BoundaryOffActivePath { entry_id: EntryId },
}
