use super::*;
use crate::event::{EventEnvelopeV1, EventV1, TaskScheduleState, TaskTerminalScope};
use std::collections::{BTreeMap, VecDeque};

const DEFERRED_LIMIT: usize = 256;
const DEFERRED_TTL_MS: u64 = 60_000;

#[derive(Debug, Clone, Default)]
pub struct SubagentHistory {
    pub records: BTreeMap<String, SubagentHistoryRecord>,
    pub finalized: BTreeMap<String, FinalizedAgentStateReferenceV1>,
    deferred: VecDeque<(u64, SubagentTransitionV1)>,
}

#[derive(Debug, Clone, Default)]
pub struct SubagentHistoryRecord {
    pub lifecycle: LifecycleState,
    pub metadata: Option<SubagentTransitionMetadataV1>,
    pub generation: u64,
    pub outcome: Option<SubagentTerminalOutcome>,
    pub accounting: Option<SubagentTerminalAccounting>,
    pub retired: bool,
    pub legacy: bool,
}

impl SubagentHistory {
    /// Supplied monotonic time controls bounded deferred backing, including replay.
    pub fn expire(&mut self, now_ms: u64) {
        self.deferred
            .retain(|(at, _)| now_ms.saturating_sub(*at) < DEFERRED_TTL_MS);
        for (id, record) in &mut self.records {
            record.lifecycle.forget_unbacked_pending_attempts(|key| {
                self.deferred.iter().any(|(_, event)| {
                    &event.child_id.0 == id
                        && SubagentAttemptKey::from_wire(event.attempt_id.as_deref()) == *key
                })
            });
        }
        self.records.retain(|id, record| {
            record.legacy
                || record.metadata.is_some()
                || record.lifecycle.has_current_attempt()
                || self
                    .deferred
                    .iter()
                    .any(|(_, pending)| &pending.child_id.0 == id)
        });
    }

    pub fn deferred_count(&self) -> usize {
        self.deferred.len()
    }

