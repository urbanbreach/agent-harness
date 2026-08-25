#![allow(
    clippy::mod_module_files,
    reason = "The attachment transport facade intentionally groups focused sibling modules"
)]

mod metadata;
mod ordering;
mod provider_context;
mod redaction;

pub use metadata::{AttachmentDimensions, AttachmentMetadata};
pub use ordering::{
    checkpoint_attachments, stable_attachment_order, AttachmentCheckpoint, AttachmentOrderingError,
};
pub(crate) use provider_context::{historical_attachment_tokens, lower_provider_attachments};
pub use redaction::{redacted_content_ref, RedactedContentRef};
