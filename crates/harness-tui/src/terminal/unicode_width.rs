//! Unicode display-width recording for terminal capability evidence.
//!
//! Records the display width of key Unicode categories so the terminal
//! capability manifest rows can verify that the TUI handles CJK, emoji,
//! combining marks, and wide characters correctly.

/// A single Unicode width entry: the codepoint, its display width, and a label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnicodeWidthEntry {
    pub label: &'static str,
    pub codepoint: char,
    pub display_width: u16,
}

impl UnicodeWidthEntry {
    pub const fn narrow(label: &'static str, codepoint: char) -> Self {
        Self {
            label,
            codepoint,
            display_width: 1,
        }
    }

    pub const fn wide(label: &'static str, codepoint: char) -> Self {
        Self {
            label,
            codepoint,
            display_width: 2,
        }
    }
}

/// A recorded set of Unicode width entries for evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnicodeWidthRecord {
    pub entries: Vec<UnicodeWidthEntry>,
}

impl UnicodeWidthRecord {
    /// The canonical set of Unicode width entries covering all categories
    /// the TUI must handle: ASCII, CJK, emoji, combining marks, etc.
    pub fn canonical() -> Self {
        Self {
            entries: vec![
                // ASCII narrow
                UnicodeWidthEntry::narrow("ascii_a", 'a'),
                UnicodeWidthEntry::narrow("ascii_Z", 'Z'),
                UnicodeWidthEntry::narrow("ascii_0", '0'),
                UnicodeWidthEntry::narrow("ascii_space", ' '),
                UnicodeWidthEntry::narrow("ascii_tilde", '~'),
                // Latin extended narrow
                UnicodeWidthEntry::narrow("latin_e_acute", '\u{E9}'),
                UnicodeWidthEntry::narrow("latin_n_tilde", '\u{F1}'),
                // Box-drawing narrow (used by composer border)
                UnicodeWidthEntry::narrow("box_light_horizontal", '\u{2500}'),
                UnicodeWidthEntry::narrow("box_light_vertical", '\u{2502}'),
                UnicodeWidthEntry::narrow("box_light_down_right", '\u{250C}'),
                UnicodeWidthEntry::narrow("box_light_up_left", '\u{2518}'),
                // Composer glyph
                UnicodeWidthEntry::narrow("prompt_glyph", '\u{276F}'), // ❯
                // CJK wide
                UnicodeWidthEntry::wide("cjk_kanji_kawa", '\u{5DDD}'), // 川
                UnicodeWidthEntry::wide("cjk_kanji_yama", '\u{5C71}'), // 山
                UnicodeWidthEntry::wide("cjk_hiragana_a", '\u{3042}'), // あ
                UnicodeWidthEntry::wide("cjk_katakana_a", '\u{30A2}'), // ア
                UnicodeWidthEntry::wide("cjk_hangul_ga", '\u{AC00}'),  // 가
                // Emoji wide
                UnicodeWidthEntry::wide("emoji_check_mark", '\u{2705}'), // ✅
                UnicodeWidthEntry::wide("emoji_cross_mark", '\u{274C}'), // ❌
                UnicodeWidthEntry::wide("emoji_warning", '\u{26A0}'),    // ⚠
                // Fullwidth forms
                UnicodeWidthEntry::wide("fullwidth_a", '\u{FF21}'), // Ａ
                UnicodeWidthEntry::wide("fullwidth_digit", '\u{FF11}'), // １
            ],
        }
    }

    /// All narrow (width=1) entries.
    pub fn narrow_entries(&self) -> Vec<&UnicodeWidthEntry> {
        self.entries
            .iter()
            .filter(|e| e.display_width == 1)
            .collect()
    }

    /// All wide (width=2) entries.
    pub fn wide_entries(&self) -> Vec<&UnicodeWidthEntry> {
        self.entries
            .iter()
            .filter(|e| e.display_width == 2)
            .collect()
    }

