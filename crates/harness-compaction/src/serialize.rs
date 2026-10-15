//! Summary transcripts omit opaque replay state and preserve tool text verbatim.
//!
//! One preparation step (useless-pair removal, result truncation, empty-record dropping)
//! feeds either the labeled default transcript or one native dialect renderer.
use crate::Item;

mod chatml;
mod deepseek;
mod default;
mod gemini;
mod gemma;
mod glm;
mod harmony;
mod json;
mod kimi;
mod legacy;
mod lookup;
mod transcript;

#[cfg(test)]
pub(crate) mod tests;

pub(crate) use json::ordered_arguments;
pub use lookup::dialect_for_model;

/// Transcript syntax chosen for the summarizer model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// Labeled `[Role]: text` lines. Never selected by model identifier.
    Default,
    Xml,
    Minimax,
    Anthropic,
    Harmony,
    /// ChatML with a `tool` role. Never selected by model identifier.
    Hermes,
    Qwen3,
    Gemini,
    Gemma,
    Glm,
    DeepSeek,
    Kimi,
}

pub(crate) fn escape_boundaries(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        result.push_str(&rest[..start]);
        let tail = &rest[start + 1..];
        let whitespace = |c: char| (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}';
        let tag = tail.trim_start_matches(whitespace);
        let tag = tag
            .strip_prefix('/')
            .unwrap_or(tag)
            .trim_start_matches(whitespace);
        let matched = ["conversation", "previous-summary"].iter().any(|name| {
            tag.get(..name.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(name))
                && tag
                    .get(name.len()..)
                    .is_some_and(|suffix| suffix.trim_start_matches(whitespace).starts_with('>'))
        });
        result.push_str(if matched { "&lt;" } else { "<" });
        rest = tail;
    }
    result.push_str(rest);
    result
}

/// Truncation counts UTF-16 units. A cut inside a surrogate pair retreats to the previous
/// scalar boundary, so the kept text may be one unit shorter than requested: Rust strings
/// cannot hold an unpaired surrogate, which is a known divergence from the reference. The
/// notice still reports `original length - keep`.
pub(crate) fn truncate_units(text: &str, keep: usize) -> String {
    let length = text.encode_utf16().count();
    if length <= keep {
        return text.to_owned();
    }
    let mut used = 0;
    let mut end = 0;
    for (index, scalar) in text.char_indices() {
        if used + scalar.len_utf16() > keep {
            break;
        }
        used += scalar.len_utf16();
        end = index + scalar.len_utf8();
    }
    format!(
        "{}\n\n[... {} more characters truncated]",
        &text[..end],
        length - keep
    )
}

/// The transcript a dialect's native renderer produces, with raw control markers.
fn render(items: &[Item<'_>], dialect: Dialect) -> String {
    let keep_thinking = dialect != Dialect::Anthropic;
    let entries = transcript::prepare(items, keep_thinking, dialect != Dialect::Default);
    match dialect {
        Dialect::Default => default::render(&entries),
        Dialect::Xml | Dialect::Minimax | Dialect::Anthropic => legacy::render(dialect, &entries),
        Dialect::Harmony => harmony::render(&entries),
        Dialect::Hermes | Dialect::Qwen3 => chatml::render(dialect, &entries),
        Dialect::Gemini => gemini::render(&entries),
        Dialect::Gemma => gemma::render(&entries),
        Dialect::Glm => glm::render(&entries),
        Dialect::DeepSeek => deepseek::render(&entries),
        Dialect::Kimi => kimi::render(&entries),
    }
}

/// Serialize visible history, removing useless pairs and escaping boundary tags.
pub fn serialize_for_summary(items: &[Item<'_>], dialect: Dialect) -> String {
    let text = render(items, dialect);
    escape_boundaries(&if dialect == Dialect::Harmony {
        harmony::neutralize(&text)
    } else {
        text
    })
}
