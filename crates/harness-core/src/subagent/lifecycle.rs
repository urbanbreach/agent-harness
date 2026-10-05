use std::collections::{HashMap, VecDeque};

const ATTEMPT_HISTORY_LIMIT: usize = 8;

/// Lifecycle notification received from a subagent runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleTransition {
    Spawned,
    Progress,
    Finished,
}

/// Current externally visible attempt phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecyclePhase {
    Running,
    Finished,
}

/// Identity for attempt-aware events; absent attempt IDs share the legacy key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SubagentAttemptKey {
    Legacy,
    Id(String),
}

impl SubagentAttemptKey {
    /// Construct a key from the optional wire attempt identifier.
    pub fn from_wire(attempt_id: Option<&str>) -> Self {
        attempt_id.map_or(Self::Legacy, |id| Self::Id(id.to_owned()))
    }

    /// Return the typed attempt identifier, if present.
    pub fn attempt_id(&self) -> Option<&str> {
        match self {
            Self::Legacy => None,
            Self::Id(id) => Some(id),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttemptPhase {
    PendingFinish,
    Running,
    Finished,
}

#[derive(Debug, Clone)]
struct AttemptLifecycle {
    phase: AttemptPhase,
    last_event_seq: Option<u64>,
}

/// Bounded attempt history for one subagent identity.
#[derive(Debug, Clone, Default)]
pub struct LifecycleState {
    current: Option<SubagentAttemptKey>,
    attempts: HashMap<SubagentAttemptKey, AttemptLifecycle>,
    order: VecDeque<SubagentAttemptKey>,
    last_spawn_seq: Option<u64>,
}

/// Coordinator-facing effect associated with an accepted transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleEffect {
    Apply,
    ApplyNewAttempt,
    ApplyWithPendingFinish,
    AwaitSpawn,
    RecordOnly,
}

/// Result of reducing one lifecycle event.
#[derive(Debug, Clone)]
pub enum LifecycleReduction {
    Accepted(AcceptedLifecycle),
    Dropped,
}

/// A candidate state and effect; state changes only when explicitly committed.
#[derive(Debug, Clone)]
pub struct AcceptedLifecycle {
    next: LifecycleState,
    effect: LifecycleEffect,
}

impl LifecycleReduction {
    /// Return the effect for an accepted event.
    pub fn effect(&self) -> Option<LifecycleEffect> {
        match self {
            Self::Accepted(accepted) => Some(accepted.effect),
            Self::Dropped => None,
        }
    }

    /// Commit an accepted candidate state.
    pub fn commit(self, state: &mut LifecycleState) -> Option<LifecycleEffect> {
        match self {
            Self::Accepted(accepted) => {
                let effect = accepted.effect;
                *state = accepted.next;
                Some(effect)
            }
            Self::Dropped => None,
        }
    }

    /// Consume the reduction and return its candidate state, if accepted.
    pub fn into_state(self) -> Option<LifecycleState> {
        match self {
            Self::Accepted(accepted) => Some(accepted.next),
            Self::Dropped => None,
        }
    }
}

impl LifecycleState {
    /// Reduce an event without mutating this state.
    pub fn reduce(
        &self,
        transition: LifecycleTransition,
        attempt_id: Option<&str>,
        event_seq: Option<u64>,
    ) -> LifecycleReduction {
        let key = SubagentAttemptKey::from_wire(attempt_id);
        match transition {
            LifecycleTransition::Spawned => self.reduce_spawn(key, event_seq),
            LifecycleTransition::Progress => self.reduce_progress(&key, event_seq),
            LifecycleTransition::Finished => self.reduce_finish(key, event_seq),
        }
    }

    /// Return the current attempt key.
    pub fn current_attempt_key(&self) -> Option<&SubagentAttemptKey> {
        self.current.as_ref()
    }

    /// Return the current typed attempt ID.
    pub fn current_attempt_id(&self) -> Option<&str> {
        self.current
            .as_ref()
            .and_then(SubagentAttemptKey::attempt_id)
    }

    /// Return the current attempt's visible phase.
    pub fn phase(&self) -> Option<LifecyclePhase> {
        self.current.as_ref().and_then(|key| {
            self.attempts
                .get(key)
                .and_then(|attempt| match attempt.phase {
                    AttemptPhase::Running => Some(LifecyclePhase::Running),
                    AttemptPhase::Finished => Some(LifecyclePhase::Finished),
                    AttemptPhase::PendingFinish => None,
                })
        })
    }

    /// Return whether the current attempt has finished.
    pub fn is_finished(&self) -> bool {
        self.phase() == Some(LifecyclePhase::Finished)
    }

    /// Return whether a current attempt has been observed.
    pub fn has_current_attempt(&self) -> bool {
        self.current.is_some()
    }

    /// Return whether an attempt key remains in bounded history.
    pub fn retains_attempt(&self, key: &SubagentAttemptKey) -> bool {
        self.attempts.contains_key(key)
    }

    /// Remove a pending finish when its separately held payload is discarded.
    pub fn forget_pending_attempt(&mut self, key: &SubagentAttemptKey) {
        if self
            .attempts
            .get(key)
            .is_some_and(|attempt| attempt.phase == AttemptPhase::PendingFinish)
        {
            self.attempts.remove(key);
            self.order.retain(|candidate| candidate != key);
        }
    }

    /// Remove pending keys whose external deferred-finish payload no longer exists.
    pub fn forget_unbacked_pending_attempts(
        &mut self,
        mut is_backed: impl FnMut(&SubagentAttemptKey) -> bool,
    ) {
        let orphaned: Vec<_> = self
            .attempts
            .iter()
            .filter(|(key, attempt)| {
                attempt.phase == AttemptPhase::PendingFinish && !is_backed(key)
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in orphaned {
            self.forget_pending_attempt(&key);
        }
    }

    fn reduce_spawn(&self, key: SubagentAttemptKey, event_seq: Option<u64>) -> LifecycleReduction {
        if self.last_spawn_seq.is_some()
            && event_seq.is_none()
            && (key != SubagentAttemptKey::Legacy || !self.attempts.contains_key(&key))
        {
            return LifecycleReduction::Dropped;
        }
        if event_seq
            .zip(self.last_spawn_seq)
            .is_some_and(|(incoming, high_water)| incoming <= high_water)
        {
            return LifecycleReduction::Dropped;
        }
        if let Some(existing) = self.attempts.get(&key) {
            if key == SubagentAttemptKey::Legacy
                && self.current.as_ref() == Some(&key)
                && existing.phase == AttemptPhase::Finished
                && event_seq.is_some()
                && is_newer(event_seq, existing.last_event_seq)
            {
                let mut next = self.clone();
                next.attempts.insert(
                    key,
                    AttemptLifecycle {
                        phase: AttemptPhase::Running,
                        last_event_seq: max_seq(existing.last_event_seq, event_seq),
                    },
                );
                next.last_spawn_seq = max_seq(next.last_spawn_seq, event_seq);
                return accepted(next, LifecycleEffect::ApplyNewAttempt);
            }
            if existing.phase == AttemptPhase::PendingFinish {
                let mut next = self.clone();
                if matches!(key, SubagentAttemptKey::Id(_)) {
                    next.attempts.remove(&SubagentAttemptKey::Legacy);
                    next.order
                        .retain(|candidate| candidate != &SubagentAttemptKey::Legacy);
                }
                next.current = Some(key.clone());
                next.attempts.insert(
                    key.clone(),
                    AttemptLifecycle {
                        phase: AttemptPhase::Finished,
                        last_event_seq: max_seq(existing.last_event_seq, event_seq),
                    },
                );
                touch_attempt(&mut next.order, key);
                next.last_spawn_seq = max_seq(next.last_spawn_seq, event_seq);
                return accepted(next, LifecycleEffect::ApplyWithPendingFinish);
            }
            return LifecycleReduction::Dropped;
        }

        let mut next = self.clone();
        let is_new_attempt = next.current.is_some();
        next.current = Some(key.clone());
        let legacy_pending = matches!(key, SubagentAttemptKey::Id(_))
            .then(|| next.attempts.get(&SubagentAttemptKey::Legacy))
            .flatten()
            .filter(|attempt| attempt.phase == AttemptPhase::PendingFinish)
            .cloned();
        if let Some(pending) = legacy_pending {
            next.attempts.remove(&SubagentAttemptKey::Legacy);
            next.order
                .retain(|candidate| candidate != &SubagentAttemptKey::Legacy);
            next.attempts.insert(
                key.clone(),
                AttemptLifecycle {
                    phase: AttemptPhase::Finished,
                    last_event_seq: max_seq(pending.last_event_seq, event_seq),
                },
            );
            touch_attempt(&mut next.order, key);
            next.last_spawn_seq = max_seq(next.last_spawn_seq, event_seq);
            trim_attempts(&mut next);
            return accepted(next, LifecycleEffect::ApplyWithPendingFinish);
        }
        next.attempts.insert(
            key.clone(),
            AttemptLifecycle {
                phase: AttemptPhase::Running,
                last_event_seq: event_seq,
            },
        );
        touch_attempt(&mut next.order, key);
        next.last_spawn_seq = max_seq(next.last_spawn_seq, event_seq);
        trim_attempts(&mut next);
        accepted(
            next,
            if is_new_attempt {
                LifecycleEffect::ApplyNewAttempt
            } else {
                LifecycleEffect::Apply
            },
        )
    }

    fn reduce_progress(
        &self,
        key: &SubagentAttemptKey,
        event_seq: Option<u64>,
    ) -> LifecycleReduction {
        let Some(attempt) = self.attempts.get(key) else {
            return LifecycleReduction::Dropped;
        };
        if self.current.as_ref() != Some(key)
            || attempt.phase != AttemptPhase::Running
            || !is_newer(event_seq, attempt.last_event_seq)
        {
            return LifecycleReduction::Dropped;
        }
        let mut next = self.clone();
        if let Some(attempt) = next.attempts.get_mut(key) {
            attempt.last_event_seq = max_seq(attempt.last_event_seq, event_seq);
        }
        accepted(next, LifecycleEffect::Apply)
    }

    fn reduce_finish(
        &self,
        mut key: SubagentAttemptKey,
        event_seq: Option<u64>,
    ) -> LifecycleReduction {
        if key == SubagentAttemptKey::Legacy
            && let Some(typed_key @ SubagentAttemptKey::Id(_)) = self.current.as_ref()
        {
            let Some(attempt) = self.attempts.get(typed_key) else {
                return LifecycleReduction::Dropped;
            };
            if attempt.phase != AttemptPhase::Running {
                return LifecycleReduction::Dropped;
            }
            key = typed_key.clone();
        }
        if let Some(attempt) = self.attempts.get(&key) {
            if attempt.phase != AttemptPhase::Running
                || !is_newer(event_seq, attempt.last_event_seq)
            {
                return LifecycleReduction::Dropped;
            }
            let mut next = self.clone();
            if let Some(attempt) = next.attempts.get_mut(&key) {
                attempt.phase = AttemptPhase::Finished;
                attempt.last_event_seq = max_seq(attempt.last_event_seq, event_seq);
            }
            return if next.current.as_ref() == Some(&key) {
                accepted(next, LifecycleEffect::Apply)
            } else {
                accepted(next, LifecycleEffect::RecordOnly)
            };
        }
        if event_seq
            .zip(self.last_spawn_seq)
            .is_some_and(|(incoming, high_water)| incoming <= high_water)
        {
            return LifecycleReduction::Dropped;
        }
        let mut next = self.clone();
        next.attempts.insert(
            key.clone(),
            AttemptLifecycle {
                phase: AttemptPhase::PendingFinish,
                last_event_seq: event_seq,
            },
        );
        touch_attempt(&mut next.order, key);
        trim_attempts(&mut next);
        accepted(next, LifecycleEffect::AwaitSpawn)
    }
}

fn accepted(next: LifecycleState, effect: LifecycleEffect) -> LifecycleReduction {
    LifecycleReduction::Accepted(AcceptedLifecycle { next, effect })
}

fn is_newer(incoming: Option<u64>, current: Option<u64>) -> bool {
    incoming
        .zip(current)
        .is_none_or(|(incoming, current)| incoming > current)
}

fn max_seq(current: Option<u64>, incoming: Option<u64>) -> Option<u64> {
    match (current, incoming) {
        (Some(current), Some(incoming)) => Some(current.max(incoming)),
        (current, incoming) => current.or(incoming),
    }
}

fn touch_attempt(order: &mut VecDeque<SubagentAttemptKey>, key: SubagentAttemptKey) {
    if let Some(index) = order.iter().position(|existing| existing == &key) {
        order.remove(index);
    }
    order.push_back(key);
}

fn trim_attempts(state: &mut LifecycleState) {
    while state.attempts.len() > ATTEMPT_HISTORY_LIMIT {
        let Some(index) = state
            .order
            .iter()
            .position(|key| state.current.as_ref() != Some(key))
        else {
            break;
        };
        if let Some(oldest_retired) = state.order.remove(index) {
            state.attempts.remove(&oldest_retired);
        }
    }
}
