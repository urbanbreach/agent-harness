use std::collections::{BTreeMap, BTreeSet};

use crate::event::{ActorKind, EventEnvelopeV1, EventV1};

use super::super::super::COORDINATOR_AGENT_ID;

struct ToolRequestIdentity {
    agent_id: String,
    correlation_id: String,
    request_event_id: String,
    request_causation_id: Option<String>,
    assistant_event_id: Option<String>,
}

pub(super) fn admitted_tool_lifecycle_event_ids(
    events: &[EventEnvelopeV1],
    agent_id: &str,
) -> BTreeSet<String> {
    let mut latest_assistant_by_turn = BTreeMap::<(String, String), String>::new();
    let mut requests_by_call_id = BTreeMap::<String, Vec<ToolRequestIdentity>>::new();
    for event in events {
        let Some(owner) = event.actor.agent_id.as_deref() else {
            continue;
        };
        let Some(correlation_id) = event.correlation_id.as_deref() else {
            continue;
        };
        match &event.payload {
            EventV1::AssistantMessageFinished(_) => {
                latest_assistant_by_turn.insert(
                    (owner.to_string(), correlation_id.to_string()),
                    event.event_id.clone(),
                );
            }
            EventV1::ToolCallRequested(requested) => {
                requests_by_call_id
                    .entry(requested.tool_call_id.to_string())
                    .or_default()
                    .push(ToolRequestIdentity {
                        agent_id: owner.to_string(),
                        correlation_id: correlation_id.to_string(),
                        request_event_id: event.event_id.clone(),
                        request_causation_id: event.causation_id.clone(),
                        assistant_event_id: latest_assistant_by_turn
                            .get(&(owner.to_string(), correlation_id.to_string()))
                            .cloned(),
                    });
            }
            _ => {}
        }
    }

    admit_lifecycle_events(events, agent_id, &requests_by_call_id)
}

fn admit_lifecycle_events(
    events: &[EventEnvelopeV1],
    agent_id: &str,
    requests_by_call_id: &BTreeMap<String, Vec<ToolRequestIdentity>>,
) -> BTreeSet<String> {
    let mut admitted = BTreeSet::new();
    let mut started_by_pair = BTreeMap::<(String, String), String>::new();
    let mut ambiguous_starts = BTreeSet::<(String, String)>::new();
    for event in events {
        if !is_coordinator_tool_lifecycle(event) {
            continue;
        }
        let (tool_call_id, terminal_status) = match &event.payload {
            EventV1::ToolCallStarted(started) => (started.tool_call_id.to_string(), None),
            EventV1::ToolCallFinished(finished) => {
                (finished.tool_call_id.to_string(), Some(finished.status))
            }
            _ => continue,
        };
        let Some([identity]) = requests_by_call_id
            .get(tool_call_id.as_str())
            .map(Vec::as_slice)
        else {
            continue;
        };
        let Some(correlation_id) = event.correlation_id.as_deref() else {
            continue;
        };
        if identity.agent_id != agent_id
            || identity.correlation_id != correlation_id
            || identity.assistant_event_id.is_none()
            || identity
                .request_causation_id
                .as_deref()
                .is_some_and(|causation| Some(causation) != identity.assistant_event_id.as_deref())
        {
            continue;
        }
        let pair = (tool_call_id, correlation_id.to_string());
        if terminal_status.is_none() {
            if event
                .causation_id
                .as_deref()
                .is_some_and(|causation| causation != identity.request_event_id)
            {
                continue;
            }
            if let Some(previous_start) =
                started_by_pair.insert(pair.clone(), event.event_id.clone())
            {
                ambiguous_starts.insert(pair);
                admitted.remove(previous_start.as_str());
            } else {
                admitted.insert(event.event_id.clone());
            }
            continue;
        }
        if ambiguous_starts.contains(&pair) {
            continue;
        }
        let Some(start_event_id) = started_by_pair.get(&pair) else {
            if terminal_status == Some(crate::event::ToolCallStatus::Failed)
                && event.causation_id.as_deref() == Some(identity.request_event_id.as_str())
            {
                admitted.insert(event.event_id.clone());
            }
            continue;
        };
        if event
            .causation_id
            .as_deref()
            .is_some_and(|causation| causation != start_event_id)
        {
            continue;
        }
        admitted.insert(event.event_id.clone());
    }
    admitted
}

fn is_coordinator_tool_lifecycle(event: &EventEnvelopeV1) -> bool {
    event.actor.kind == ActorKind::System
        && event.actor.agent_id.as_deref() == Some(COORDINATOR_AGENT_ID)
        && matches!(
            event.payload,
            EventV1::ToolCallStarted(_) | EventV1::ToolCallFinished(_)
        )
}
