//! Terminal capability leaf: color, keyboard, mouse, and clipboard modes.
//!
//! Mirrors the manifest TERM-CAP-* rows as pure value types. The actual
//! runtime `TerminalCapabilityState` lives in `runtime.rs` (shared root);
//! this leaf provides a testable, dependency-free projection.

/// Color support mode negotiated from `COLORTERM` / `TERM`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorMode {
    /// No color support (dumb terminal).
    None,
    /// 16-color ANSI.
    #[default]
    Ansi16,
    /// 256-color palette.
    Ansi256,
    /// 24-bit truecolor (`COLORTERM=truecolor` or `24bit`).
    Truecolor,
}

impl ColorMode {
    /// Probe from environment variables (pure; no I/O).
    pub fn from_env(colorterm: Option<&str>, term: Option<&str>) -> Self {
        if let Some(ct) = colorterm {
            let lower = ct.to_ascii_lowercase();
            if lower.contains("truecolor") || lower.contains("24bit") {
                return Self::Truecolor;
            }
        }
        if let Some(t) = term {
            let lower = t.to_ascii_lowercase();
            if lower.contains("256color") {
                return Self::Ansi256;
            }
            if lower == "dumb" {
                return Self::None;
            }
        }
        Self::Ansi16
    }

    /// True when truecolor is available.
    pub const fn is_truecolor(self) -> bool {
        matches!(self, Self::Truecolor)
    }

    /// True when at least 256 colors are available.
    pub const fn supports_256(self) -> bool {
        matches!(self, Self::Ansi256 | Self::Truecolor)
    }
}

/// Keyboard enhancement mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyboardMode {
    /// Legacy mode (no CSI u / Kitty keyboard protocol).
    #[default]
    Legacy,
    /// Enhanced mode (CSI u / Kitty keyboard protocol active).
    Enhanced,
}

impl KeyboardMode {
    pub const fn is_enhanced(self) -> bool {
        matches!(self, Self::Enhanced)
    }
}

/// Terminal capability leaf — a pure projection of the runtime capability
/// state for TERM-CAP-* manifest rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalCapabilityLeaf {
    pub color_mode: ColorMode,
    pub keyboard_mode: KeyboardMode,
    pub mouse_capture: bool,
    pub bracketed_paste: bool,
    pub osc52_clipboard: bool,
    pub alternate_screen: bool,
    pub focus_reporting: bool,
}

impl TerminalCapabilityLeaf {
    /// Full-capability fixture (truecolor + enhanced keyboard + mouse + paste + OSC52).
    pub const fn full() -> Self {
        Self {
            color_mode: ColorMode::Truecolor,
            keyboard_mode: KeyboardMode::Enhanced,
            mouse_capture: true,
            bracketed_paste: true,
            osc52_clipboard: true,
            alternate_screen: true,
            focus_reporting: true,
        }
    }

    /// Reduced-capability fixture (legacy terminal: 16-color, no mouse, no paste, no OSC52).
    pub const fn reduced() -> Self {
        Self {
            color_mode: ColorMode::Ansi16,
            keyboard_mode: KeyboardMode::Legacy,
            mouse_capture: false,
            bracketed_paste: false,
            osc52_clipboard: false,
            alternate_screen: false,
            focus_reporting: false,
        }
    }

    /// Probe from environment (pure; no I/O). Matches the runtime's static probes.
    pub fn from_env(colorterm: Option<&str>, term: Option<&str>, is_tty: bool) -> Self {
        Self {
            color_mode: ColorMode::from_env(colorterm, term),
            keyboard_mode: KeyboardMode::Legacy,
            mouse_capture: false,
            bracketed_paste: false,
            osc52_clipboard: is_tty,
            alternate_screen: false,
            focus_reporting: false,
        }
    }

    /// Manifest behavior_id for a given TERM-CAP-* row.
    pub const fn behavior_id(row: TerminalCapabilityRow) -> &'static str {
        match row {
            TerminalCapabilityRow::Color => "TERM-CAP-COLOR",
            TerminalCapabilityRow::Keys => "TERM-CAP-KEYS",
            TerminalCapabilityRow::Mouse => "TERM-CAP-MOUSE",
            TerminalCapabilityRow::Clipboard => "TERM-CAP-CLIPBOARD",
        }
    }
}

impl Default for TerminalCapabilityLeaf {
    fn default() -> Self {
        Self::reduced()
    }
}
/// Manifest terminal capability row identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TerminalCapabilityRow {
    Color,
    Keys,
    Mouse,
    Clipboard,
}

impl TerminalCapabilityRow {
    pub const ALL: [Self; 4] = [Self::Color, Self::Keys, Self::Mouse, Self::Clipboard];
}

/// A recorded terminal capability snapshot for evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalCapabilityRecord {
    pub row: TerminalCapabilityRow,
    pub behavior_id: &'static str,
    pub color_mode: ColorMode,
    pub keyboard_mode: KeyboardMode,
    pub mouse_capture: bool,
    pub bracketed_paste: bool,
    pub osc52_clipboard: bool,
    pub alternate_screen: bool,
    pub focus_reporting: bool,
}

