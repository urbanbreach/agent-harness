//! Render-safe normalization and atomic data-URL redaction.
use super::{fonts, silver};
mod data_urls;
pub use data_urls::elide_data_urls;
use regex::Regex;
use std::sync::LazyLock;
use unicode_normalization::UnicodeNormalization;

static FORMAT: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| Regex::new(r"^\p{Cf}$"));
static MARK: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| Regex::new(r"^\p{M}$"));
static DROP: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"^[\p{Extended_Pictographic}\p{Cc}\p{Mn}\p{Me}]$"));

/// A combining mark the selected font or Silver can draw survives the drop pass: coverage
/// is tested before the DROP properties apply.
fn supported_mark(ch: char, font: &str) -> bool {
    property(&MARK, ch) && (fonts::covers(font, ch) || silver::covers(ch))
}

fn property(regex: &Result<Regex, regex::Error>, ch: char) -> bool {
    let mut bytes = [0; 4];
    regex
        .as_ref()
        .is_ok_and(|r| r.is_match(ch.encode_utf8(&mut bytes)))
}

fn fold(ch: char) -> Option<&'static str> {
    Some(match ch {
        '\u{2018}'..='\u{201b}' | '\u{2032}' | '\u{2035}' => "'",
        '\u{201c}'..='\u{201e}' | '\u{2033}' | '\u{2036}' => "\"",
        '‹' => "<",
        '›' => ">",
        '\u{2010}'..='\u{2015}' | '−' | '⁃' => "-",
        '⁄' => "/",
        '․' => ".",
        '‥' => "..",
        '…' | '⋯' => "...",
        '•' | '‣' | '∙' | '●' | '■' | '▪' => "*",
        '←' => "<-",
        '↑' => "^",
        '→' => "->",
        '↓' => "v",
        '↔' => "<->",
        '⇐' => "<=",
        '⇒' => "=>",
        '⇔' => "<=>",
        '✓' | '✔' => "v",
        '✗' | '✘' => "x",
        _ => return None,
    })
}
fn status(ch: char) -> Option<&'static str> {
    Some(match ch {
        '✅' | '☑' | '✔' => "[OK]",
        '❌' | '❎' | '✖' => "[FAIL]",
        '⚠' => "[WARN]",
        '🚨' => "[ALERT]",
        'ℹ' => "[INFO]",
        '🐛' => "[BUG]",
        '💥' => "[CRASH]",
        '🔥' => "[HOT]",
        '🔒' => "[LOCK]",
        '🔓' => "[UNLOCK]",
        '📁' | '📂' => "[DIR]",
        '📄' => "[FILE]",
        '📝' => "[NOTE]",
        '🧪' => "[TEST]",
        '⏳' | '⌛' => "[WAIT]",
        '🚀' => "[RUN]",
        _ => return None,
    })
}
fn latin(ch: char) -> bool {
    matches!(ch, ' '..='~' | '\u{a0}'..='\u{ff}')
}
fn whitespace(ch: char) -> bool {
    matches!(ch, '\t'..='\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}

// Incomplete escapes remain text; complete terminal controls never become glyphs.
fn ansi_end(bytes: &[u8], start: usize) -> Option<usize> {
    let next = *bytes.get(start + 1)?;
    if next == b'[' {
        let mut index = start + 2;
        while bytes.get(index).is_some_and(|b| (0x30..=0x3f).contains(b)) {
            index += 1;
        }
        while bytes.get(index).is_some_and(|b| (0x20..=0x2f).contains(b)) {
            index += 1;
        }
        if bytes.get(index).is_some_and(|b| (0x40..=0x7e).contains(b)) {
            return Some(index + 1);
        }
    } else if matches!(next, b']' | b'P' | b'X' | b'^' | b'_') {
        let mut index = start + 2;
        while index < bytes.len() {
            if next == b']' && bytes[index] == 7 {
                return Some(index + 1);
            }
            if bytes[index] == 27 && bytes.get(index + 1) == Some(&b'\\') {
                return Some(index + 2);
            }
            index += 1;
        }
    } else {
        let mut index = start + 1;
        while bytes.get(index).is_some_and(|b| (0x20..=0x2f).contains(b)) {
            index += 1;
        }
        if bytes.get(index).is_some_and(|b| (0x30..=0x7e).contains(b)) {
            return Some(index + 1);
        }
    }
    None
}

fn collapse_whitespace(text: &str) -> String {
    let mut collapsed = String::with_capacity(text.len());
    let mut index = 0;
    let mut pending = false;
    let mut space = false;
    let mut line = false;
    while index < text.len() {
        if text.as_bytes()[index] == 27
            && let Some(end) = ansi_end(text.as_bytes(), index)
        {
            index = end;
            continue;
        }
        let Some(ch) = text[index..].chars().next() else {
            break;
        };
        index += ch.len_utf8();
        let format = property(&FORMAT, ch);
        if whitespace(ch) || format {
            pending = true;
            space |= !format;
            line |= matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}');
            continue;
        }
        if pending {
            if line {
                collapsed.push('█');
            } else if space {
                collapsed.push(' ');
            }
            pending = false;
            space = false;
            line = false;
        }
        collapsed.push(ch);
    }
    collapsed
}

/// Coverage of graphic inputs after whitespace collapse and semantic folding.
/// Spaces count as graphics; dim controls, line separators and dropped emoji do not.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FontCoverage {
    pub fallback_count: usize,
    pub total_graphics: usize,
}

