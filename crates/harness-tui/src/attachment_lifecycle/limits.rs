use std::convert::TryInto;

use super::mime::MimeKind;
use super::AttachmentError;

pub const MAX_ATTACHMENT_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_DECOMPRESSED_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_IMAGE_PIXELS: u64 = 100 * 1024 * 1024;
pub const MAX_IMAGE_WIDTH: u32 = 16_384;
pub const MAX_IMAGE_HEIGHT: u32 = 16_384;
pub const MAX_COMPRESSION_RATIO: u64 = 100;
pub const READ_CHUNK_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttachmentLimits {
    pub max_bytes: u64,
    pub max_decompressed_bytes: u64,
    pub max_image_pixels: u64,
    pub max_image_width: u32,
    pub max_image_height: u32,
    pub max_compression_ratio: u64,
}

impl Default for AttachmentLimits {
    fn default() -> Self {
        Self {
            max_bytes: MAX_ATTACHMENT_BYTES,
            max_decompressed_bytes: MAX_DECOMPRESSED_BYTES,
            max_image_pixels: MAX_IMAGE_PIXELS,
            max_image_width: MAX_IMAGE_WIDTH,
            max_image_height: MAX_IMAGE_HEIGHT,
            max_compression_ratio: MAX_COMPRESSION_RATIO,
        }
    }
}

impl AttachmentLimits {
    pub const fn with_max_bytes(self, max_bytes: u64) -> Self {
        Self { max_bytes, ..self }
    }

    pub const fn with_max_decompressed_bytes(self, max_decompressed_bytes: u64) -> Self {
        Self {
            max_decompressed_bytes,
            ..self
        }
    }

    pub fn validate(&self, mime: MimeKind, bytes: &[u8]) -> Result<(), AttachmentError> {
        let observed = u64::try_from(bytes.len()).map_err(|_| AttachmentError::AllocationLimit)?;
        if observed > self.max_bytes {
            return Err(AttachmentError::SizeLimit {
                observed,
                limit: self.max_bytes,
            });
        }

        match mime {
            MimeKind::Png => self.validate_image(png_dimensions(bytes)),
            MimeKind::Jpeg => self.validate_image(jpeg_dimensions(bytes)),
            MimeKind::Zip => self.validate_zip(bytes),
            MimeKind::PlainText | MimeKind::Binary | MimeKind::Unknown => Ok(()),
        }
    }

    pub fn dimensions(&self, mime: MimeKind, bytes: &[u8]) -> Option<ImageDimensions> {
        match mime {
            MimeKind::Png => png_dimensions(bytes),
            MimeKind::Jpeg => jpeg_dimensions(bytes),
            MimeKind::PlainText | MimeKind::Binary | MimeKind::Zip | MimeKind::Unknown => None,
        }
    }

    fn validate_image(&self, dimensions: Option<ImageDimensions>) -> Result<(), AttachmentError> {
        let Some(dimensions) = dimensions else {
            return Ok(());
        };
        if dimensions.width > self.max_image_width || dimensions.height > self.max_image_height {
            return Err(AttachmentError::DimensionLimit {
                width: dimensions.width,
                height: dimensions.height,
            });
        }
        let pixels = u64::from(dimensions.width) * u64::from(dimensions.height);
        let decompressed = pixels.saturating_mul(4);
        if pixels > self.max_image_pixels || decompressed > self.max_decompressed_bytes {
            return Err(AttachmentError::DecompressionLimit {
                observed: decompressed,
                limit: self.max_decompressed_bytes,
            });
        }
        Ok(())
    }

    fn validate_zip(&self, bytes: &[u8]) -> Result<(), AttachmentError> {
        if bytes.len() < 26 {
            return Ok(());
        }
        let Ok(compressed) = bytes[18..22].try_into() else {
            return Ok(());
        };
        let Ok(decompressed) = bytes[22..26].try_into() else {
            return Ok(());
        };
        let compressed = u64::from(u32::from_le_bytes(compressed));
        let decompressed = u64::from(u32::from_le_bytes(decompressed));
        if decompressed > self.max_decompressed_bytes
            || (compressed > 0
                && decompressed > compressed.saturating_mul(self.max_compression_ratio))
        {
            return Err(AttachmentError::DecompressionLimit {
                observed: decompressed,
                limit: self.max_decompressed_bytes,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageDimensions {
    pub width: u32,
    pub height: u32,
}

fn png_dimensions(bytes: &[u8]) -> Option<ImageDimensions> {
    if bytes.len() < 24 || !bytes.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]) {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    Some(ImageDimensions { width, height })
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<ImageDimensions> {
    if !bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return None;
    }
    let mut index = 2;
    while index + 3 < bytes.len() {
        if bytes[index] != 0xff {
            index += 1;
            continue;
        }
        while index < bytes.len() && bytes[index] == 0xff {
            index += 1;
        }
        let marker = *bytes.get(index)?;
        index += 1;
        if matches!(marker, 0xd8 | 0xd9 | 0x01) {
            continue;
        }
        let length = usize::from(u16::from_be_bytes(
            bytes.get(index..index + 2)?.try_into().ok()?,
        ));
        if length < 2 || index + length > bytes.len() {
            return None;
        }
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) && length >= 7 {
            let height = u32::from(u16::from_be_bytes(
                bytes[index + 3..index + 5].try_into().ok()?,
            ));
            let width = u32::from(u16::from_be_bytes(
                bytes[index + 5..index + 7].try_into().ok()?,
            ));
            return Some(ImageDimensions { width, height });
        }
        index += length;
    }
    None
}
