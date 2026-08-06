use super::transitions::{LifecycleState, TransitionError, TransitionTable};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleSnapshot {
    pub state: LifecycleState,
    pub pending_permissions: u8,
    pub queued_prompts: u8,
    pub recovering: bool,
    pub tick: u64,
}

impl LifecycleSnapshot {
    pub fn new() -> Self {
        Self {
            state: LifecycleState::Idle,
            pending_permissions: 0,
            queued_prompts: 0,
            recovering: false,
            tick: 0,
        }
    }

    pub fn rest_frame(&self) -> bool {
        matches!(
            self.state,
            LifecycleState::Idle | LifecycleState::Completed | LifecycleState::Failed
        ) && self.pending_permissions == 0
            && self.queued_prompts == 0
    }

    pub const fn is_composite_impossible(&self) -> bool {
        false
    }
}

impl Default for LifecycleSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

/// Single authority for lifecycle state; counts accompany state but do not drive transitions.
pub struct LifecycleAuthority {
    snapshot: LifecycleSnapshot,
    transition_table: TransitionTable,
}

impl LifecycleAuthority {
    pub fn new() -> Self {
        Self {
            snapshot: LifecycleSnapshot::new(),
            transition_table: TransitionTable::new(),
        }
    }

    pub const fn snapshot(&self) -> &LifecycleSnapshot {
        &self.snapshot
    }

    pub fn transition(&mut self, to: LifecycleState) -> Result<(), TransitionError> {
        let from = self.snapshot.state;
        if !TransitionTable::is_valid(from, to) {
            return Err(TransitionError::InvalidTransition { from, to });
        }
        let _ = &self.transition_table;
        self.snapshot.state = to;
        Ok(())
    }

    pub fn set_pending_permissions(&mut self, count: u8) {
        self.snapshot.pending_permissions = count;
    }

    pub fn set_queued_prompts(&mut self, count: u8) {
        self.snapshot.queued_prompts = count;
    }

    pub fn set_recovering(&mut self, recovering: bool) {
        self.snapshot.recovering = recovering;
    }

    pub fn tick(&mut self) {
        self.snapshot.tick += 1;
    }
}

impl Default for LifecycleAuthority {
    fn default() -> Self {
        Self::new()
    }
}