impl FontCoverage {
    /// Fraction of graphic inputs replaced by an unsupported-glyph placeholder.
    #[expect(
        clippy::cast_precision_loss,
        reason = "The ratio is only used to display a rounded warning percentage"
    )]
    pub fn unsupported_ratio(self) -> f64 {
        if self.total_graphics == 0 {
            0.0
        } else {
            self.fallback_count as f64 / self.total_graphics as f64
        }
    }

    /// The unsupported ratio ceiling is five percent, inclusive.
    pub fn is_safe(self) -> bool {
        self.fallback_count.saturating_mul(20) <= self.total_graphics
    }
}

/// Scan raw source using the same folds and font coverage as normalization. A character
/// the selected font lacks and compatibility folding cannot replace is supported when
/// Silver can draw it.
pub fn font_coverage(text: &str, font: &str) -> FontCoverage {
    visit_normalized(text, font, |_| {})
}

fn visit_normalized(text: &str, font: &str, mut emit: impl FnMut(char)) -> FontCoverage {
    let collapsed = collapse_whitespace(text);
    let mut coverage = FontCoverage::default();
    for ch in collapsed.trim_matches([' ', '█']).chars() {
        if matches!(ch, '\u{e}' | '\u{f}' | '█') {
            emit(ch);
            continue;
        }
        if latin(ch) {
            emit(ch);
        } else if let Some(label) = status(ch) {
            label.chars().for_each(&mut emit);
        } else if let Some(punctuation) = fold(ch) {
            punctuation.chars().for_each(&mut emit);
        } else if matches!(ch, '\u{2500}'..='\u{257f}') {
            emit(match ch {
                '│' | '┃' => '|',
                '─' | '━' => '-',
                _ => '+',
            });
        } else if property(&DROP, ch) && !supported_mark(ch, font) {
            continue;
        } else if fonts::covers(font, ch) {
            emit(ch);
        } else {
            coverage.fallback_count += usize::from(normalize_fallback(ch, &mut emit));
        }
        coverage.total_graphics += 1;
    }
    coverage
}

/// Normalize whitespace, punctuation and glyphs for the selected bundled font.
pub fn normalize_text(text: &str, font: &str) -> String {
    let mut output = String::with_capacity(text.len());
    visit_normalized(text, font, |ch| output.push(ch));
    let mut result = String::with_capacity(output.len());
    for ch in output.trim_matches([' ', '█']).chars() {
        if ch != ' ' || !result.ends_with(' ') {
            result.push(ch);
        }
    }
    result
}

