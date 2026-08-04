use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::event::{EventEnvelopeV1, EventV1};

use super::AttachmentMetadata;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AttachmentOrderingError {
    #[error("attachment id appears more than once: {id}")]
    DuplicateId { id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AttachmentCheckpoint {
    by_request: BTreeMap<String, Vec<AttachmentMetadata>>,
}

impl AttachmentCheckpoint {
    pub fn for_request(&self, request_id: &str) -> Option<&[AttachmentMetadata]> {
        self.by_request.get(request_id).map(Vec::as_slice)
    }
}

pub fn stable_attachment_order(
    attachments: &[AttachmentMetadata],
) -> Result<Vec<AttachmentMetadata>, AttachmentOrderingError> {
    let mut seen = BTreeSet::new();
    for attachment in attachments {
        if !seen.insert(attachment.id.clone()) {
            return Err(AttachmentOrderingError::DuplicateId {
                id: attachment.id.clone(),
            });
        }
    }
    Ok(attachments.to_vec())
}

pub fn checkpoint_attachments(
    events: &[EventEnvelopeV1],
    through_seq: u64,
) -> AttachmentCheckpoint {
    let mut checkpoint = AttachmentCheckpoint::default();
    for event in events.iter().filter(|event| event.seq <= through_seq) {
        if let EventV1::PromptAttachmentsSubmitted(payload) = &event.payload {
            checkpoint
                .by_request
                .entry(payload.request_id.to_string())
                .or_default()
                .extend(payload.attachments.iter().cloned());
        }
    }
    checkpoint
}
