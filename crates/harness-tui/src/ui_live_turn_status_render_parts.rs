use ratatui::{
    style::{Modifier, Style},
    text::Span,
};

use crate::theme::Theme;

use super::super::ui_context_budget::ContextBudgetTone;

pub(super) struct RightStatusInput<'a> {
    pub(super) parts: &'a [String],
    pub(super) background_label: &'static str,
    pub(super) background_visible: bool,
    pub(super) stop_visible: bool,
    pub(super) background_hovered: bool,
    pub(super) stop_hovered: bool,
    pub(super) context_label: Option<&'a str>,
    pub(super) context_tone: Option<ContextBudgetTone>,
}

impl RightStatusInput<'_> {
    pub(super) fn into_spans(self, theme: &Theme) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        for part in self.parts.iter().filter(|part| {
            part.as_str() != super::geometry::STOP_LABEL && part.as_str() != self.background_label
        }) {
            if !spans.is_empty() {
                spans.push(Span::raw(" "));
            }
            let color = if self.context_label == Some(part.as_str()) {
                self.context_tone
                    .map_or(theme.live_turn_timer_color(), |tone| tone.color(theme))
            } else {
                theme.live_turn_timer_color()
            };
            spans.push(Span::styled(
                part.clone(),
                Style::default()
                    .fg(color)
                    .bg(theme.live_turn_background_color())
                    .remove_modifier(Modifier::all()),
            ));
        }
        if self.background_visible {
            if !spans.is_empty() {
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(
                self.background_label,
                Style::default()
                    .fg(if self.background_hovered {
                        theme.status.success
                    } else {
                        theme.live_turn_timer_color()
                    })
                    .bg(theme.live_turn_background_color())
                    .remove_modifier(Modifier::all()),
            ));
        }
        if self.stop_visible {
            if !spans.is_empty() && !self.background_visible {
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(
                super::geometry::STOP_LABEL,
                Style::default()
                    .fg(if self.stop_hovered {
                        theme.status.error
                    } else {
                        theme.live_turn_timer_color()
                    })
                    .bg(theme.live_turn_background_color())
                    .remove_modifier(Modifier::all()),
            ));
        }
        spans
    }
}