fn normalize_fallback(ch: char, emit: &mut impl FnMut(char)) -> bool {
    let mut buffer = [0; 4];
    let original: &str = ch.encode_utf8(&mut buffer);
    let decomposition = original.nfkd().filter(|c| !property(&MARK, *c));
    if decomposition.clone().eq(original.chars())
        || !decomposition.clone().all(|c| latin(c) || fold(c).is_some())
    {
        // The selected font and compatibility folding come first; Silver draws the rest.
        if silver::covers(ch) {
            emit(ch);
            return false;
        }
        emit('?');
        return true;
    }
    for scalar in decomposition {
        if let Some(punctuation) = fold(scalar) {
            punctuation.chars().for_each(&mut *emit);
        } else {
            emit(scalar);
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_counts_graphic_inputs_not_expansion_lengths() {
        for (input, font, fallback_count, total_graphics) in [
            ("", "5x8", 0, 0),
            (" \u{200b}\n😀\u{e}\u{f} ", "5x8", 0, 0),
            // Silver draws the combining mark, so it survives the drop pass.
            ("😀\u{301}", "5x8", 0, 1),
            ("\u{1b}[31ma\u{1b}[0m\t\t\u{10ffff}", "5x8", 1, 3),
            ("a\r\n\u{10ffff}", "5x8", 1, 2),
            ("✅→ﬁＡ─\u{10ffff}", "5x8", 1, 6),
            ("?é½", "5x8", 0, 3),
            ("Ω", "5x8", 0, 1),
            ("Ω", "6x12", 0, 1),
            ("Ω", "8x13", 0, 1),
            ("A\u{10ffff}", "8x8", 1, 2),
            (
                "\u{e000}\u{e000}\u{e000}\u{e000}\u{e000}\u{e000}\u{e000}\u{e000}\u{e000}\u{e000}",
                "8x13",
                10,
                10,
            ),
            ("const a = '你好世界';", "8x13", 0, 17),
            ("你好", "5x8", 0, 2),
            ("你好", "silver", 0, 2),
            ("\u{e000}", "silver", 1, 1),
            ("A\u{e000}", "silver", 1, 2),
        ] {
            assert_eq!(
                font_coverage(input, font),
                FontCoverage {
                    fallback_count,
                    total_graphics,
                },
                "{input:?} with {font}",
            );
        }
    }

    #[test]
    fn supported_combining_marks_survive_the_drop_pass() {
        // U+0301 is in the 8x13 table but not in the 5x8 table.
        assert_eq!(normalize_text("e\u{301}", "8x13"), "e\u{301}");
        assert_eq!(normalize_text("e\u{301}", "5x8"), "e");
    }

    #[test]
    fn coverage_rejects_above_five_percent() {
        for (supported, expected_safe) in [(20, true), (19, true), (18, false)] {
            let text = format!("{}\u{10ffff}", "a".repeat(supported));
            let coverage = font_coverage(&text, "5x8");
            assert_eq!(coverage.is_safe(), expected_safe);
        }
        let empty = font_coverage("😀", "5x8");
        assert!(empty.is_safe());
        assert_eq!(empty.unsupported_ratio(), 0.0);
    }

    #[test]
    fn protects_data_url_atoms_and_heals_cut_archives() {
        for (input, strict, expected) in [
            (
                "![a[b](data:Image/PNG;x=y;base64,QUFB)",
                false,
                "[data URL omitted: Image/PNG;x=y, 4 base64 chars]",
            ),
            (
                "data:image/png;base64,abc",
                false,
                "data:image/png;base64,abc",
            ),
            (
                "data:image/png;base64,a",
                true,
                "[data URL omitted: image/png, 1 base64 chars]",
            ),
            (
                "data:image/png;base64,QUFB [...900ch elided...] QUFB",
                true,
                "[data URL omitted: image/png, 908 base64 chars]",
            ),
            (
                "[a](data:image/png;base64,QUFB",
                false,
                "[a]([data URL omitted: image/png, 4 base64 chars]",
            ),
            (
                "data:image/png;base64,QUFB\u{a0}[...900ch elided...]\u{2003}QUFB",
                true,
                "[data URL omitted: image/png, 908 base64 chars]",
            ),
            (
                "[a](data:image/png;base64,QUFB\u{2003})",
                false,
                "[data URL omitted: image/png, 4 base64 chars]",
            ),
        ] {
            assert_eq!(elide_data_urls(input, strict), expected);
        }
    }
    #[test]
    fn normalizes_terminal_whitespace_semantics_and_compatibility() {
        for (input, expected) in [
            (" \u{1b}[31mred\u{1b}[0m\r\n\u{200b}\tblue ", "red█blue"),
            ("a\u{200b}b\u{feff}c", "abc"),
            ("✅ ⚠ 🚀 😀", "[OK] [WARN] [RUN]"),
            ("‘x’ → ─│┼ …", "'x' -> -|+ ..."),
            ("é ½ ﬁ Ａ e\u{301}", "é ½ fi A e"),
            ("\u{e}dim\u{f}\nline", "\u{e}dim\u{f}█line"),
            ("\u{10ffff}", "?"),
        ] {
            assert_eq!(normalize_text(input, "5x8"), expected);
        }
    }
}
