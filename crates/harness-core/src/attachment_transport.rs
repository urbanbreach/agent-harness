//! Serialized attachment metadata contains digests, never source paths or payload bytes.
pub use harness_providers::attachment_protocol::{
    AttachmentDimensions, AttachmentMetadata, MAX_ATTACHMENTS, MAX_REQUEST_ATTACHMENT_BYTES,
};
pub type RedactedContentRef = String;
