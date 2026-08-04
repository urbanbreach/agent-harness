use super::limits::{AttachmentLimits, ImageDimensions};
use super::mime::MimeKind;
use super::AttachmentError;

pub const MAX_PREVIEW_TEXT_BYTES: usize = 4 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Preview {
    Image {
        mime: MimeKind,
        dimensions: Option<ImageDimensions>,
    },
    Text {
        text: String,
        truncated: bool,
    },
    Binary {
        bytes: usize,
        hex_prefix: String,
    },
}

pub fn build_preview(
    bytes: &[u8],
    mime: MimeKind,
    limits: &AttachmentLimits,
) -> Result<Preview, AttachmentError> {
    limits.validate(mime, bytes)?;
    match mime {
        MimeKind::Png | MimeKind::Jpeg => Ok(Preview::Image {
            mime,
            dimensions: limits.dimensions(mime, bytes),
        }),
        MimeKind::PlainText => {
            let (text, truncated) = bounded_text(bytes);
            Ok(Preview::Text { text, truncated })
        }
        MimeKind::Binary => Ok(Preview::Binary {
            bytes: bytes.len(),
            hex_prefix: hex_prefix(bytes),
        }),
        MimeKind::Zip | MimeKind::Unknown => Err(AttachmentError::MimeRejected { mime }),
    }
}

fn bounded_text(bytes: &[u8]) -> (String, bool) {
    let text = String::from_utf8_lossy(bytes);
    let Some((end, _)) = text.char_indices().nth(MAX_PREVIEW_TEXT_BYTES) else {
        return (text.into_owned(), false);
    };
    (text[..end].to_owned(), true)
}

fn hex_prefix(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join("")
}
