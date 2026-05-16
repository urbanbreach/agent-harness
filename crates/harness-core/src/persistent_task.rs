use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::event::{
    EventEnvelopeV1, EventV1, PersistentTask, PersistentTaskStatus, PersistentTaskUpdatedEvent,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentTaskProjection {
    pub tasks: BTreeMap<String, PersistentTask>,
}

pub fn project_persistent_tasks(events: &[EventEnvelopeV1]) -> PersistentTaskProjection {
    let mut projection = PersistentTaskProjection::default();
    for event in events {
        match &event.payload {
            EventV1::PersistentTaskCreated(payload) => {
                if !projection.tasks.contains_key(&payload.task.task_id) {
                    let mut task = payload.task.clone();
                    task.blocks.clear();
                    projection.tasks.insert(task.task_id.clone(), task);
                }
            }
            EventV1::PersistentTaskUpdated(payload) => {
                if let Some(task) = projection.tasks.get_mut(&payload.task_id) {
                    apply_persistent_task_update(task, payload);
                }
            }
            _ => {}
        }
        refresh_persistent_task_blocks(&mut projection);
    }
    projection
}

pub fn apply_persistent_task_update(
    task: &mut PersistentTask,
    update: &PersistentTaskUpdatedEvent,
) {
    task.status = update.status;
    if let Some(subject) = update.subject.as_ref() {
        task.subject = subject.clone();
    }
    if let Some(description) = update.description.as_ref() {
        task.description = description.clone();
    }
    if let Some(active_form) = update.active_form.as_ref() {
        task.active_form = Some(active_form.clone());
    }
    if update.owner.is_some() {
        task.owner = update.owner.clone();
    }
    if let Some(blocked_by) = update.blocked_by.as_ref() {
        task.blocked_by = blocked_by.clone();
        task.blocks.clear();
    }
    if !update.metadata.is_empty() {
        task.metadata.extend(update.metadata.clone());
    }
}

pub fn refresh_persistent_task_blocks(projection: &mut PersistentTaskProjection) {
    for task in projection.tasks.values_mut() {
        task.blocks.clear();
    }
    let edges = projection
        .tasks
        .iter()
        .flat_map(|(task_id, task)| {
            task.blocked_by
                .iter()
                .cloned()
                .map(|blocked_by| (blocked_by, task_id.clone()))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    for (blocked_by, task_id) in edges {
        if let Some(blocker) = projection.tasks.get_mut(&blocked_by) {
            if !blocker.blocks.contains(&task_id) {
                blocker.blocks.push(task_id);
            }
        }
    }
}

pub fn blocked_by_incomplete(
    tasks: &BTreeMap<String, PersistentTask>,
    task_id: &str,
) -> Option<String> {
    let task = tasks.get(task_id)?;
    task.blocked_by.iter().find_map(|blocked_by| {
        tasks
            .get(blocked_by)
            .filter(|candidate| candidate.status != PersistentTaskStatus::Completed)
            .map(|_| blocked_by.clone())
    })
}

pub fn ready_persistent_task_ids(tasks: &BTreeMap<String, PersistentTask>) -> Vec<String> {
    tasks
        .iter()
        .filter(|(_, task)| task.status == PersistentTaskStatus::Pending)
        .filter(|(task_id, _)| blocked_by_incomplete(tasks, task_id).is_none())
        .map(|(task_id, _)| task_id.clone())
        .collect()
}

pub fn has_persistent_task_dependency_path(
    tasks: &BTreeMap<String, PersistentTask>,
    from_task_id: &str,
    to_task_id: &str,
) -> bool {
    let mut stack = vec![from_task_id.to_string()];
    let mut visited = Vec::<String>::new();
    while let Some(task_id) = stack.pop() {
        if task_id == to_task_id {
            return true;
        }
        if visited.contains(&task_id) {
            continue;
        }
        visited.push(task_id.clone());
        if let Some(task) = tasks.get(&task_id) {
            stack.extend(task.blocked_by.iter().cloned());
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{ActorKind, EventActor, PersistentTaskCreatedEvent, SCHEMA_VERSION};

    #[test]
    fn projection_computes_blocks_from_blocked_by() {
        let actor = EventActor::new(ActorKind::System, None);
        let events = vec![
            envelope(
                1,
                EventV1::PersistentTaskCreated(PersistentTaskCreatedEvent {
                    task: task("task_a", vec![]),
                }),
                actor.clone(),
            ),
            envelope(
                2,
                EventV1::PersistentTaskCreated(PersistentTaskCreatedEvent {
                    task: task("task_b", vec!["task_a".to_string()]),
                }),
                actor,
            ),
        ];

        let projection = project_persistent_tasks(&events);
        assert_eq!(projection.tasks["task_a"].blocks, vec!["task_b"]);
        assert_eq!(projection.tasks["task_b"].blocked_by, vec!["task_a"]);
    }

    fn task(task_id: &str, blocked_by: Vec<String>) -> PersistentTask {
        PersistentTask {
            version: 1,
            task_id: task_id.to_string(),
            run_id: Some("run_persistent_tasks".to_string()),
            thread_id: None,
            subject: task_id.to_string(),
            description: "description".to_string(),
            status: PersistentTaskStatus::Pending,
            active_form: None,
            owner: None,
            blocks: Vec::new(),
            blocked_by,
            metadata: BTreeMap::new(),
        }
    }

    fn envelope(seq: u64, payload: EventV1, actor: EventActor) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt_{seq}"),
            seq,
            run_id: "run_persistent_tasks".to_string(),
            mono_ms: seq,
            ts: None,
            actor,
            correlation_id: None,
            causation_id: None,
            stream_key: None,
            payload,
        }
    }
}
