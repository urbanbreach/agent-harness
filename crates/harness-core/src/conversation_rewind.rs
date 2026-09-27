use crate::event::{ActorKind, EventEnvelopeV1, EventV1};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationRewoundEvent {
    pub target_seq: u64,
    pub request_id: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RewindPoint {
    pub seq: u64,
    pub request_id: String,
    pub text: String,
}

pub fn active_events(events: &[EventEnvelopeV1]) -> Cow<'_, [EventEnvelopeV1]> {
    if !events
        .iter()
        .any(|e| matches!(e.payload, EventV1::ConversationRewound(_)))
    {
        return Cow::Borrowed(events);
    }
    Cow::Owned(active_refs(events).into_iter().cloned().collect())
}

pub(crate) fn active_refs<'a>(
    events: impl IntoIterator<Item = &'a EventEnvelopeV1>,
) -> Vec<&'a EventEnvelopeV1> {
    let mut active: Vec<&EventEnvelopeV1> = Vec::new();
    for event in events {
        if let EventV1::ConversationRewound(rewind) = &event.payload {
            let boundary = active.partition_point(|e| e.seq < rewind.target_seq);
            active.truncate(boundary);
        }
        active.push(event);
    }
    active
}

pub fn rewind_points(events: &[EventEnvelopeV1]) -> Vec<RewindPoint> {
    active_events(events)
        .iter()
        .filter_map(|event| {
            let EventV1::UserMessageSubmitted(message) = &event.payload else {
                return None;
            };
            matches!(event.actor.kind, ActorKind::User | ActorKind::Supervisor).then(|| {
                RewindPoint {
                    seq: event.seq,
                    request_id: message.request_id.to_string(),
                    text: message.text.clone(),
                }
            })
        })
        .collect()
}
