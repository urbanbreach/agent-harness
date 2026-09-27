use crate::attachment_protocol::AttachmentDimensions;

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

/// Conservative upper estimates for default image detail. Unknown models remain unavailable.
/// Sources: developers.openai.com/api/docs/guides/images-vision and
/// platform.claude.com/docs/en/build-with-claude/vision, checked 2026-09-26.
pub(super) fn tokens(model: &str, dimensions: AttachmentDimensions) -> Option<u32> {
    let matches = |names: &[&str]| matches_model(model, names);
    let (mut width, mut height) = (u64::from(dimensions.width), u64::from(dimensions.height));
    let patches = |size: u64| width.div_ceil(size) * height.div_ceil(size);
    let patch_cost = |cap: u64, multiplier: u64| {
        u32::try_from((patches(32).min(cap) * multiplier).div_ceil(100)).ok()
    };
    if matches(ORIGINAL_MODELS) {
        let count = original_patches(dimensions);
        if count > 30_000 {
            return None;
        }
        return u32::try_from((count * 120).div_ceil(100)).ok();
    }
    if matches(&["gpt-5.5"]) {
        return patch_cost(10_000, 120);
    }
    if matches(&["gpt-5.4", "gpt-5.4-mini", "gpt-5.4-nano"]) {
        return patch_cost(2500, 120);
    }
    if matches(&["gpt-5.2", "gpt-5-mini"]) {
        return patch_cost(6144, 120);
    }
    if matches(&["gpt-4.1-mini"]) {
        return patch_cost(6144, 162);
    }
    if matches(&["gpt-4.1-nano"]) {
        return patch_cost(1536, 246);
    }
    if matches(&["gpt-5-nano"]) {
        return patch_cost(1536, 150);
    }
    if matches(&["o4-mini"]) {
        return patch_cost(1536, 172);
    }
    if matches(&[
        "claude-opus-4-7",
        "claude-sonnet-4-7",
        "claude-opus-5",
        "claude-opus-5-5",
    ]) {
        return u32::try_from(patches(28).min(4784)).ok();
    }
    if matches(&[
        "claude-3-5-sonnet",
        "claude-3-7-sonnet",
        "claude-sonnet-4",
        "claude-opus-4",
        "claude-opus-4-1",
        "claude-sonnet-4-5",
        "claude-opus-4-5",
        "claude-sonnet-4-6",
        "claude-opus-4-6",
        "claude-haiku-4-5",
    ]) {
        return u32::try_from(patches(28).min(1568)).ok();
    }
    let (base, tile) = if matches(&["gpt-4o-mini"]) {
        (2833, 5667)
    } else if matches(&["gpt-4o", "gpt-4.1", "chatgpt-4o-latest"]) {
        (85, 170)
    } else if matches(&["gpt-5", "gpt-5.1"]) {
        (70, 140)
    } else if matches(&["o1", "o1-pro", "o3"]) {
        (75, 150)
    } else {
        return None;
    };
    for long_edge in [true, false] {
        let (edge, limit) = if long_edge {
            (width.max(height), 2048)
        } else {
            (width.min(height), 768)
        };
        if edge > limit {
            width = (width * limit).div_ceil(edge);
            height = (height * limit).div_ceil(edge);
        }
    }
    u32::try_from(base + tile * width.div_ceil(512) * height.div_ceil(512)).ok()
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
