use base64::engine::general_purpose::STANDARD;
use base64::Engine;

use crate::count::Counter;

/// Requested image detail level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageDetail {
    Auto,
    Low,
    High,
    Original,
}

impl ImageDetail {
    /// Parses the wire name of a detail hint.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "auto" => Some(Self::Auto),
            "low" => Some(Self::Low),
            "high" => Some(Self::High),
            "original" => Some(Self::Original),
            _ => None,
        }
    }
}

/// Pixel dimensions of an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImageSize {
    pub width: u32,
    pub height: u32,
}

mod rules;

/// Dimension assumed for a recognized image whose header is unreadable.
const UNREADABLE_EDGE: u32 = 65535;
/// Base64 characters decoded for the header probe (64 KiB of bytes).
const MAX_BASE64_CHARS: usize = 87384;
/// Header bytes probed for image dimensions.
const MAX_HEADER_BYTES: usize = MAX_BASE64_CHARS / 4 * 3;

pub(crate) use rules::{resolve, Rule};

/// Identity of the ordinary image rule, compared by counter equivalence.
pub(crate) fn rule_key(rule: Option<Rule>) -> String {
    rules::key(rule, None)
}

impl Counter {
    /// Ordinary image price under the reader's lineage rule, then its wire rule, then the
    /// OpenAI Responses fallback. `None` size means unknown dimensions (65535x65535).
    pub fn image_tokens(&self, size: Option<ImageSize>, detail: Option<ImageDetail>) -> u32 {
        let rule = self.image_rule().unwrap_or_else(rules::fallback);
        let size = size.unwrap_or(ImageSize {
            width: UNREADABLE_EDGE,
            height: UNREADABLE_EDGE,
        });
        rules::price(&rule.regime, size, detail)
    }

    /// Archive frame price. A zero dimension means an unreadable frame, and a reader without a
    /// rule prices every frame at the ceiling.
    pub fn frame_tokens(&self, width: u32, height: u32) -> u32 {
        if width == 0 || height == 0 {
            return rules::FRAME_CEILING_TOKENS;
        }
        match self.image_rule() {
            Some(rule) => rules::price(
                &rule.regime,
                ImageSize { width, height },
                frame_detail(&rule),
            ),
            None => rules::FRAME_CEILING_TOKENS,
        }
    }

    /// Price shape planning charges for a square frame: the frame price when the rule is
    /// verified, otherwise never below the ceiling.
    pub fn frame_planning_tokens(&self, width: u32, height: u32) -> u32 {
        let price = self.frame_tokens(width, height);
        if self.image_rule().is_some_and(|rule| rule.verified) {
            price
        } else {
            price.max(rules::FRAME_CEILING_TOKENS)
        }
    }

    /// Identity of the frame billing rule, compared by [`Counter::equivalent`].
    pub fn frame_key(&self) -> String {
        let rule = self.image_rule();
        rules::key(rule, rule.as_ref().and_then(frame_detail))
    }

    pub(crate) fn image_rule(&self) -> Option<Rule> {
        self.image_rule
    }
}

/// Archive frames ask OpenAI-style rules for original detail and omit detail elsewhere.
fn frame_detail(rule: &Rule) -> Option<ImageDetail> {
    matches!(rule.regime, rules::Regime::OpenAi { .. }).then_some(ImageDetail::Original)
}

/// Dimensions of an inline base64 image. Remote URLs and file ids return `None`;
/// a recognized data URL with an unreadable header returns 65535x65535.
pub fn image_size_from_data_url(data_url: &str) -> Option<ImageSize> {
    let rest = data_url.strip_prefix("data:")?;
    let (meta, payload) = rest.split_once(',')?;
    if !meta.ends_with(";base64") {
        return None;
    }
    let take = payload.len().min(MAX_BASE64_CHARS) / 4 * 4;
    let Ok(bytes) = STANDARD.decode(&payload.as_bytes()[..take]) else {
        return Some(unreadable_size());
    };
    Some(image_size_from_bytes(&bytes))
}

/// Dimensions from the header of raw image bytes; only the first 64 KiB is inspected. An
/// unreadable or zero-sized header returns 65535x65535.
pub fn image_size_from_bytes(bytes: &[u8]) -> ImageSize {
    let header = &bytes[..bytes.len().min(MAX_HEADER_BYTES)];
    probe_dimensions(header)
        .filter(|&(width, height)| width > 0 && height > 0)
        .map_or_else(unreadable_size, |(width, height)| ImageSize {
            width,
            height,
        })
}

