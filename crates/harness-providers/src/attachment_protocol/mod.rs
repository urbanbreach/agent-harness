#![allow(
    clippy::mod_module_files,
    reason = "The attachment protocol facade intentionally groups focused sibling modules"
)]

mod capability;
mod serialize;

pub use capability::{AttachmentCapability, AttachmentProtocol, AttachmentProtocolError};
pub use serialize::{
    serialize_attachments, AttachmentDimensions, AttachmentMetadata, AttachmentPayload,
    SerializedAttachment,
};
