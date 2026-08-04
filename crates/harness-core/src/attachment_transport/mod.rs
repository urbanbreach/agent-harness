#![allow(
    clippy::mod_module_files,
    reason = "The attachment transport facade intentionally groups focused sibling modules"
)]

mod metadata;
mod ordering;
mod redaction;

pub use metadata::{AttachmentDimensions, AttachmentMetadata};
pub use ordering::{
    checkpoint_attachments, stable_attachment_order, AttachmentCheckpoint, AttachmentOrderingError,
};
pub use redaction::{redacted_content_ref, RedactedContentRef};