fn unreadable_size() -> ImageSize {
    ImageSize {
        width: UNREADABLE_EDGE,
        height: UNREADABLE_EDGE,
    }
}

fn probe_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        png(bytes)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some((u32::from(le16(bytes, 6)?), u32::from(le16(bytes, 8)?)))
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        webp(bytes)
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        jpeg(bytes)
    } else {
        None
    }
}

fn png(bytes: &[u8]) -> Option<(u32, u32)> {
    (bytes.get(12..16)? == b"IHDR").then_some(())?;
    Some((be32(bytes, 16)?, be32(bytes, 20)?))
}

fn webp(bytes: &[u8]) -> Option<(u32, u32)> {
    match bytes.get(12..16)? {
        b"VP8 " => {
            (bytes.get(23..26)? == [0x9D, 0x01, 0x2A]).then_some(())?;
            Some((
                u32::from(le16(bytes, 26)? & 0x3FFF),
                u32::from(le16(bytes, 28)? & 0x3FFF),
            ))
        }
        b"VP8L" => {
            (*bytes.get(20)? == 0x2F).then_some(())?;
            let bits = le32(bytes, 21)?;
            Some(((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1))
        }
        b"VP8X" => Some((le24(bytes, 24)? + 1, le24(bytes, 27)? + 1)),
        _ => None,
    }
}

fn jpeg(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut pos = 2;
    loop {
        if *bytes.get(pos)? != 0xFF {
            return None;
        }
        let mut marker = *bytes.get(pos + 1)?;
        pos += 2;
        while marker == 0xFF {
            marker = *bytes.get(pos)?;
            pos += 1;
        }
        match marker {
            // Standalone markers carry no length.
            0x01 | 0xD0..=0xD7 => continue,
            // End of image or start of scan before any frame header.
            0xD9 | 0xDA => return None,
            _ => {}
        }
        let length = usize::from(be16(bytes, pos)?);
        if matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            return Some((
                u32::from(be16(bytes, pos + 5)?),
                u32::from(be16(bytes, pos + 3)?),
            ));
        }
        pos += length;
    }
}

fn be16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn le16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn le32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn le24(bytes: &[u8], at: usize) -> Option<u32> {
    let [a, b, c] = *bytes.get(at..at + 3)? else {
        return None;
    };
    Some(u32::from_le_bytes([a, b, c, 0]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_url(bytes: &[u8]) -> String {
        format!("data:image/x;base64,{}", STANDARD.encode(bytes))
    }

    fn size(width: u32, height: u32) -> Option<ImageSize> {
        Some(ImageSize { width, height })
    }

    #[test]
    fn probes_each_format_header() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend([0, 0, 1, 0, 0, 0, 0, 200]);
        assert_eq!(image_size_from_data_url(&data_url(&png)), size(256, 200));

        let mut gif = b"GIF89a".to_vec();
        gif.extend([3, 0, 2, 0]);
        assert_eq!(image_size_from_data_url(&data_url(&gif)), size(3, 2));

        let mut webp = b"RIFF\0\0\0\0WEBPVP8X\n\0\0\0\0\0\0\0".to_vec();
        webp.extend([9, 0, 0, 4, 0, 0]);
        assert_eq!(image_size_from_data_url(&data_url(&webp)), size(10, 5));

        // SOI, an APP0 segment, then SOF0 with height 7 and width 9.
        let jpeg = [
            0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0, 0xFF, 0xC0, 0, 11, 8, 0, 7, 0, 9, 1, 1, 0x11, 0,
        ];
        assert_eq!(image_size_from_data_url(&data_url(&jpeg)), size(9, 7));
    }

    #[test]
    fn unreadable_and_unrecognized_inputs() {
        let unreadable = size(65535, 65535);
        assert_eq!(
            image_size_from_data_url(&data_url(b"not an image")),
            unreadable
        );
        assert_eq!(
            image_size_from_data_url("data:image/png;base64,@@@@"),
            unreadable
        );
        let mut zero = b"GIF89a".to_vec();
        zero.extend([0, 0, 2, 0]);
        assert_eq!(image_size_from_data_url(&data_url(&zero)), unreadable);

        assert_eq!(image_size_from_data_url("https://example.com/a.png"), None);
        assert_eq!(image_size_from_data_url("data:image/png,abcd"), None);
        assert_eq!(image_size_from_data_url("data:image/png;base64"), None);
    }
}
