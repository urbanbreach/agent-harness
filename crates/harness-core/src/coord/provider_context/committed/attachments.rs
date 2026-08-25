use std::collections::BTreeMap;

use crate::attachment_transport::{stable_attachment_order, AttachmentMetadata};
use crate::event::{EventEnvelopeV1, EventV1};

use super::{event_belongs_to_agent, ProviderContextReconstructionError};

pub(super) fn attachments_by_request(
    events: &[EventEnvelopeV1],
    agent_id: &str,
) -> Result<BTreeMap<String, Vec<AttachmentMetadata>>, ProviderContextReconstructionError> {
    let stream_key = format!("agent:{agent_id}");
    let mut by_request = BTreeMap::<String, Vec<AttachmentMetadata>>::new();
    for event in events
        .iter()
        .filter(|event| event_belongs_to_agent(event, agent_id, &stream_key))
    {
        let EventV1::PromptAttachmentsSubmitted(submitted) = &event.payload else {
            continue;
        };
        by_request
            .entry(submitted.request_id.to_string())
            .or_default()
            .extend(submitted.attachments.iter().cloned());
    }
    for (request_id, attachments) in &mut by_request {
        *attachments = stable_attachment_order(attachments).map_err(|source| {
            ProviderContextReconstructionError::Attachments {
                request_id: request_id.clone(),
                source,
            }
        })?;
    }
    Ok(by_request)
}
