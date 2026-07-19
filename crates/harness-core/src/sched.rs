use crate::UnwrapOrAbort;
use std::collections::{BTreeMap, VecDeque};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConcurrencyKey {
    ProviderModel {
        provider_id: String,
        model_id: String,
    },
    NestedProviderModel {
        provider_id: String,
        model_id: String,
        parent_agent_id: String,
        agent_id: String,
    },
    Tool {
        tool_id: String,
    },
}

impl ConcurrencyKey {
    pub fn queue_key(&self) -> String {
        match self {
            Self::ProviderModel {
                provider_id,
                model_id,
            }
            | Self::NestedProviderModel {
                provider_id,
                model_id,
                ..
            } => {
                format!("provider_model:{provider_id}:{model_id}")
            }
            Self::Tool { tool_id } => format!("tool:{tool_id}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSpec {
    pub task_id: crate::ids::TaskId,
    pub key: ConcurrencyKey,
}

impl TaskSpec {
    pub fn new(task_id: impl Into<crate::ids::TaskId>, key: ConcurrencyKey) -> Self {
        Self {
            task_id: task_id.into(),
            key,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotGate {
    pub limit: usize,
    pub in_flight: usize,
    pub queue: VecDeque<TaskSpec>,
}

impl SlotGate {
    fn new(limit: usize) -> Self {
        Self {
            limit: limit.max(1),
            in_flight: 0,
            queue: VecDeque::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerLimits {
    pub provider_model: usize,
    pub tool: usize,
}

impl Default for SchedulerLimits {
    fn default() -> Self {
        Self {
            provider_model: 1,
            tool: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleDecision {
    Started(TaskSpec),
    Queued(TaskSpec),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskProgressSnapshot {
    pub task_id: crate::ids::TaskId,
    pub key: ConcurrencyKey,
    pub last_progress_mono_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleTask {
    pub task_id: crate::ids::TaskId,
    pub stale_for_ms: u64,
}

#[derive(Debug)]
pub struct Scheduler {
    limits: SchedulerLimits,
    gates: BTreeMap<ConcurrencyKey, SlotGate>,
}

impl Scheduler {
    pub fn new(limits: SchedulerLimits) -> Self {
        Self {
            limits,
            gates: BTreeMap::new(),
        }
    }

    pub fn schedule(
        &mut self,
        task_id: impl Into<crate::ids::TaskId>,
        key: ConcurrencyKey,
    ) -> ScheduleDecision {
        let task = TaskSpec::new(task_id, key.clone());
        let gate_limit = self.limit_for(&key);
        let gate = self
            .gates
            .entry(key)
            .or_insert_with(|| SlotGate::new(gate_limit));

        if gate.in_flight < gate.limit {
            gate.in_flight += 1;
            return ScheduleDecision::Started(task);
        }

        gate.queue.push_back(task.clone());
        ScheduleDecision::Queued(task)
    }

    pub fn complete(&mut self, key: &ConcurrencyKey) -> Vec<TaskSpec> {
        let mut dequeued = Vec::new();

        if let Some(gate) = self.gates.get_mut(key) {
            gate.in_flight = gate.in_flight.saturating_sub(1);

            while gate.in_flight < gate.limit {
                let Some(task) = gate.queue.pop_front() else {
                    break;
                };

                gate.in_flight += 1;
                dequeued.push(task);
            }
        }

        self.compact_gate(key);
        dequeued
    }

    pub fn cancel_queued(&mut self, task_id: &str) -> Option<TaskSpec> {
        let mut removed: Option<TaskSpec> = None;
        let mut compact_key: Option<ConcurrencyKey> = None;

        for (key, gate) in self.gates.iter_mut() {
            let Some(index) = gate
                .queue
                .iter()
                .position(|task| task.task_id.as_str() == task_id)
            else {
                continue;
            };

            removed = gate.queue.remove(index);
            compact_key = Some(key.clone());
            break;
        }

        if let Some(key) = compact_key {
            self.compact_gate(&key);
        }

        removed
    }

    pub fn detect_stale(
        &self,
        now_mono_ms: u64,
        stale_timeout_ms: u64,
        running_tasks: &[TaskProgressSnapshot],
    ) -> Vec<StaleTask> {
        running_tasks
            .iter()
            .filter_map(|task| {
                let gate = self.gates.get(&task.key)?;
                if gate.in_flight == 0 {
                    return None;
                }

                let stale_for_ms = now_mono_ms.saturating_sub(task.last_progress_mono_ms);
                if stale_for_ms <= stale_timeout_ms {
                    return None;
                }

                Some(StaleTask {
                    task_id: task.task_id.clone(),
                    stale_for_ms,
                })
            })
            .collect()
    }

    fn limit_for(&self, key: &ConcurrencyKey) -> usize {
        match key {
            ConcurrencyKey::ProviderModel { .. } | ConcurrencyKey::NestedProviderModel { .. } => {
                self.limits.provider_model.max(1)
            }
            ConcurrencyKey::Tool { .. } => self.limits.tool.max(1),
        }
    }

    fn compact_gate(&mut self, key: &ConcurrencyKey) {
        let Some(gate) = self.gates.get(key) else {
            return;
        };

        if gate.in_flight == 0 && gate.queue.is_empty() {
            self.gates.remove(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ConcurrencyKey, ScheduleDecision, Scheduler, SchedulerLimits, TaskProgressSnapshot,
    };
    use crate::UnwrapOrAbort;

    #[test]
    fn scheduler_limit_one_queues_then_dequeues_after_completion() {
        // arrange
        // act
        // assert
        let mut scheduler = Scheduler::new(SchedulerLimits {
            provider_model: 1,
            tool: 1,
        });

        let key = ConcurrencyKey::Tool {
            tool_id: "shell.run".to_string(),
        };

        let first = scheduler.schedule("task_1", key.clone());
        let second = scheduler.schedule("task_2", key.clone());

        assert!(matches!(
            first,
            ScheduleDecision::Started(task) if task.task_id.as_str() == "task_1"
        ));
        assert!(matches!(
            second,
            ScheduleDecision::Queued(task) if task.task_id.as_str() == "task_2"
        ));

        let dequeued = scheduler.complete(&key);
        assert_eq!(dequeued.len(), 1);
        assert_eq!(dequeued[0].task_id.as_str(), "task_2");
    }

    #[test]
    fn scheduler_can_cancel_queued_task() {
        // arrange
        // act
        // assert
        let mut scheduler = Scheduler::new(SchedulerLimits {
            provider_model: 1,
            tool: 1,
        });

        let key = ConcurrencyKey::Tool {
            tool_id: "shell.run".to_string(),
        };

        let _ = scheduler.schedule("task_1", key.clone());
        let _ = scheduler.schedule("task_2", key);

        let cancelled = scheduler.cancel_queued("task_2").unwrap_or_abort();
        assert_eq!(cancelled.task_id.as_str(), "task_2");
    }

    #[test]
    fn scheduler_detects_stale_running_tasks() {
        // arrange
        // act
        // assert
        let mut scheduler = Scheduler::new(SchedulerLimits {
            provider_model: 1,
            tool: 1,
        });

        let key = ConcurrencyKey::Tool {
            tool_id: "shell.run".to_string(),
        };

        let _ = scheduler.schedule("task_1", key.clone());

        let stale = scheduler.detect_stale(
            1_500,
            1_000,
            &[TaskProgressSnapshot {
                task_id: "task_1".to_string().into(),
                key,
                last_progress_mono_ms: 0,
            }],
        );

        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].task_id.as_str(), "task_1");
        assert_eq!(stale[0].stale_for_ms, 1_500);
    }

    #[test]
    fn scheduler_limit_two_starts_two_and_dequeues_fifo_when_saturated() {
        // arrange
        // act
        // assert
        let mut scheduler = Scheduler::new(SchedulerLimits {
            provider_model: 2,
            tool: 1,
        });

        let key = ConcurrencyKey::ProviderModel {
            provider_id: "mock".to_string(),
            model_id: "model-1".to_string(),
        };

        let first = scheduler.schedule("task_1", key.clone());
        let second = scheduler.schedule("task_2", key.clone());
        let third = scheduler.schedule("task_3", key.clone());

        assert!(matches!(
            first,
            ScheduleDecision::Started(task) if task.task_id.as_str() == "task_1"
        ));
        assert!(matches!(
            second,
            ScheduleDecision::Started(task) if task.task_id.as_str() == "task_2"
        ));
        assert!(matches!(
            third,
            ScheduleDecision::Queued(task) if task.task_id.as_str() == "task_3"
        ));

        let dequeued = scheduler.complete(&key);
        assert_eq!(dequeued.len(), 1);
        assert_eq!(dequeued[0].task_id.as_str(), "task_3");
    }
}
