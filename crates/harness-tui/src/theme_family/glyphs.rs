use crate::theme::{StatusGlyphs, Theme, TranscriptGlyphs};

use super::roles::GlyphRole;

impl GlyphRole {
    pub const LABELS: [&str; 13] = [
        "streaming",
        "done",
        "error",
        "pending_permission",
        "queued",
        "running",
        "succeeded",
        "failed",
        "user_marker",
        "tool_marker",
        "card_top",
        "card_mid",
        "card_bottom",
    ];

    pub const fn label(self) -> &'static str {
        Self::LABELS[self.index()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlyphPalette {
    pub values: [&'static str; 13],
}

impl GlyphPalette {
    pub const fn from_theme(theme: &Theme) -> Self {
        let status: StatusGlyphs = theme.live_shell.glyphs;
        let transcript: TranscriptGlyphs = theme.live_shell.transcript_glyphs;
        Self {
            values: [
                status.streaming,
                status.done,
                status.error,
                status.pending_permission,
                status.queued,
                status.running,
                status.succeeded,
                status.failed,
                transcript.user_marker,
                transcript.tool_marker,
                transcript.card_top,
                transcript.card_mid,
                transcript.card_bottom,
            ],
        }
    }

    pub const fn glyph(self, role: GlyphRole) -> &'static str {
        self.values[role.index()]
    }
}
