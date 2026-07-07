use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use super::common::{
    horizontal_inset, render_panel, render_prompt_header, split_hint, wrap_text, PANEL_WIDTH,
    PROMPT_HEADER_HEIGHT,
};
use crate::theme::Theme;

pub(super) struct PromptPanel<'a> {
    pub(super) title: &'a str,
    pub(super) description: Option<&'a str>,
    pub(super) placeholder: &'a str,
    pub(super) value: &'a str,
    pub(super) secret: bool,
    pub(super) error: Option<&'a str>,
    pub(super) footer: &'a str,
}

impl PromptPanel<'_> {
    pub(super) fn render(self, frame: &mut Frame, theme: &Theme, root: Rect) {
        let panel_width = PANEL_WIDTH.min(root.width.saturating_sub(2)).max(1);
        let content_width = panel_width.saturating_sub(4).max(1);
        let description_lines = self
            .description
            .map(|text| wrap_text(text, content_width))
            .unwrap_or_default();
        let error_lines = self
            .error
            .map(|text| wrap_text(text, content_width))
            .unwrap_or_default();
        let area = render_panel(
            frame,
            theme,
            root,
            PROMPT_HEADER_HEIGHT
                + u16::try_from(description_lines.len()).unwrap_or(u16::MAX)
                + 4
                + u16::try_from(error_lines.len()).unwrap_or(u16::MAX)
                + 2,
        );
        render_prompt_header(frame, theme, area, self.title);

        let mut y = area.y + PROMPT_HEADER_HEIGHT;
        for description in description_lines {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    description,
                    Style::default().fg(theme.text.tertiary),
                ))),
                horizontal_inset(Rect::new(area.x, y, area.width, 1), 2),
            );
            y += 1;
        }

        self.render_input(frame, theme, area, &mut y);

        for error in error_lines {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    error,
                    Style::default().fg(theme.status.error),
                ))),
                horizontal_inset(Rect::new(area.x, y, area.width, 1), 2),
            );
            y += 1;
        }

        frame.render_widget(
            Paragraph::new(Line::from(split_hint(self.footer, theme))),
            horizontal_inset(Rect::new(area.x, y, area.width, 1), 2),
        );
    }

    fn render_input(&self, frame: &mut Frame, theme: &Theme, area: Rect, y: &mut u16) {
        let display_value = if self.secret {
            "•".repeat(self.value.chars().count())
        } else {
            self.value.to_string()
        };
        let input_text = if display_value.is_empty() {
            Span::styled(
                self.placeholder.to_string(),
                Style::default().fg(theme.text.tertiary),
            )
        } else {
            Span::styled(display_value, Style::default().fg(theme.text.primary))
        };
        frame.render_widget(
            Paragraph::new(vec![Line::from(input_text), Line::from(""), Line::from("")])
                .style(Style::default().bg(theme.surface.panel_elevated)),
            horizontal_inset(Rect::new(area.x, *y, area.width, 3), 2),
        );
        *y += 4;
    }
}
