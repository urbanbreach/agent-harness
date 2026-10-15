//! Codepoint classification for the Claude count reconstruction.
//!
//! Ported from the public ctok project (MIT); see `LICENSES.md`. Every rule is a measured fact
//! about the provider's count endpoint, not a Unicode-category rule, unless noted.

mod tables;

use unicode_general_category::{get_general_category, GeneralCategory as Gc};
use unicode_normalization::char::canonical_combining_class;

use tables::{
    in_ranges, PUNCT_SYMS, SEPARATOR_ANNOTATIONS, SEPARATOR_MARKS, SEPARATOR_SIGNS, SYMBOL_LETTERS,
};

/// Structural markers written into the marked stream: one noncharacter each.
pub(super) const BOW: char = '\u{FDD0}';
pub(super) const EOW: char = '\u{FDD1}';
pub(super) const SHIFT: char = '\u{FDD3}';
pub(super) const CAPS: char = '\u{FDD4}';

/// Input noncharacters equal to a marker are swapped for private-use codepoints (which input
/// normalization has already removed) so they are priced as the bytes they are.
const MARKER_LITERALS: [char; 5] = ['\u{FDD0}', '\u{FDD1}', '\u{FDD2}', '\u{FDD3}', '\u{FDD4}'];
const MARKER_ESCAPES: [char; 5] = ['\u{E000}', '\u{E001}', '\u{E002}', '\u{E003}', '\u{E004}'];

pub(super) fn escape_marker(ch: char) -> char {
    MARKER_LITERALS
        .iter()
        .position(|&m| m == ch)
        .map_or(ch, |i| MARKER_ESCAPES[i])
}

pub(super) fn unescape_marker(ch: char) -> char {
    MARKER_ESCAPES
        .iter()
        .position(|&m| m == ch)
        .map_or(ch, |i| MARKER_LITERALS[i])
}