impl TerminalCapabilityRecord {
    pub fn for_row(row: TerminalCapabilityRow, caps: &TerminalCapabilityLeaf) -> Self {
        Self {
            row,
            behavior_id: TerminalCapabilityLeaf::behavior_id(row),
            color_mode: caps.color_mode,
            keyboard_mode: caps.keyboard_mode,
            mouse_capture: caps.mouse_capture,
            bracketed_paste: caps.bracketed_paste,
            osc52_clipboard: caps.osc52_clipboard,
            alternate_screen: caps.alternate_screen,
            focus_reporting: caps.focus_reporting,
        }
    }

    /// All four TERM-CAP-* records for a given capability leaf.
    pub fn all_for(caps: &TerminalCapabilityLeaf) -> Vec<Self> {
        TerminalCapabilityRow::ALL
            .iter()
            .map(|&row| Self::for_row(row, caps))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_mode_probes_truecolor_from_colorterm() {
        assert_eq!(
            ColorMode::from_env(Some("truecolor"), Some("xterm-256color")),
            ColorMode::Truecolor
        );
        assert_eq!(
            ColorMode::from_env(Some("24bit"), Some("xterm-256color")),
            ColorMode::Truecolor
        );
        assert_eq!(
            ColorMode::from_env(Some("TrueColor"), None),
            ColorMode::Truecolor
        );
    }

    #[test]
    fn color_mode_falls_back_to_256_from_term() {
        assert_eq!(
            ColorMode::from_env(None, Some("xterm-256color")),
            ColorMode::Ansi256
        );
    }

    #[test]
    fn color_mode_returns_none_for_dumb() {
        assert_eq!(ColorMode::from_env(None, Some("dumb")), ColorMode::None);
    }

    #[test]
    fn color_mode_defaults_to_ansi16() {
        assert_eq!(ColorMode::from_env(None, None), ColorMode::Ansi16);
        assert_eq!(ColorMode::from_env(None, Some("xterm")), ColorMode::Ansi16);
    }

    #[test]
    fn full_caps_have_all_features_enabled() {
        // arrange
        let caps = TerminalCapabilityLeaf::full();

        // act
        // assert
        assert!(caps.color_mode.is_truecolor());
        assert!(caps.keyboard_mode.is_enhanced());
        assert!(caps.mouse_capture);
        assert!(caps.bracketed_paste);
        assert!(caps.osc52_clipboard);
        assert!(caps.alternate_screen);
        assert!(caps.focus_reporting);
    }

    #[test]
    fn reduced_caps_have_all_features_disabled() {
        // arrange
        let caps = TerminalCapabilityLeaf::reduced();

        // act
        // assert
        assert!(!caps.color_mode.is_truecolor());
        assert!(!caps.keyboard_mode.is_enhanced());
        assert!(!caps.mouse_capture);
        assert!(!caps.bracketed_paste);
        assert!(!caps.osc52_clipboard);
        assert!(!caps.alternate_screen);
        assert!(!caps.focus_reporting);
    }

    #[test]
    fn from_env_probes_truecolor_and_osc52_for_tty() {
        // arrange
        // act
        let caps =
            TerminalCapabilityLeaf::from_env(Some("truecolor"), Some("xterm-256color"), true);

        // assert
        assert!(caps.color_mode.is_truecolor());
        assert!(caps.osc52_clipboard);
        assert!(!caps.mouse_capture);
        assert!(!caps.bracketed_paste);
    }

    #[test]
    fn from_env_disables_osc52_for_non_tty() {
        // arrange
        // act
        let caps =
            TerminalCapabilityLeaf::from_env(Some("truecolor"), Some("xterm-256color"), false);

        // assert
        assert!(!caps.osc52_clipboard);
    }

    #[test]
    fn behavior_ids_match_manifest() {
        assert_eq!(
            TerminalCapabilityLeaf::behavior_id(TerminalCapabilityRow::Color),
            "TERM-CAP-COLOR"
        );
        assert_eq!(
            TerminalCapabilityLeaf::behavior_id(TerminalCapabilityRow::Keys),
            "TERM-CAP-KEYS"
        );
        assert_eq!(
            TerminalCapabilityLeaf::behavior_id(TerminalCapabilityRow::Mouse),
            "TERM-CAP-MOUSE"
        );
        assert_eq!(
            TerminalCapabilityLeaf::behavior_id(TerminalCapabilityRow::Clipboard),
            "TERM-CAP-CLIPBOARD"
        );
    }

    #[test]
    fn records_cover_all_four_rows() {
        // arrange
        let caps = TerminalCapabilityLeaf::full();

        // act
        let records = TerminalCapabilityRecord::all_for(&caps);

        // assert
        assert_eq!(records.len(), 4);
        assert_eq!(records[0].behavior_id, "TERM-CAP-COLOR");
        assert_eq!(records[1].behavior_id, "TERM-CAP-KEYS");
        assert_eq!(records[2].behavior_id, "TERM-CAP-MOUSE");
        assert_eq!(records[3].behavior_id, "TERM-CAP-CLIPBOARD");
        assert!(records.iter().all(|r| r.color_mode.is_truecolor()));
    }
}
