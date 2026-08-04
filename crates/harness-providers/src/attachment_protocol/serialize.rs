use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};

use super::capability::{AttachmentCapability, AttachmentProtocol, AttachmentProtocolError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentDimensions {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentMetadata {
    pub id: String,
    pub mime: String,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<AttachmentDimensions>,
    pub content_ref: String,
}

impl AttachmentMetadata {
    pub fn new(
        id: impl Into<String>,
        mime: impl Into<String>,
        size: u64,
        dimensions: Option<AttachmentDimensions>,
        content_ref: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            mime: mime.into(),
            size,
            dimensions,
            content_ref: content_ref.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentPayload {
    pub metadata: AttachmentMetadata,
    #[serde(skip)]
    pub bytes: Vec<u8>,
}

impl AttachmentPayload {
    pub fn new(metadata: AttachmentMetadata, bytes: Vec<u8>) -> Self {
        Self { metadata, bytes }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerializedAttachment {
    metadata: AttachmentMetadata,
    content: SerializedContent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SerializedContent {
    DataUrl(String),
    Text(String),
}

impl SerializedAttachment {
    pub fn id(&self) -> &str {
        &self.metadata.id
    }

    pub fn data_url(&self) -> Option<&str> {
        match &self.content {
            SerializedContent::DataUrl(value) => Some(value),
            SerializedContent::Text(_) => None,
        }
    }

    pub fn text(&self) -> Option<&str> {
        match &self.content {
            SerializedContent::DataUrl(_) => None,
            SerializedContent::Text(value) => Some(value),
        }
    }

    pub fn metadata(&self) -> &AttachmentMetadata {
        &self.metadata
    }
}

pub fn serialize_attachments(
    protocol: &AttachmentProtocol,
    attachments: &[AttachmentPayload],
) -> Result<Vec<SerializedAttachment>, AttachmentProtocolError> {
    for attachment in attachments {
        ensure_capability(protocol.capability(), attachment)?;
    }

    attachments.iter().map(serialize_one).collect()
}

fn ensure_capability(
    capability: AttachmentCapability,
    attachment: &AttachmentPayload,
) -> Result<(), AttachmentProtocolError> {
    if capability.supports(&attachment.metadata.mime) {
        Ok(())
    } else {
        Err(AttachmentProtocolError::UnsupportedCapability {
            attachment_id: attachment.metadata.id.clone(),
            mime: attachment.metadata.mime.clone(),
        })
    }
}

fn serialize_one(
    attachment: &AttachmentPayload,
) -> Result<SerializedAttachment, AttachmentProtocolError> {
    let payload_size = attachment.bytes.len() as u64;
    if attachment.metadata.size != payload_size {
        return Err(AttachmentProtocolError::SizeMismatch {
            attachment_id: attachment.metadata.id.clone(),
            metadata_size: attachment.metadata.size,
            payload_size,
        });
    }

    let content = match attachment.metadata.mime.as_str() {
        "image/png" | "image/jpeg" => SerializedContent::DataUrl(format!(
            "data:{};base64,{}",
            attachment.metadata.mime,
            STANDARD.encode(&attachment.bytes)
        )),
        "text/plain" => {
            SerializedContent::Text(String::from_utf8(attachment.bytes.clone()).map_err(|_| {
                AttachmentProtocolError::InvalidText {
                    attachment_id: attachment.metadata.id.clone(),
                }
            })?)
        }
        mime => {
            return Err(AttachmentProtocolError::UnsupportedMime {
                mime: mime.to_string(),
            })
        }
    };

    Ok(SerializedAttachment {
        metadata: attachment.metadata.clone(),
        content,
    })
}
