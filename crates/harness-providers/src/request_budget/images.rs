use crate::attachment_protocol::AttachmentDimensions;
use harness_tokenizer::{image_size_from_bytes, Counter, ImageDetail, ImageSize};

const ORIGINAL_MODELS: &[&str] = &[
    "gpt-6-astra",
    "gpt-5.6-sol",
    "gpt-5.6-terra",
    "gpt-5.6-luna",
];

pub(crate) fn check_limits(
    model: &str,
    dimensions: AttachmentDimensions,
    bytes: usize,
    protocol: crate::Protocol,
) -> Result<(), crate::attachment_protocol::AttachmentProtocolError> {
    use crate::attachment_protocol::AttachmentProtocolError as Error;
    if protocol == crate::Protocol::Anthropic {
        if dimensions.width > 8000 || dimensions.height > 8000 {
            return Err(Error("Claude images must fit within 8000 by 8000 pixels"));
        }
        if bytes.div_ceil(3) * 4 > 10_000_000 {
            return Err(Error("Claude image exceeds the 10 MB base64-encoded limit"));
        }
    } else if matches_model(model, ORIGINAL_MODELS) && original_patches(dimensions) > 30_000 {
        return Err(Error("image exceeds 30,000 patches; reduce its dimensions"));
    }
    Ok(())
}

fn original_patches(dimensions: AttachmentDimensions) -> u64 {
    let (mut width, mut height) = (u64::from(dimensions.width), u64::from(dimensions.height));
    let edge = width.max(height);
    if edge > 65_535 {
        width = (width * 65_535 / edge).max(1);
        height = (height * 65_535 / edge).max(1);
    }
    width.div_ceil(32) * height.div_ceil(32)
}

fn matches_model(model: &str, names: &[&str]) -> bool {
    names.iter().any(|name| {
        model.strip_prefix(name).is_some_and(|suffix| {
            suffix.is_empty()
                || suffix == "-latest"
                || suffix.strip_prefix('-').is_some_and(date_suffix)
        })
    })
}

/// Ordinary image price under the request model's rule. Dimensions come from the attachment
/// metadata when present, else from a bounded header probe; unreadable headers price as the
/// maximum image.
pub(super) fn tokens(
    counter: &Counter,
    attachment: &crate::attachment_protocol::AttachmentMetadata,
    bytes: &[u8],
) -> u32 {
    let size = attachment.dimensions.map_or_else(
        || image_size_from_bytes(bytes),
        |dimensions| ImageSize {
            width: dimensions.width,
            height: dimensions.height,
        },
    );
    let detail = attachment
        .detail
        .as_deref()
        .and_then(ImageDetail::from_name);
    counter.image_tokens(Some(size), detail)
}

fn date_suffix(s: &str) -> bool {
    (s.len() == 8 && s.bytes().all(|b| b.is_ascii_digit()))
        || (s.len() == 10
            && s.bytes().enumerate().all(|(i, b)| {
                if i == 4 || i == 7 {
                    b == b'-'
                } else {
                    b.is_ascii_digit()
                }
            }))
}