pub(super) fn is_marker(ch: char) -> bool {
    matches!(ch, BOW | EOW | SHIFT | CAPS)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Class {
    Wordy,
    Hard,
    Digit,
    Punct,
    Space,
    /// A combining mark with no letter to attach to; only produced by run splitting.
    Stray,
}

pub(super) fn category(ch: char) -> Gc {
    get_general_category(ch)
}

pub(super) const fn is_letter(gc: Gc) -> bool {
    matches!(
        gc,
        Gc::UppercaseLetter
            | Gc::LowercaseLetter
            | Gc::TitlecaseLetter
            | Gc::ModifierLetter
            | Gc::OtherLetter
    )
}

pub(super) const fn is_mark(gc: Gc) -> bool {
    matches!(gc, Gc::NonspacingMark | Gc::SpacingMark | Gc::EnclosingMark)
}

const fn is_punct_or_symbol(gc: Gc) -> bool {
    matches!(
        gc,
        Gc::ConnectorPunctuation
            | Gc::DashPunctuation
            | Gc::OpenPunctuation
            | Gc::ClosePunctuation
            | Gc::InitialPunctuation
            | Gc::FinalPunctuation
            | Gc::OtherPunctuation
            | Gc::MathSymbol
            | Gc::CurrencySymbol
            | Gc::ModifierSymbol
            | Gc::OtherSymbol
    )
}

const fn is_separator_space(gc: Gc) -> bool {
    matches!(
        gc,
        Gc::SpaceSeparator | Gc::LineSeparator | Gc::ParagraphSeparator
    )
}

/// Codepoints that use the isolated-character path for letters.
fn is_hard_cp(cp: u32) -> bool {
    cp >= 0x1_0000
        || (0x4E00..=0x9FFF).contains(&cp)
        || (0x3400..=0x4DBF).contains(&cp)
        || (0xF900..=0xFAFF).contains(&cp)
        || (0xAC00..=0xD7A3).contains(&cp)
        || matches!(cp, 0x3005 | 0x3006)
        || (0x06DD..=0x06E0).contains(&cp)
        || (0x06E9..=0x06EC).contains(&cp)
}

/// Two cased letters newer than the Unicode tables ctok was measured with; the models know them.
const NEW_CASE_PAIRS: [(char, char); 2] = [('\u{1C89}', '\u{1C8A}'), ('\u{A7CB}', '\u{0264}')];

pub(super) fn is_new_cased(ch: char) -> bool {
    NEW_CASE_PAIRS
        .iter()
        .any(|&(upper, lower)| ch == upper || ch == lower)
}

pub(super) fn is_upper(ch: char) -> bool {
    NEW_CASE_PAIRS.iter().any(|&(upper, _)| ch == upper) || ch.is_uppercase()
}

pub(super) fn is_lower(ch: char) -> bool {
    NEW_CASE_PAIRS.iter().any(|&(_, lower)| ch == lower) || ch.is_lowercase()
}

/// Per-character lowercase (no final-sigma context), with the newer pairs mapped explicitly.
pub(super) fn lower_into(ch: char, out: &mut String) {
    match NEW_CASE_PAIRS.iter().find(|&&(upper, _)| upper == ch) {
        Some(&(_, lower)) => out.push(lower),
        None => out.extend(ch.to_lowercase()),
    }
}

pub(super) fn lower_char(ch: char) -> String {
    let mut out = String::new();
    lower_into(ch, &mut out);
    out
}

fn is_variation_selector(ch: char) -> bool {
    ('\u{FE00}'..='\u{FE0F}').contains(&ch)
}

pub(super) fn is_selector(ch: char) -> bool {
    is_variation_selector(ch)
}

/// A mark that terminates the orthographic syllable and so separates word runs: viramas
/// (combining class 9 except Thai phinthu), the measured accent and annotation ranges, and the
/// enumerated signs.
pub(super) fn is_separator(ch: char) -> bool {
    if u32::from(ch) < 0x0300 {
        return false;
    }
    (canonical_combining_class(ch) == 9 && ch != '\u{0E3A}')
        || in_ranges(SEPARATOR_SIGNS, ch)
        || in_ranges(SEPARATOR_MARKS, ch)
        || in_ranges(SEPARATOR_ANNOTATIONS, ch)
}

/// A combining mark where nothing before it can be its base.
pub(super) fn is_stray_mark(ch: char) -> bool {
    let cp = u32::from(ch);
    if cp == 0x0711 || (0x0730..=0x073F).contains(&cp) {
        return false; // a baseless Syriac vowel is a word-forming letter
    }
    canonical_combining_class(ch) != 0 && !is_separator(ch)
}

pub(super) fn classify(ch: char) -> Class {
    if is_separator(ch) {
        return Class::Hard;
    }
    if is_new_cased(ch) {
        return Class::Wordy;
    }
    let cp = u32::from(ch);
    let gc = category(ch);
    if is_separator_space(gc) || matches!(ch, '\t' | '\n' | '\r' | '\u{0C}' | '\u{0B}') {
        return Class::Space;
    }
    if in_ranges(SYMBOL_LETTERS, ch) {
        return Class::Wordy;
    }
    if gc == Gc::DecimalNumber
        && (cp < 0x80 || (0x0660..=0x0669).contains(&cp) || (0x06F0..=0x06F9).contains(&cp))
    {
        return Class::Digit;
    }
    if cp < 0x80 && is_punct_or_symbol(gc) {
        return Class::Punct;
    }
    if in_ranges(PUNCT_SYMS, ch) {
        return Class::Punct;
    }
    if is_variation_selector(ch) {
        return Class::Hard;
    }
    if cp == 0x0CF3 {
        return Class::Wordy;
    }
    if (is_letter(gc) || is_mark(gc)) && !is_hard_cp(cp) {
        return Class::Wordy;
    }
    Class::Hard
}

/// Whether the border-marker branch can claim this character: punctuation, symbols, format
/// characters and separator marks, minus ideographic punctuation and astral characters.
pub(super) fn marks_like_punct(ch: char) -> bool {
    let ch = unescape_marker(ch);
    if u32::from(ch) >= 0x1_0000 {
        return false;
    }
    if is_separator(ch) {
        return true;
    }
    let gc = category(ch);
    let ideographic_punct = (0x3001..=0x303F).contains(&u32::from(ch))
        && matches!(
            gc,
            Gc::ConnectorPunctuation
                | Gc::DashPunctuation
                | Gc::OpenPunctuation
                | Gc::ClosePunctuation
                | Gc::InitialPunctuation
                | Gc::FinalPunctuation
                | Gc::OtherPunctuation
        );
    (is_punct_or_symbol(gc) || matches!(gc, Gc::Format | Gc::Unassigned)) && !ideographic_punct
}

/// Whether a digit receives a border marker: non-ASCII decimal digits and other numbers, BMP only.
pub(super) fn digit_border(ch: char) -> bool {
    let gc = category(ch);
    (gc == Gc::OtherNumber || (gc == Gc::DecimalNumber && !ch.is_ascii()))
        && u32::from(ch) < 0x1_0000
}

/// A uniform run of decimal digits or of other numbers.
pub(super) fn is_digit_run(body: &[char]) -> bool {
    let Some(&first) = body.first() else {
        return false;
    };
    let gc = category(first);
    matches!(gc, Gc::DecimalNumber | Gc::OtherNumber) && body.iter().all(|&c| category(c) == gc)
}

/// ASCII whitespace the v5 frame absorbs at the end of content.
pub(super) const fn is_ascii_frame_space(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{0C}' | '\u{0B}')
}

/// Space separators the tokenizer treats as U+0020.
pub(super) fn is_funny_space(ch: char) -> bool {
    matches!(
        ch,
        '\u{00A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}'
    )
}

/// Characters the endpoint strips before tokenizing: C0/C1 controls except TAB, LF and NUL.
pub(super) fn is_stripped_control(ch: char) -> bool {
    matches!(ch, '\u{01}'..='\u{08}' | '\u{0B}'..='\u{1F}' | '\u{7F}'..='\u{9F}')
}

/// BMP private use is stripped the same way.
pub(super) fn is_private_use(ch: char) -> bool {
    ('\u{E000}'..='\u{F8FF}').contains(&ch)
}

/// Contraction suffixes an apostrophe binds into the word ahead of it (lowercase, whole word).
pub(super) fn is_contraction_suffix(body: &[char]) -> bool {
    matches!(
        body,
        ['s'] | ['t'] | ['d'] | ['m'] | ['l', 'l'] | ['r', 'e'] | ['v', 'e']
    )
}
