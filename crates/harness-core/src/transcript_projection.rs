//! A single pass over settled events, shared by replay and incremental UI updates.
use crate::{attachment_transport::AttachmentMetadata, event::*};
use std::collections::HashMap;
mod compaction;
mod messages;
mod model;
mod operations;
mod parts;
pub use model::*;
pub use parts::*;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("events must be strictly increasing by seq: previous={previous_seq}, current={seq}")]
pub struct TranscriptProjectionError {
    pub previous_seq: u64,
    pub seq: u64,
}

pub fn project_transcript(
    events: &[EventEnvelopeV1],
) -> Result<TranscriptProjection, TranscriptProjectionError> {
    for pair in events.windows(2) {
        if pair[0].seq >= pair[1].seq {
            return Err(TranscriptProjectionError {
                previous_seq: pair[0].seq,
                seq: pair[1].seq,
            });
        }
    }
    let mut output = TranscriptProjection::default();
    let mut index = TranscriptIndex::default();
    for event in crate::conversation_rewind::active_events(events).iter() {
        apply(&mut output, &mut index, event);
    }
    Ok(output)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TranscriptIndex {
    users: HashMap<String, usize>,
    providers: HashMap<String, usize>,
    turns: HashMap<String, usize>,
    tools: HashMap<String, (usize, usize)>,
    permissions: HashMap<String, (usize, usize, Option<usize>)>,
}

pub(crate) fn apply(
    output: &mut TranscriptProjection,
    index: &mut TranscriptIndex,
    event: &EventEnvelopeV1,
) {
    output
        .session
        .run_id
        .get_or_insert_with(|| event.run_id.to_string());
    output.session.max_seq = Some(event.seq);
    if !messages::apply(output, index, event) && !operations::apply(output, index, event) {
        compaction::apply(output, index, event);
    }
}

impl ProvenanceRange {
    fn at(event: &EventEnvelopeV1) -> Self {
        Self {
            first_seq: event.seq,
            last_seq: event.seq,
            event_ids: vec![event.event_id.clone()],
        }
    }
    fn extend(&mut self, event: &EventEnvelopeV1) {
        self.last_seq = event.seq;
        if self.event_ids.last() != Some(&event.event_id) {
            self.event_ids.push(event.event_id.clone());
        }
    }
}

fn push_message(
    output: &mut TranscriptProjection,
    event: &EventEnvelopeV1,
    role: ProjectedMessageRole,
    request: Option<&str>,
) -> usize {
    let index = output.messages.len();
    output.messages.push(ProjectedMessage {
        message_id: event.event_id.clone(),
        role,
        state: ProjectedMessageState::Complete,
        request_id: request.map(Into::into),
        agent_id: event.actor.agent_id.clone(),
        provider: None,
        provenance: ProvenanceRange::at(event),
        parts: Vec::new(),
        attachments: Vec::new(),
    });
    index
}

fn part_message(
    output: &mut TranscriptProjection,
    index: &TranscriptIndex,
    event: &EventEnvelopeV1,
) -> usize {
    event
        .correlation_id
        .as_ref()
        .and_then(|id| index.turns.get(id))
        .copied()
        .unwrap_or_else(|| {
            push_message(
                output,
                event,
                ProjectedMessageRole::System,
                event.correlation_id.as_deref(),
            )
        })
}

fn push_part(
    output: &mut TranscriptProjection,
    index: &TranscriptIndex,
    event: &EventEnvelopeV1,
    part: ProjectedPart,
) -> (usize, usize) {
    let message = part_message(output, index, event);
    let target = &mut output.messages[message];
    let at = target.parts.len();
    target.parts.push(part);
    target.provenance.extend(event);
    (message, at)
}

#[cfg(test)]
mod tests;
