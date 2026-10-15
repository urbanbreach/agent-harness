//! Per-agent compaction state that outlives a single pass. These types only hold data;
//! the maintenance, manual and notes passes own their transitions.
use std::collections::BTreeSet;

/// Root-chain compaction settings a worker captures when it spawns, so a child keeps the
/// threshold pair its root agent had even when the live settings change underneath it.
#[derive(Debug, Clone, Default, PartialEq)]
pub(in crate::coord) struct CompactionOverlay {
    /// Percent of the window at which compaction triggers, from the root chain.
    pub threshold_percent: Option<f64>,
    /// Fixed trigger tokens, from the root chain.
    pub threshold_tokens: Option<u32>,
    /// `enabled` as the root chain resolved it; `None` follows the live setting.
    pub enabled: Option<bool>,
    /// A named agent override disabled the per-model threshold maps.
    pub ignore_model_thresholds: bool,
    /// Per-spawn override of mid-turn maintenance; `None` keeps the default of on.
    pub mid_turn_enabled: Option<bool>,
    /// A child of a child keeps the pair its parent captured instead of re-reading it.
    pub pinned: bool,
}

/// Compaction contexts that already failed to make room, so the next trigger does not
/// repeat the same attempt, plus the one-shot marker for a futile pre-prompt rescue.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::coord) struct ParkedSet {
    identities: BTreeSet<String>,
    futile_pre_prompt: bool,
}

impl ParkedSet {
    pub fn park(&mut self, identity: impl Into<String>) {
        self.identities.insert(identity.into());
    }

    pub fn is_parked(&self, identity: &str) -> bool {
        self.identities.contains(identity)
    }

    /// Forgets every parked context. A commit, rewind or new context calls this.
    pub fn clear(&mut self) {
        self.identities.clear();
        self.futile_pre_prompt = false;
    }

    pub fn mark_futile_pre_prompt(&mut self) {
        self.futile_pre_prompt = true;
    }

    /// Consumes the marker; the next user pre-prompt sees it exactly once.
    pub fn take_futile_pre_prompt(&mut self) -> bool {
        std::mem::take(&mut self.futile_pre_prompt)
    }
}

pub(in crate::coord) use super::manual::ManualClaims;

/// What the notes-backed context window mode tracks for one agent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::coord) struct NotesState {
    /// Window key the near-threshold reminder was last queued for.
    pub reminded_window: Option<String>,
    /// A reminder is waiting for the next aside boundary.
    pub reminder_queued: bool,
    /// Generation of the prompt the queued reminder was built for.
    pub reminder_generation: u64,
    /// Revision of the latest notebook, once one exists.
    pub version: u8,
    /// Turn whose `new_context` call was acknowledged and not yet acted on.
    pub new_context_turn: Option<String>,
}
