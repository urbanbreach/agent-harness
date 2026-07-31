//! Clipboard leaf types for the TUI responsive/terminal shard.
//!
//! These are plain value objects with no shared registry or app-state
//! dependency. They capture the clipboard integration mode (OSC52,
//! native fallback), paste behavior, and hyperlink (OSC8) formatting
//! that the TERM-CAP-CLIPBOARD manifest row requires.

/// Clipboard integration mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClipboardMode {
    /// No clipboard integration available.
    #[default]
    None,
    /// OSC 52 escape sequence (terminal-native clipboard).
    Osc52,
    /// Native clipboard (pbcopy/wl-copy/xclip/powershell).
    Native,
    /// OSC 52 with native fallback.
    Osc52WithNativeFallback,
}

impl ClipboardMode {
    pub const fn is_available(self) -> bool {
        !matches!(self, Self::None)
    }

    pub const fn supports_osc52(self) -> bool {
        matches!(self, Self::Osc52 | Self::Osc52WithNativeFallback)
    }

    pub const fn supports_native(self) -> bool {
        matches!(self, Self::Native | Self::Osc52WithNativeFallback)
    }
}

/// Paste behavior mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PasteMode {
    /// No bracketed paste support.
    #[default]
    Disabled,
    /// Bracketed paste is active.
    Bracketed,
}

/// Clipboard leaf — a pure value type for clipboard/paste/hyperlink state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardLeaf {
    pub mode: ClipboardMode,
    pub paste_mode: PasteMode,
    /// Copy-on-select is enabled.
    pub copy_on_select: bool,
    /// OSC 8 hyperlinks are supported.
    pub hyperlink_support: bool,
}

impl ClipboardLeaf {
    /// Full clipboard support (OSC52 + native fallback + bracketed paste + copy-on-select + hyperlinks).
    pub const fn full() -> Self {
        Self {
            mode: ClipboardMode::Osc52WithNativeFallback,
            paste_mode: PasteMode::Bracketed,
            copy_on_select: true,
            hyperlink_support: true,
        }
    }

    /// No clipboard support (legacy terminal).
    pub const fn disabled() -> Self {
        Self {
            mode: ClipboardMode::None,
            paste_mode: PasteMode::Disabled,
            copy_on_select: false,
            hyperlink_support: false,
        }
    }

    /// OSC52 only (no native fallback).
    pub const fn osc52_only() -> Self {
        Self {
            mode: ClipboardMode::Osc52,
            paste_mode: PasteMode::Disabled,
            copy_on_select: true,
            hyperlink_support: true,
        }
    }

    /// Native only (no OSC52).
    pub const fn native_only() -> Self {
        Self {
            mode: ClipboardMode::Native,
            paste_mode: PasteMode::Disabled,
            copy_on_select: true,
            hyperlink_support: false,
        }
    }
}

impl Default for ClipboardLeaf {
    fn default() -> Self {
        Self::disabled()
    }
}

/// Format an OSC 8 hyperlink sequence (open + label + close).
///
/// Terminals that ignore OSC 8 still render `label` as plain text.
/// Empty `uri` or `label` yields plain label text without escape sequences.
pub fn format_osc8_hyperlink(uri: &str, label: &str) -> String {
    if uri.is_empty() || label.is_empty() {
        return label.to_string();
    }
    format!("\x1b]8;;{uri}\x1b\\{label}\x1b]8;;\x1b\\")
}

