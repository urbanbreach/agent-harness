//! Account display names (senpi `auth/pool/slots.ts`): untrusted presentation input.
use super::AccountSlot;
use regex::Regex;
use std::sync::LazyLock;

pub const DISPLAY_NAME_MAX_COLUMNS: usize = 32;
static UNSAFE_DISPLAY: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"[\p{Cc}\p{Cf}\p{Zl}\p{Zp}]").ok());
static MARK: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"^\p{M}$").ok());
static WIDE: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"^(?:[\x{1100}-\x{115f}\x{2329}\x{232a}\x{2e80}-\x{303e}\x{3041}-\x{33ff}\x{3400}-\x{4dbf}\x{4e00}-\x{9fff}\x{a000}-\x{a4cf}\x{a960}-\x{a97f}\x{ac00}-\x{d7a3}\x{f900}-\x{faff}\x{fe10}-\x{fe19}\x{fe30}-\x{fe6f}\x{ff00}-\x{ff60}\x{ffe0}-\x{ffe6}]|[\x{1f300}-\x{1faff}]|[\x{20000}-\x{3fffd}])$").ok()
});
const INVISIBLE: [char; 7] = [
    '\u{115f}', '\u{1160}', '\u{17b4}', '\u{17b5}', '\u{2800}', '\u{3164}', '\u{ffa0}',
];

fn is_mark(c: char) -> bool {
    MARK.as_ref().is_some_and(|re| re.is_match(&c.to_string()))
}
fn invisible(c: char) -> bool {
    is_mark(c) || INVISIBLE.contains(&c)
}

/// Terminal columns a label occupies, measured per grapheme cluster.
pub fn display_name_columns(value: &str) -> usize {
    use unicode_segmentation::UnicodeSegmentation;
    value
        .graphemes(true)
        .filter_map(|segment| {
            let base = segment.chars().next().filter(|c| !invisible(*c))?;
            let wide = WIDE
                .as_ref()
                .is_some_and(|re| re.is_match(&base.to_string()));
            Some(if wide || segment.contains('\u{fe0f}') {
                2
            } else {
                1
            })
        })
        .sum()
}

/// The stored form: NFC, trimmed, internal whitespace runs collapsed to one space.
fn normalize_display_name(value: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    value
        .nfc()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Treats persisted display names as untrusted presentation input (`accountDisplayName`).
pub fn account_display_name(value: Option<&str>) -> Option<String> {
    let value = value?;
    if UNSAFE_DISPLAY.as_ref().is_none_or(|re| re.is_match(value)) {
        return None;
    }
    let normalized = normalize_display_name(value);
    let first = normalized.chars().next()?;
    if is_mark(first)
        || normalized
            .chars()
            .all(|c| c.is_whitespace() || invisible(c))
    {
        return None;
    }
    (display_name_columns(&normalized) <= DISPLAY_NAME_MAX_COLUMNS).then_some(normalized)
}

pub fn account_label(slot: &AccountSlot) -> String {
    match account_display_name(slot.display_name.as_deref()) {
        Some(display) => format!("{display} ({})", slot.name),
        None => slot.name.clone(),
    }
}

pub fn display_name_key(value: &str) -> String {
    const LOOKALIKES: [(char, char); 15] = [
        ('\u{0430}', 'a'),
        ('\u{0432}', 'b'),
        ('\u{0435}', 'e'),
        ('\u{043a}', 'k'),
        ('\u{043c}', 'm'),
        ('\u{043d}', 'h'),
        ('\u{043e}', 'o'),
        ('\u{0440}', 'p'),
        ('\u{0441}', 'c'),
        ('\u{0442}', 't'),
        ('\u{0443}', 'y'),
        ('\u{0445}', 'x'),
        ('\u{0455}', 's'),
        ('\u{0456}', 'i'),
        ('\u{0458}', 'j'),
    ];
    use unicode_normalization::UnicodeNormalization;
    normalize_display_name(value)
        .nfkc()
        .collect::<String>()
        .to_lowercase()
        .chars()
        .filter(|c| !invisible(*c))
        .map(|c| {
            LOOKALIKES
                .iter()
                .find(|(from, _)| *from == c)
                .map_or(c, |(_, to)| *to)
        })
        .collect()
}
