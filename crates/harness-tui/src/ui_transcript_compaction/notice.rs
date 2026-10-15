//! Transcript notices: dim status rows, handoff success blocks and padded `Warning:` /
//! `Error:` rows.
use super::super::ui_transcript_types::TranscriptNoticeSection;
use crate::app::notices::TranscriptNoticeKind;
use crate::theme::Theme;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

const PADDING: &str = " ";

pub(super) fn notice_lines(
    notice: &TranscriptNoticeSection,
    theme: &Theme,
    width: u16,
) -> Vec<Line<'static>> {
    let text = crate::text::strip_ansi_escapes(&notice.text);
    let (style, prefix, vertical_padding) = match notice.kind {
        TranscriptNoticeKind::Status => (
            Style::default().fg(theme.text.tertiary),
            String::new(),
            false,
        ),
        TranscriptNoticeKind::Success => (
            Style::default().fg(theme.text.accent),
            format!("{} ", theme.live_shell.transcript_glyphs.success_marker),
            true,
        ),
        TranscriptNoticeKind::Warning => (
            Style::default().fg(theme.status.warning),
            "Warning: ".to_string(),
            false,
        ),
        TranscriptNoticeKind::Error => (
            Style::default().fg(theme.status.error),
            "Error: ".to_string(),
            false,
        ),
    };
    let inner = usize::from(width).saturating_sub(PADDING.len() * 2).max(1);
    let mut lines = vec![Line::default()];
    if vertical_padding {
        lines.push(Line::default());
    }
    for row in crate::ui::ui_transcript_surface::wrap_surface_spans(
        vec![Span::styled(format!("{prefix}{text}"), style)],
        inner,
    ) {
        let mut spans = vec![Span::raw(PADDING)];
        spans.extend(row);
        lines.push(Line::from(spans));
    }
    if vertical_padding {
        lines.push(Line::default());
    }
    lines
}
