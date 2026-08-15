use ratatui::{style::Style, text::Span};

use crate::theme::Theme;

pub(super) struct RightStatusInput<'a> {
    pub(super) parts: &'a [String],
    pub(super) background_label: &'static str,
    pub(super) background_visible: bool,
    pub(super) stop_visible: bool,
    pub(super) background_hovered: bool,
    pub(super) stop_hovered: bool,
}

impl RightStatusInput<'_> {
    pub(super) fn into_spans(self, theme: &Theme) -> Vec<Span<'static>> {
        let metadata = self
            .parts
            .iter()
            .filter(|part| {
                part.as_str() != super::geometry::STOP_LABEL
                    && part.as_str() != self.background_label
            })
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        let mut spans = Vec::new();
        if !metadata.is_empty() {
            spans.push(Span::styled(
                metadata,
                Style::default().fg(theme.text.secondary),
            ));
        }
        if self.background_visible {
            if !spans.is_empty() {
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(
                self.background_label,
                Style::default().fg(if self.background_hovered {
                    theme.status.success
                } else {
                    theme.text.secondary
                }),
            ));
        }
        if self.stop_visible {
            if !spans.is_empty() {
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(
                super::geometry::STOP_LABEL,
                Style::default().fg(if self.stop_hovered {
                    theme.status.error
                } else {
                    theme.text.secondary
                }),
            ));
        }
        spans
    }
}
