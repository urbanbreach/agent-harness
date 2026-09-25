//! Conversation rewind is a projection change. The source journal and workspace stay intact.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::event::{EventEnvelopeV1, EventV1};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationRewoundEvent {
    pub target_seq: u64,
    pub request_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewindPoint {
    pub seq: u64,
    pub request_id: String,
    pub text: String,
}

pub fn excluded_ranges(events: &[EventEnvelopeV1]) -> Vec<Range<u64>> {
    events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::ConversationRewound(rewind) => Some(rewind.target_seq..event.seq),
            _ => None,
        })
        .collect()
}

pub fn is_excluded(ranges: &[Range<u64>], seq: u64) -> bool {
    ranges.iter().any(|range| range.contains(&seq))
}

pub fn rewind_points(events: &[EventEnvelopeV1]) -> Vec<RewindPoint> {
    let ranges = excluded_ranges(events);
    events
        .iter()
        .filter(|event| !is_excluded(&ranges, event.seq))
        .filter_map(|event| match &event.payload {
            EventV1::UserMessageSubmitted(prompt)
                if matches!(
                    event.actor.kind,
                    crate::event::ActorKind::User | crate::event::ActorKind::Supervisor
                ) =>
            {
                Some(RewindPoint {
                    seq: event.seq,
                    request_id: prompt.request_id.to_string(),
                    text: prompt.text.clone(),
                })
            }
            _ => None,
        })
        .collect()
}

/// Select the visible conversation without renumbering or changing stored events.
pub fn active_events(events: &[EventEnvelopeV1]) -> std::borrow::Cow<'_, [EventEnvelopeV1]> {
    let ranges = excluded_ranges(events);
    if ranges.is_empty() {
        return std::borrow::Cow::Borrowed(events);
    }
    std::borrow::Cow::Owned(
        events
            .iter()
            .filter(|event| !is_excluded(&ranges, event.seq))
            .cloned()
            .collect(),
    )
}
