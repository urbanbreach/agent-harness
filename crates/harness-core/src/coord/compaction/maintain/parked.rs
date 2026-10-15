//! Contexts a mid-turn dead end already failed to shrink.
//!
//! The set keeps one identity per live turn array, so the tool loop is not asked to
//! compact again until a new boundary makes a cut point possible. The runtime clears an
//! agent's set when a commit, rewind or new context changes its history.
use crate::coord::{turn::Worker, CoordinatorError};

impl Worker {
    /// Stable for as long as this turn's message array lives; a later turn gets a new one.
    pub(in crate::coord) fn context_identity(&self) -> String {
        self.turn.id.clone()
    }

    pub(in crate::coord) fn park_context(&mut self) {
        let identity = self.context_identity();
        self.parked.park(identity);
    }

    pub(in crate::coord) fn unpark_context(&mut self) {
        // A new boundary made a cut point possible: the identity and the one-shot marker go.
        self.parked.clear();
    }

    pub(in crate::coord) fn is_parked(&self) -> bool {
        self.parked.is_parked(&self.context_identity())
    }

    pub(in crate::coord) fn mark_futile_pre_prompt(&mut self) {
        self.parked.mark_futile_pre_prompt();
    }

    /// Consumes the one-shot marker the next user pre-prompt looks at.
    pub(in crate::coord) fn take_futile_pre_prompt(&mut self) -> bool {
        self.parked.take_futile_pre_prompt()
    }

    /// Writes this worker's set back so the next turn of the agent starts from it.
    pub(in crate::coord) async fn sync_parked(&self) -> Result<(), CoordinatorError> {
        let agent = self.actor.agent_id.clone().unwrap_or_default();
        let parked = self.parked.clone();
        self.handle
            .call(move |s| {
                s.parked.insert(agent, parked);
                Ok(())
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use crate::coord::compaction::state::ParkedSet;

    #[test]
    fn parking_is_per_identity() {
        let mut set = ParkedSet::default();
        set.park("turn-1");
        for (identity, parked) in [("turn-1", true), ("turn-2", false)] {
            assert_eq!(set.is_parked(identity), parked, "{identity}");
        }
    }

    #[test]
    fn clear_forgets_identity_and_marker() {
        let mut set = ParkedSet::default();
        set.park("turn-1");
        set.mark_futile_pre_prompt();
        set.clear();
        assert!(!set.is_parked("turn-1"));
        assert!(!set.take_futile_pre_prompt());
    }

    #[test]
    fn marker_is_consumed_once() {
        let mut set = ParkedSet::default();
        set.mark_futile_pre_prompt();
        assert!(set.take_futile_pre_prompt());
        assert!(!set.take_futile_pre_prompt());
    }
}
