use std::path::Path;

use serde::{Deserialize, Serialize};

use super::redaction::{redacted_content_ref, RedactedContentRef};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentDimensions {
    pub width: u32,
    pub height: u32,
}

impl AttachmentDimensions {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentMetadata {
    pub id: String,
    pub mime: String,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<AttachmentDimensions>,
    pub content_ref: RedactedContentRef,
}

impl AttachmentMetadata {
    pub fn from_bytes(
        id: impl Into<String>,
        mime: impl Into<String>,
        path: Option<&Path>,
        bytes: &[u8],
        dimensions: Option<AttachmentDimensions>,
    ) -> Self {
        Self {
            id: id.into(),
            mime: mime.into(),
            size: bytes.len() as u64,
            dimensions,
            content_ref: redacted_content_ref(path, bytes),
        }
    }
}
