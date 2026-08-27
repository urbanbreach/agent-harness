use thiserror::Error;

use crate::agent::{ProviderContext, ProviderConversationTurn};
use crate::attachment_transport::{lower_provider_attachments, AttachmentOrderingError};
use crate::conversation::{ConversationMessage, ConversationProjection};
use crate::event::{EventEnvelopeV1, EventV1};
use crate::session::{CanonicalSessionProjection, CanonicalSessionProjectionError};

use self::attachments::attachments_by_request;
use self::tool_lifecycle::admitted_tool_lifecycle_event_ids;

mod attachments;
mod tool_lifecycle;

#[cfg(test)]
mod tests;

#[derive(Debug, Error, PartialEq, Eq)]
pub(in crate::coord) enum ProviderContextReconstructionError {
    #[error(transparent)]
    Projection(#[from] CanonicalSessionProjectionError),
    #[error("attachments for request `{request_id}` are malformed: {source}")]
    Attachments {
        request_id: String,
        source: AttachmentOrderingError,
    },
}

pub(in crate::coord) fn reconstruct_provider_context_from_events(
    events: &[EventEnvelopeV1],
    agent_id: &str,
) -> Result<ProviderContext, ProviderContextReconstructionError> {
    let mut projection = project_committed_context(events, agent_id)?;
    let attachments_by_request = attachments_by_request(events, agent_id)?;
    for message in &mut projection.messages {
        let ConversationMessage::User(user) = message else {
            continue;
        };
        if let Some(values) = attachments_by_request.get(user.request_id.as_str()) {
            user.text = lower_provider_attachments(&user.text, values);
        }
    }

    let mut compacted_summary = None;
    let mut suffix = Vec::with_capacity(projection.messages.len());
    for message in projection.messages {
        match message {
            ConversationMessage::Checkpoint(checkpoint) => {
                compacted_summary = Some(checkpoint.summary);
            }
            ConversationMessage::User(_)
            | ConversationMessage::Assistant(_)
            | ConversationMessage::ToolResult(_) => suffix.push(message),
        }
    }
    let preserved_turns = (!suffix.is_empty())
        .then(|| {
            let attachments = suffix
                .iter()
                .filter_map(|message| match message {
                    ConversationMessage::User(user) => {
                        attachments_by_request.get(user.request_id.as_str())
                    }
                    ConversationMessage::Checkpoint(_)
                    | ConversationMessage::Assistant(_)
                    | ConversationMessage::ToolResult(_) => None,
                })
                .flatten()
                .cloned()
                .collect();
            ProviderConversationTurn {
                messages: suffix,
                attachments,
                ..ProviderConversationTurn::default()
            }
        })
        .into_iter()
        .collect();
    Ok(ProviderContext {
        compacted_summary,
        preserved_turns,
        checkpoint: None,
    })
}

pub(in crate::coord) fn project_committed_context(
    events: &[EventEnvelopeV1],
    agent_id: &str,
) -> Result<ConversationProjection, CanonicalSessionProjectionError> {
    let stream_key = format!("agent:{agent_id}");
    let lifecycle_event_ids = admitted_tool_lifecycle_event_ids(events, agent_id);
    let agent_events = events
        .iter()
        .filter(|event| {
            event_belongs_to_agent(event, agent_id, &stream_key)
                || lifecycle_event_ids.contains(event.event_id.as_str())
        })
        .cloned()
        .collect::<Vec<_>>();
    CanonicalSessionProjection::from_owner_event_history(events, &agent_events, agent_id)
        .map(|projection| projection.conversation)
}

pub(in crate::coord) fn latest_agent_event_seq(
    events: &[EventEnvelopeV1],
    agent_id: &str,
) -> Option<u64> {
    let stream_key = format!("agent:{agent_id}");
    events
        .iter()
        .filter(|event| event_belongs_to_agent(event, agent_id, &stream_key))
        .map(|event| event.seq)
        .max()
}

pub(in crate::coord) fn event_belongs_to_agent(
    event: &EventEnvelopeV1,
    agent_id: &str,
    stream_key: &str,
) -> bool {
    matches!(
        &event.payload,
        EventV1::SessionCompaction(compaction) if compaction.agent_id == agent_id
    ) || event.actor.agent_id.as_deref() == Some(agent_id)
        || event.stream_key.as_deref() == Some(stream_key)
}