    /// Compute on a clone before append; live authority applies it only after append.
    pub fn apply_transition(&mut self, event: &SubagentTransitionV1, now_ms: u64) -> bool {
        if event.payload_version != 1 || event.child_id.0.is_empty() || event.generation == 0 {
            return false;
        }
        self.expire(now_ms);
        let record = self.records.entry(event.child_id.0.clone()).or_default();
        if (record.retired && event.generation <= record.generation)
            || event.generation < record.generation
            || record
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.ancestry != event.metadata.ancestry)
        {
            return false;
        }
        if event.transition == SubagentTransitionKind::Retired {
            if !record.lifecycle.is_finished() || event.generation != record.generation {
                return false;
            }
            record.retired = true;
            return true;
        }
        if event.transition == SubagentTransitionKind::Routed {
            if event.generation != record.generation
                || record.lifecycle.current_attempt_id() != event.attempt_id.as_deref()
                || record.metadata.as_ref() == Some(&event.metadata)
            {
                return false;
            }
            record.metadata = Some(event.metadata.clone());
            return true;
        }
        let matching_pending = self.deferred.iter().position(|(_, pending)| {
            pending.child_id == event.child_id
                && pending.generation == event.generation
                && (pending.attempt_id == event.attempt_id || pending.attempt_id.is_none())
        });
        if event.transition == SubagentTransitionKind::Spawned {
            // A pending native key is not authority for a different admitted generation.
            record.lifecycle.forget_unbacked_pending_attempts(|key| {
                self.deferred.iter().any(|(_, pending)| {
                    pending.child_id == event.child_id
                        && pending.generation == event.generation
                        && SubagentAttemptKey::from_wire(pending.attempt_id.as_deref()) == *key
                })
            });
        } else if event.transition == SubagentTransitionKind::Finished
            && record.lifecycle.has_current_attempt()
            && event.generation > record.generation
        {
            // In particular, an attempt-less future finish must not alias the current attempt.
            if matching_pending.is_some() {
                return false;
            }
            self.deferred.push_back((now_ms, event.clone()));
            if self.deferred.len() > DEFERRED_LIMIT {
                self.deferred.pop_front();
            }
            self.expire(now_ms);
            return true;
        }
        let transition = match event.transition {
            SubagentTransitionKind::Spawned => LifecycleTransition::Spawned,
            SubagentTransitionKind::Progress => LifecycleTransition::Progress,
            SubagentTransitionKind::Finished => LifecycleTransition::Finished,
            SubagentTransitionKind::Retired | SubagentTransitionKind::Routed => return false,
        };
        let reduction = record.lifecycle.reduce(
            transition,
            event.attempt_id.as_deref(),
            event.notification_seq,
        );
        let Some(effect) = reduction.commit(&mut record.lifecycle) else {
            return false;
        };
        if effect == LifecycleEffect::AwaitSpawn {
            self.deferred.push_back((now_ms, event.clone()));
            if self.deferred.len() > DEFERRED_LIMIT {
                self.deferred.pop_front();
            }
            self.expire(now_ms);
            return true;
        }
        if effect == LifecycleEffect::RecordOnly {
            return true;
        }
        record.metadata = Some(event.metadata.clone());
        record.generation = event.generation;
        record.legacy = false;
        if effect == LifecycleEffect::ApplyNewAttempt {
            record.retired = false;
            record.outcome = None;
            record.accounting = None;
        }
        let terminal =
            if event.transition == SubagentTransitionKind::Spawned && matching_pending.is_some() {
                if !record.lifecycle.is_finished() {
                    // Backing survived an earlier generation's admission. Apply the same native
                    // pending semantics without treating arrival/journal sequence as notification order.
                    record
                        .lifecycle
                        .reduce(
                            LifecycleTransition::Finished,
                            event.attempt_id.as_deref(),
                            None,
                        )
                        .commit(&mut record.lifecycle);
                }
                matching_pending
                    .and_then(|index| self.deferred.remove(index))
                    .map(|(_, pending)| pending)
            } else if event.transition == SubagentTransitionKind::Finished {
                Some(event.clone())
            } else {
                None
            };
        if let Some(terminal) = terminal {
            // First terminal accounting is sealed; later reconciliation cannot invent usage.
            record.outcome = record.outcome.or(terminal.outcome);
            record.accounting = record.accounting.or(terminal.accounting);
            if let Some(reference) = terminal.finalized_state
                && reference.state.owner == event.child_id
                && reference.attempt_id == event.attempt_id.as_deref().unwrap_or("")
                && reference.generation == event.generation
            {
                self.finalized.insert(event.child_id.0.clone(), reference);
            }
        }
        true
    }

    /// Additive V1 decode, with honest summary-only legacy lifecycle and no work.
    pub fn apply(&mut self, event: &EventEnvelopeV1) {
        self.expire(event.mono_ms);
        match &event.payload {
            EventV1::SubagentTransition(transition) => {
                self.apply_transition(transition, event.mono_ms);
            }
            EventV1::FinalizedAgentState(reference)
                if reference.payload_version == 1
                    && reference.owner_run_id == event.run_id.as_str() =>
            {
                self.finalized
                    .insert(reference.state.owner.0.clone(), reference.clone());
            }
            EventV1::AgentSpawned(spawned) if spawned.parent_agent_id.is_some() => {
                self.records
                    .entry(spawned.agent_id.clone())
                    .or_default()
                    .legacy = true;
            }
            EventV1::TaskScheduled(task) if task.state == TaskScheduleState::Started => {
                if let Some(record) = event
                    .actor
                    .agent_id
                    .as_ref()
                    .and_then(|id| self.records.get_mut(id))
                    .filter(|r| r.legacy)
                {
                    record
                        .lifecycle
                        .reduce(LifecycleTransition::Spawned, None, Some(event.seq))
                        .commit(&mut record.lifecycle);
                }
            }
            EventV1::TaskCompleted(task)
                if task.metadata.as_ref().and_then(|m| m.task_scope)
                    == Some(TaskTerminalScope::AgentTurn) =>
            {
                self.legacy_finish(event, SubagentTerminalOutcome::Completed);
            }
            EventV1::TaskCancelled(task)
                if task.task_scope == Some(TaskTerminalScope::AgentTurn) =>
            {
                self.legacy_finish(event, SubagentTerminalOutcome::Cancelled);
            }
            _ => {}
        }
    }

    fn legacy_finish(&mut self, event: &EventEnvelopeV1, outcome: SubagentTerminalOutcome) {
        if let Some(record) = event
            .actor
            .agent_id
            .as_ref()
            .and_then(|id| self.records.get_mut(id))
            .filter(|r| r.legacy)
        {
            record
                .lifecycle
                .reduce(LifecycleTransition::Finished, None, Some(event.seq))
                .commit(&mut record.lifecycle);
            record.outcome = Some(outcome);
        }
    }

    pub fn from_events(events: &[EventEnvelopeV1]) -> Self {
        let mut history = Self::default();
        for event in events {
            history.apply(event);
        }
        history
    }
}
