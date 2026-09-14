use std::collections::BTreeSet;

use harness_core::event::{
    EventEnvelopeV1, EventV1, LiveEventEnvelope, TaskLineageMetadata, ToolCallFinishedEvent,
};

use super::child_session::{event_agent_id, session_id_from_path};
use super::{task_child_request_id_from_output, task_child_session_id_from_output, AppState};
use crate::text::non_empty_trimmed;

impl AppState {
    pub(in crate::app) fn route_live_fragment_while_viewing_child(
        &self,
        event: &LiveEventEnvelope,
    ) -> bool {
        if !self.replay_mode {
            return false;
        }
        let Some(current_session_id) = self.current_session_id() else {
            return false;
        };
        let Some(parent_snapshot) = self.session_navigation_stack.first() else {
            return false;
        };
        if parent_snapshot.replay_mode {
            return false;
        }

        let visible = event_agent_id(&event.actor, event.stream_key.as_deref())
            == Some(current_session_id)
            || event.correlation_id.as_deref().is_some_and(|request_id| {
                child_request_ids_for_session(&parent_snapshot.events, current_session_id)
                    .contains(request_id)
            });
        !visible
    }

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
        let Some((parent_snapshot, child_snapshots)) =
            self.session_navigation_stack.split_first_mut()
        else {
            return false;
        };
        if parent_snapshot.replay_mode {
            return false;
        }

        let visible_in_current_child = event_belongs_to_child_session(
            event,
            &child_request_ids_for_session(&parent_snapshot.events, &current_session_id),
            &current_session_id,
        );
        if !parent_snapshot
            .events
            .iter()
            .any(|existing| existing.seq == event.seq)
        {
            let child_session_id = child_session_id_from_event(event);
            let belongs_to_child = child_session_id.is_some()
                && parent_snapshot.child_session_ids.iter().any(|id| {
                    event_belongs_to_child_session(
                        event,
                        &child_request_ids_for_session(&parent_snapshot.events, id),
                        id,
                    )
                });
            parent_snapshot.events.push(event.clone());
            if !belongs_to_child {
                push_child_session_id(&mut parent_snapshot.child_session_ids, child_session_id);
            }
        }
        for snapshot in child_snapshots {
            let Some(session_id) = session_id_from_path(&snapshot.session_path) else {
                continue;
            };
            if event_belongs_to_child_session(
                event,
                &child_request_ids_for_session(&parent_snapshot.events, &session_id),
                &session_id,
            ) && !snapshot
                .events
                .iter()
                .any(|existing| existing.seq == event.seq)
            {
                snapshot.events.push(event.clone());
                push_child_session_id(
                    &mut snapshot.child_session_ids,
                    child_session_id_from_event(event).filter(|id| id != &session_id),
                );
            }
        }

        !visible_in_current_child
    }
}

pub(super) fn event_belongs_to_child_session(
    event: &EventEnvelopeV1,
    child_request_ids: &BTreeSet<String>,
    child_session_id: &str,
) -> bool {
    if event_agent_id(&event.actor, event.stream_key.as_deref()) == Some(child_session_id) {
        return true;
    }

    if matches!(&event.payload, EventV1::AgentSpawned(data) if data.agent_id == child_session_id) {
        return true;
    }

    event
        .correlation_id
        .as_deref()
        .is_some_and(|request_id| child_request_ids.contains(request_id))
}

pub(super) fn child_request_ids_for_session(
    events: &[EventEnvelopeV1],
    child_session_id: &str,
) -> BTreeSet<String> {
    events
        .iter()
        .filter_map(|event| child_request_id_from_event(event, child_session_id))
        .collect()
}

fn child_request_id_from_event(event: &EventEnvelopeV1, child_session_id: &str) -> Option<String> {
    if event_agent_id(&event.actor, event.stream_key.as_deref()) == Some(child_session_id) {
        let request_id = match &event.payload {
            EventV1::UserMessageSubmitted(data) => Some(data.request_id.as_str()),
            EventV1::ProviderRequestStarted(data) => event
                .correlation_id
                .as_deref()
                .or(Some(data.request_id.as_str())),
            EventV1::ProviderRequestFinished(data) => event
                .correlation_id
                .as_deref()
                .or(Some(data.request_id.as_str())),
            EventV1::TaskScheduled(_) => event.correlation_id.as_deref(),
            _ => None,
        };
        if let Some(request_id) = request_id {
            return Some(request_id.to_string());
        }
    }
    match &event.payload {
        EventV1::TaskScheduled(data) => data
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(|lineage| lineage_child_request_id(lineage, child_session_id)),
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
