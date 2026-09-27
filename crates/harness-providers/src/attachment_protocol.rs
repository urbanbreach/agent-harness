use serde::{Deserialize, Serialize};
use std::{fmt, sync::Arc};

pub const MAX_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_REQUEST_ATTACHMENT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_ATTACHMENTS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentCapability {
    None,
    Images,
    ImagesAndText,
}
#[derive(Debug, Clone, Copy)]
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
}

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
#[derive(Clone, Serialize, Deserialize)]
pub struct AttachmentMetadata {
    pub id: String,
    pub mime: String,
    pub size: u64,
    pub dimensions: Option<AttachmentDimensions>,
    pub content_ref: String,
    #[serde(skip)]
    bytes: Option<Arc<[u8]>>,
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
            bytes: None,
        }
    }
    pub fn from_bytes(
        id: impl Into<String>,
        mime: impl Into<String>,
        _path: Option<&std::path::Path>,
        bytes: &[u8],
        dimensions: Option<AttachmentDimensions>,
    ) -> Self {
        let mut metadata = Self::new(
            id,
            mime,
            bytes.len() as u64,
            dimensions,
            format!("attachment:blake3:{}", blake3::hash(bytes)),
        );
        if bytes.len() <= MAX_ATTACHMENT_BYTES {
            metadata.bytes = Some(Arc::from(bytes));
        }
        metadata
    }
    pub fn bytes(&self) -> Result<&[u8], AttachmentProtocolError> {
        let bytes = self
            .bytes
            .as_deref()
            .ok_or(AttachmentProtocolError("attachment bytes are unavailable"))?;
        validate(self, bytes)?;
        if self.content_ref != format!("attachment:blake3:{}", blake3::hash(bytes)) {
            return Err(AttachmentProtocolError(
                "attachment digest does not match its bytes",
            ));
        }
        Ok(bytes)
    }
    pub fn with_bytes(mut self, bytes: Vec<u8>) -> Result<Self, AttachmentProtocolError> {
        validate(&self, &bytes)?;
        self.bytes = Some(bytes.into());
        self.bytes()?;
        Ok(self)
    }
}
impl fmt::Debug for AttachmentMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AttachmentMetadata")
            .field("id", &self.id)
            .field("mime", &self.mime)
            .field("size", &self.size)
            .field("dimensions", &self.dimensions)
            .field("content_ref", &self.content_ref)
            .finish()
    }
}
impl PartialEq for AttachmentMetadata {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.mime == other.mime
            && self.size == other.size
            && self.dimensions == other.dimensions
            && self.content_ref == other.content_ref
    }
}
impl Eq for AttachmentMetadata {}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentPayload {
    pub metadata: AttachmentMetadata,
    #[serde(skip)]
    pub bytes: Vec<u8>,
}
impl fmt::Debug for AttachmentPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.metadata.fmt(f)
    }
}
impl AttachmentPayload {
    pub fn new(metadata: AttachmentMetadata, bytes: Vec<u8>) -> Self {
        Self { metadata, bytes }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerializedAttachment {
    metadata: AttachmentMetadata,
    content: String,
    image: bool,
}
impl SerializedAttachment {
    pub fn id(&self) -> &str {
        &self.metadata.id
    }
    pub fn metadata(&self) -> &AttachmentMetadata {
        &self.metadata
    }
    pub fn data_url(&self) -> Option<&str> {
        self.image.then_some(self.content.as_str())
    }
    pub fn text(&self) -> Option<&str> {
        (!self.image).then_some(self.content.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct AttachmentProtocolError(pub &'static str);

fn validate(metadata: &AttachmentMetadata, bytes: &[u8]) -> Result<(), AttachmentProtocolError> {
    if bytes.len() > MAX_ATTACHMENT_BYTES || metadata.size != bytes.len() as u64 {
        return Err(AttachmentProtocolError("attachment size is invalid"));
    }
    if !matches!(
        metadata.mime.as_str(),
        "image/png" | "image/jpeg" | "image/webp" | "image/gif" | "text/plain"
    ) {
        return Err(AttachmentProtocolError("unsupported attachment MIME type"));
    }
    if metadata.dimensions.is_some_and(|d| {
        d.width == 0 || d.height == 0 || u64::from(d.width) * u64::from(d.height) > 100_000_000
    }) {
        return Err(AttachmentProtocolError("invalid attachment dimensions"));
    }
    if metadata.mime == "text/plain" && std::str::from_utf8(bytes).is_err() {
        return Err(AttachmentProtocolError("text attachment is not UTF-8"));
    }
    if metadata.mime.starts_with("image/") {
        let dimensions = image_dimensions(&metadata.mime, bytes)?;
        if metadata
            .dimensions
            .is_some_and(|declared| declared != dimensions)
        {
            return Err(AttachmentProtocolError(
                "attachment dimensions do not match the image",
            ));
        }
    }
    Ok(())
}

pub(crate) fn image_dimensions(
    mime: &str,
    bytes: &[u8],
) -> Result<AttachmentDimensions, AttachmentProtocolError> {
    let format = match mime {
        "image/png" => image::ImageFormat::Png,
        "image/jpeg" => image::ImageFormat::Jpeg,
        "image/gif" => image::ImageFormat::Gif,
        "image/webp" => image::ImageFormat::WebP,
        _ => return Err(AttachmentProtocolError("unsupported image format")),
    };
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(1024 * 1024);
    reader.limits(limits);
    let (width, height) = reader
        .into_dimensions()
        .map_err(|_| AttachmentProtocolError("invalid image header"))?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 100_000_000 {
        return Err(AttachmentProtocolError("invalid image dimensions"));
    }
    Ok(AttachmentDimensions { width, height })
}

pub(crate) fn validate_request(
    request: &crate::CompletionRequest,
    protocol: crate::Protocol,
) -> Result<(), AttachmentProtocolError> {
    let (mut count, mut total) = (0, 0usize);
    for (index, attachments) in &request.attachments {
        if request
            .messages
            .get(*index)
            .is_none_or(|m| !matches!(m.role, crate::MessageRole::User | crate::MessageRole::Tool))
        {
            return Err(AttachmentProtocolError(
                "attachments require a user message or tool result",
            ));
        }
        for attachment in attachments {
            count += 1;
            let bytes = attachment.bytes()?;
            total = total.saturating_add(bytes.len());
            if count > MAX_ATTACHMENTS || total > MAX_REQUEST_ATTACHMENT_BYTES {
                return Err(AttachmentProtocolError("request attachment limit exceeded"));
            }
            if attachment.mime.starts_with("image/") {
                crate::request_budget::check_image_limits(
                    &request.model_id,
                    image_dimensions(&attachment.mime, bytes)?,
                    bytes.len(),
                    protocol,
                )?;
            }
        }
    }
    Ok(())
}

pub fn serialize_attachments(
    protocol: &AttachmentProtocol,
    attachments: &[AttachmentPayload],
) -> Result<Vec<SerializedAttachment>, AttachmentProtocolError> {
    use base64::Engine;
    if attachments.len() > MAX_ATTACHMENTS {
        return Err(AttachmentProtocolError("too many attachments"));
    }
    let mut total = 0_usize;
    for attachment in attachments {
        let size = attachment.bytes.len();
        if total.saturating_add(size) > MAX_REQUEST_ATTACHMENT_BYTES {
            return Err(AttachmentProtocolError("attachment byte limit exceeded"));
        }
        total += size;
        validate(&attachment.metadata, &attachment.bytes)?;
        let text = attachment.metadata.mime == "text/plain";
        if protocol.capability == AttachmentCapability::None
            || (text && protocol.capability == AttachmentCapability::Images)
        {
            return Err(AttachmentProtocolError(
                "provider does not support this attachment",
            ));
        }
    }
    attachments
        .iter()
        .map(|a| {
            let image = a.metadata.mime.starts_with("image/");
            let content = if image {
                format!(
                    "data:{};base64,{}",
                    a.metadata.mime,
                    base64::engine::general_purpose::STANDARD.encode(&a.bytes)
                )
            } else {
                std::str::from_utf8(&a.bytes)
                    .map_err(|_| AttachmentProtocolError("text attachment is not UTF-8"))?
                    .to_owned()
            };
            Ok(SerializedAttachment {
                metadata: a.metadata.clone(),
                image,
                content,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachments_validate_payloads_before_serialization() -> Result<(), AttachmentProtocolError> {
        let mut text = AttachmentPayload::new(
            AttachmentMetadata::new("a", "text/plain", 5, None, "safe-ref"),
            b"hello".to_vec(),
        );
        let encoded = serialize_attachments(&AttachmentProtocol::openai(), &[text.clone()])?;
        assert_eq!(encoded[0].text(), Some("hello"));
        assert!(serialize_attachments(
            &AttachmentProtocol::new(AttachmentCapability::Images),
            &[text.clone()]
        )
        .is_err());
        text.metadata.size = 4;
        assert!(serialize_attachments(&AttachmentProtocol::openai(), &[text]).is_err());
        let image = AttachmentPayload::new(
            AttachmentMetadata::new("b", "image/png", 3, None, "safe-ref"),
            vec![1, 2, 3],
        );
        assert!(serialize_attachments(&AttachmentProtocol::openai(), &[image]).is_err());
        Ok(())
    }
}
