use std::collections::BTreeSet;

use harness_core::event::{EventEnvelopeV1, EventV1, TaskLineageMetadata, ToolCallFinishedEvent};

use super::{task_child_request_id_from_output, task_child_session_id_from_output, AppState};
use crate::text::non_empty_trimmed;

impl AppState {
    pub(in crate::app) fn route_live_event_while_viewing_child(
        &mut self,
        event: &EventEnvelopeV1,
    ) -> bool {
        if !self.replay_mode {
            return false;
        }

        let Some(current_session_id) = self.current_session_id().map(str::to_string) else {
            return false;
        };
        let Some(parent_snapshot) = self.session_navigation_stack.last_mut() else {
            return false;
        };
        if parent_snapshot.replay_mode {
            return false;
        }

        let visible_in_current_child =
            event_belongs_to_child_session(event, &parent_snapshot.events, &current_session_id);
        if !parent_snapshot
            .events
            .iter()
            .any(|existing| existing.seq == event.seq)
        {
            parent_snapshot.events.push(event.clone());
            push_child_session_id(
                &mut parent_snapshot.child_session_ids,
                child_session_id_from_event(event),
            );
        }

        !visible_in_current_child
    }
}

fn event_belongs_to_child_session(
    event: &EventEnvelopeV1,
    parent_events: &[EventEnvelopeV1],
    child_session_id: &str,
) -> bool {
    if event.actor.agent_id.as_deref() == Some(child_session_id) {
        return true;
    }

    if matches!(&event.payload, EventV1::AgentSpawned(data) if data.agent_id == child_session_id) {
        return true;
    }

    let child_request_ids = child_request_ids_for_session(parent_events, child_session_id);
    event
        .correlation_id
        .as_deref()
        .is_some_and(|request_id| child_request_ids.contains(request_id))
}

fn child_request_ids_for_session(
    events: &[EventEnvelopeV1],
    child_session_id: &str,
) -> BTreeSet<String> {
    events
        .iter()
        .filter_map(|event| child_request_id_from_event(event, child_session_id))
        .collect()
}

fn child_request_id_from_event(event: &EventEnvelopeV1, child_session_id: &str) -> Option<String> {
    match &event.payload {
        EventV1::ToolCallRequested(data) => data
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(|lineage| lineage_child_request_id(lineage, child_session_id)),
        EventV1::ToolCallFinished(data) => tool_finished_child_request_id(data, child_session_id),
        EventV1::TaskCompleted(data) => data
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(|lineage| lineage_child_request_id(lineage, child_session_id)),
        EventV1::BackgroundTaskNotification(data)
            if non_empty_trimmed(data.child_session_id.as_str()) == Some(child_session_id) =>
        {
            non_empty_trimmed(&data.child_request_id).map(str::to_string)
        }
        _ => None,
    }
}

fn tool_finished_child_request_id(
    data: &ToolCallFinishedEvent,
    child_session_id: &str,
) -> Option<String> {
    data.metadata
        .as_ref()
        .and_then(|metadata| metadata.lineage.as_ref())
        .and_then(|lineage| lineage_child_request_id(lineage, child_session_id))
        .or_else(|| {
            let output_json = data.output_json.as_ref();
            let output_child_session_id = task_child_session_id_from_output(output_json)?;
            (output_child_session_id == child_session_id)
                .then(|| task_child_request_id_from_output(output_json))
                .flatten()
        })
}

fn lineage_child_request_id(
    lineage: &TaskLineageMetadata,
    child_session_id: &str,
) -> Option<String> {
    let lineage_child_session_id = lineage
        .child_session_id
        .as_deref()
        .and_then(non_empty_trimmed)?;
    if lineage_child_session_id != child_session_id {
        return None;
    }

    lineage
        .child_request_id
        .as_deref()
        .and_then(non_empty_trimmed)
        .map(str::to_string)
}

fn child_session_id_from_event(event: &EventEnvelopeV1) -> Option<String> {
    match &event.payload {
        EventV1::ToolCallRequested(data) => data
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(lineage_child_session_id),
        EventV1::ToolCallFinished(data) => data
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(lineage_child_session_id)
            .or_else(|| task_child_session_id_from_output(data.output_json.as_ref())),
        EventV1::TaskCompleted(data) => data
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(lineage_child_session_id),
        EventV1::BackgroundTaskNotification(data) => {
            non_empty_trimmed(data.child_session_id.as_str()).map(str::to_string)
        }
        _ => None,
    }
}

fn lineage_child_session_id(lineage: &TaskLineageMetadata) -> Option<String> {
    lineage
        .child_session_id
        .as_deref()
        .and_then(non_empty_trimmed)
        .map(str::to_string)
}

fn push_child_session_id(child_session_ids: &mut Vec<String>, child_session_id: Option<String>) {
    let Some(child_session_id) = child_session_id else {
        return;
    };
    if !child_session_ids.contains(&child_session_id) {
        child_session_ids.push(child_session_id);
    }
}
