use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentCapability {
    None,
    Images,
    ImagesAndText,
}

impl AttachmentCapability {
    pub(crate) fn supports(self, mime: &str) -> bool {
        match self {
            Self::None => false,
            Self::Images => matches!(mime, "image/png" | "image/jpeg"),
            Self::ImagesAndText => matches!(mime, "image/png" | "image/jpeg" | "text/plain"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttachmentProtocol {
    capability: AttachmentCapability,
}

impl AttachmentProtocol {
    pub const fn new(capability: AttachmentCapability) -> Self {
        Self { capability }
    }

    pub const fn openai() -> Self {
        Self::new(AttachmentCapability::ImagesAndText)
    }

    pub(crate) const fn capability(self) -> AttachmentCapability {
        self.capability
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AttachmentProtocolError {
    #[error("provider does not support attachment {attachment_id} with MIME {mime}")]
    UnsupportedCapability { attachment_id: String, mime: String },
    #[error("unsupported attachment MIME for provider serialization: {mime}")]
    UnsupportedMime { mime: String },
    #[error("attachment {attachment_id} metadata size {metadata_size} does not match payload size {payload_size}")]
    SizeMismatch {
        attachment_id: String,
        metadata_size: u64,
        payload_size: u64,
    },
    #[error("text attachment {attachment_id} is not valid UTF-8")]
    InvalidText { attachment_id: String },
}