/// Encode bytes as base64 (for OSC 52 clipboard sequences).
pub fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = *chunk.get(1).unwrap_or(&0);
        let third = *chunk.get(2).unwrap_or(&0);
        let bits = u32::from(first) << 16 | u32::from(second) << 8 | u32::from(third);

        encoded.push(TABLE[((bits >> 18) & 0x3F) as usize] as char);
        encoded.push(TABLE[((bits >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            encoded.push(TABLE[((bits >> 6) & 0x3F) as usize] as char);
        } else {
            encoded.push('=');
        }
        if chunk.len() > 2 {
            encoded.push(TABLE[(bits & 0x3F) as usize] as char);
        } else {
            encoded.push('=');
        }
    }

    encoded
}

/// Build an OSC 52 clipboard copy sequence.
///
/// Wraps the base64-encoded text in the OSC 52 escape sequence, with
/// optional tmux/screen passthrough wrapping.
pub fn build_osc52_sequence(text: &str, in_tmux: bool) -> String {
    let base64 = encode_base64(text.as_bytes());
    let sequence = format!("\x1b]52;c;{base64}\x07");
    if in_tmux {
        format!("\x1bPtmux;\x1b{sequence}\x1b\\")
    } else {
        sequence
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_clipboard_has_all_features() {
        // arrange
        // act
        let leaf = ClipboardLeaf::full();

        // assert
        assert!(leaf.mode.is_available());
        assert!(leaf.mode.supports_osc52());
        assert!(leaf.mode.supports_native());
        assert!(leaf.copy_on_select);
        assert!(leaf.hyperlink_support);
        assert_eq!(leaf.paste_mode, PasteMode::Bracketed);
    }

    #[test]
    fn disabled_clipboard_has_no_features() {
        // arrange
        // act
        let leaf = ClipboardLeaf::disabled();

        // assert
        assert!(!leaf.mode.is_available());
        assert!(!leaf.copy_on_select);
        assert!(!leaf.hyperlink_support);
        assert_eq!(leaf.paste_mode, PasteMode::Disabled);
    }

    #[test]
    fn osc52_only_has_no_native_fallback() {
        // arrange
        // act
        let leaf = ClipboardLeaf::osc52_only();

        // assert
        assert!(leaf.mode.supports_osc52());
        assert!(!leaf.mode.supports_native());
    }

    #[test]
    fn native_only_has_no_osc52() {
        // arrange
        // act
        let leaf = ClipboardLeaf::native_only();

        // assert
        assert!(!leaf.mode.supports_osc52());
        assert!(leaf.mode.supports_native());
        assert!(!leaf.hyperlink_support);
    }

    #[test]
    fn osc8_hyperlink_wraps_label_and_falls_back_on_empty() {
        // arrange
        // act
        // assert
        let linked = format_osc8_hyperlink("https://example.com/path", "path");
        assert!(linked.contains("https://example.com/path"));
        assert!(linked.contains("path"));
        assert!(linked.starts_with("\x1b]8;;"));
        assert!(linked.ends_with("\x1b]8;;\x1b\\"));

        assert_eq!(format_osc8_hyperlink("", "plain"), "plain");
        assert_eq!(format_osc8_hyperlink("https://x", ""), "");
    }

    #[test]
    fn base64_encoding_matches_standard() {
        // arrange
        // act
        // assert
        assert_eq!(encode_base64(b""), "");
        assert_eq!(encode_base64(b"f"), "Zg==");
        assert_eq!(encode_base64(b"fo"), "Zm8=");
        assert_eq!(encode_base64(b"foo"), "Zm9v");
        assert_eq!(encode_base64(b"hello"), "aGVsbG8=");
    }

    #[test]
    fn osc52_sequence_wraps_base64_in_escape() {
        // arrange
        // act
        let seq = build_osc52_sequence("test", false);

        // assert
        assert!(seq.starts_with("\x1b]52;c;"));
        assert!(seq.ends_with("\x07"));
        assert!(seq.contains("dGVzdA==")); // base64("test")
    }

    #[test]
    fn osc52_sequence_wraps_tmux_passthrough() {
        // arrange
        // act
        let seq = build_osc52_sequence("test", true);

        // assert
        assert!(seq.starts_with("\x1bPtmux;\x1b"));
        assert!(seq.ends_with("\x1b\\"));
    }

    #[test]
    fn osc52_sequence_without_tmux_has_no_passthrough() {
        // arrange
        // act
        let seq = build_osc52_sequence("test", false);

        // assert
        assert!(!seq.contains("Ptmux"));
    }
}