    /// Total display width of all entries combined.
    pub fn total_display_width(&self) -> u16 {
        self.entries.iter().map(|e| e.display_width).sum()
    }
}

pub fn char_display_width(c: char) -> u16 {
    u16::try_from(unicode_width::UnicodeWidthChar::width(c).unwrap_or(0)).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_characters_are_narrow() {
        assert_eq!(char_display_width('a'), 1);
        assert_eq!(char_display_width('Z'), 1);
        assert_eq!(char_display_width('0'), 1);
        assert_eq!(char_display_width(' '), 1);
        assert_eq!(char_display_width('~'), 1);
    }

    #[test]
    fn box_drawing_characters_are_narrow() {
        assert_eq!(char_display_width('\u{2500}'), 1); // ─
        assert_eq!(char_display_width('\u{2502}'), 1); // │
        assert_eq!(char_display_width('\u{250C}'), 1); // ╭
        assert_eq!(char_display_width('\u{2518}'), 1); // ╰
    }

    #[test]
    fn cjk_ideographs_are_wide() {
        assert_eq!(char_display_width('\u{5DDD}'), 2); // 川
        assert_eq!(char_display_width('\u{5C71}'), 2); // 山
        assert_eq!(char_display_width('\u{3400}'), 2);
        assert_eq!(char_display_width('\u{F900}'), 2);
    }

    #[test]
    fn hiragana_and_katakana_are_wide() {
        assert_eq!(char_display_width('\u{3042}'), 2); // あ
        assert_eq!(char_display_width('\u{30A2}'), 2); // ア
    }

    #[test]
    fn hangul_syllables_are_wide() {
        assert_eq!(char_display_width('\u{AC00}'), 2); // 가
    }

    #[test]
    fn fullwidth_forms_are_wide() {
        assert_eq!(char_display_width('\u{FF21}'), 2); // Ａ
        assert_eq!(char_display_width('\u{FF11}'), 2); // １
    }

    #[test]
    fn prompt_glyph_is_narrow() {
        assert_eq!(char_display_width('\u{276F}'), 1); // ❯
    }

    #[test]
    fn emoji_and_text_presentation_widths_match_terminal_cells() {
        assert_eq!(char_display_width('\u{2705}'), 2); // ✅
        assert_eq!(char_display_width('\u{274C}'), 2); // ❌
        assert_eq!(char_display_width('\u{26A0}'), 1); // ⚠
    }

    #[test]
    fn combining_marks_are_zero_width() {
        assert_eq!(char_display_width('\u{0301}'), 0); // combining acute accent
        assert_eq!(char_display_width('\u{0308}'), 0); // combining diaeresis
    }

    #[test]
    fn canonical_record_has_mixed_widths() {
        // arrange
        // act
        let record = UnicodeWidthRecord::canonical();

        // assert
        assert!(!record.entries.is_empty());
        assert!(!record.narrow_entries().is_empty());
        assert!(!record.wide_entries().is_empty());
        assert!(
            record.total_display_width() > u16::try_from(record.entries.len()).unwrap_or(u16::MAX)
        );
    }

    #[test]
    fn canonical_record_covers_all_categories() {
        // arrange
        // act
        let record = UnicodeWidthRecord::canonical();
        let labels: Vec<&str> = record.entries.iter().map(|e| e.label).collect();

        // assert
        assert!(labels.contains(&"ascii_a"));
        assert!(labels.contains(&"cjk_kanji_kawa"));
        assert!(labels.contains(&"cjk_hiragana_a"));
        assert!(labels.contains(&"cjk_katakana_a"));
        assert!(labels.contains(&"cjk_hangul_ga"));
        assert!(labels.contains(&"emoji_check_mark"));
        assert!(labels.contains(&"fullwidth_a"));
        assert!(labels.contains(&"box_light_horizontal"));
        assert!(labels.contains(&"prompt_glyph"));
    }
}
