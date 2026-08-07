use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
    Frame,
};

use crate::theme::Theme;

pub(super) const PANEL_WIDTH: u16 = 60;
pub(super) const PROMPT_HEADER_HEIGHT: u16 = 2;
pub(super) const SELECT_HEADER_HEIGHT: u16 = 4;

pub(super) fn render_panel(
    frame: &mut Frame,
    theme: &Theme,
    root: Rect,
    requested_height: u16,
) -> Rect {
    let area = panel_area(root, requested_height);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.surface.panel_elevated)),
        area,
    );
    area
}

pub(super) fn panel_area(root: Rect, requested_height: u16) -> Rect {
    let width = PANEL_WIDTH.min(root.width.saturating_sub(2)).max(1);
    let height = requested_height.min(root.height.saturating_sub(2)).max(1);
    let x = root.x + (root.width.saturating_sub(width)) / 2;
    let max_y = root.y + root.height.saturating_sub(height);
    let y = (root.y + root.height / 4).min(max_y);
    Rect::new(x, y, width, height)
}

pub(super) fn horizontal_inset(area: Rect, requested_inset: u16) -> Rect {
    let inset = requested_inset.min(area.width.saturating_sub(1) / 2);
    Rect::new(
        area.x.saturating_add(inset),
        area.y,
        area.width.saturating_sub(inset.saturating_mul(2)).max(1),
        area.height,
    )
}

pub(super) fn left_inset(area: Rect, requested_inset: u16) -> Rect {
    let inset = requested_inset.min(area.width.saturating_sub(1));
    Rect::new(
        area.x.saturating_add(inset),
        area.y,
        area.width.saturating_sub(inset).max(1),
        area.height,
    )
}

pub(super) fn render_select_header(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    title: &str,
    filter: &str,
) {
    let header_area = horizontal_inset(area, 4);
    render_header(
        frame,
        theme,
        Rect::new(
            header_area.x,
            area.y.saturating_add(1),
            header_area.width,
            1,
        ),
        title,
    );
    let input = if filter.is_empty() { "Search" } else { filter };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            input,
            Style::default().fg(theme.text.tertiary),
        ))),
        Rect::new(
            header_area.x,
            area.y.saturating_add(3),
            header_area.width,
            1,
        ),
    );
}

pub(super) fn render_prompt_header(frame: &mut Frame, theme: &Theme, area: Rect, title: &str) {
    let header_area = horizontal_inset(area, 2);
    render_header(
        frame,
        theme,
        Rect::new(
            header_area.x,
            area.y.saturating_add(1),
            header_area.width,
            1,
        ),
        title,
    );
}

fn render_header(frame: &mut Frame, theme: &Theme, area: Rect, title: &str) {
    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            title.to_string(),
            Style::default()
                .fg(theme.text.primary)
                .add_modifier(Modifier::BOLD),
        )])),
        area,
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "esc",
            Style::default().fg(theme.text.tertiary),
        )))
        .alignment(Alignment::Right),
        area,
    );
}

pub(super) fn render_empty(frame: &mut Frame, theme: &Theme, area: Rect) {
    let empty_area = horizontal_inset(area, 4);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "No results found",
            Style::default().fg(theme.text.tertiary),
        ))),
        Rect::new(empty_area.x, area.y, empty_area.width, 1),
    );
}

pub(super) fn render_option_row(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    selected: bool,
    title: &str,
    description: Option<&str>,
) {
    let background = if selected {
        theme.text.accent
    } else {
        theme.surface.panel_elevated
    };
    let foreground = if selected {
        theme.surface.canvas
    } else {
        theme.text.primary
    };
    let muted = if selected {
        theme.surface.canvas
    } else {
        theme.text.tertiary
    };
    frame.render_widget(
        Block::default().style(Style::default().bg(background)),
        area,
    );

    let mut spans = vec![
        Span::raw("      "),
        Span::styled(
            title.to_string(),
            Style::default().fg(foreground).add_modifier(if selected {
                Modifier::BOLD
            } else {
                Modifier::empty()
            }),
        ),
    ];
    if let Some(description) = description {
        spans.push(Span::styled(
            format!(" {description}"),
            Style::default().fg(muted),
        ));
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(background)),
        area,
    );
}

pub(super) fn split_hint<'a>(hint: &'a str, theme: &Theme) -> Vec<Span<'a>> {
    let mut parts = hint.splitn(2, ' ');
    let key = parts.next().unwrap_or_default();
    let label = parts.next().unwrap_or_default();
    vec![
        Span::styled(key, Style::default().fg(theme.text.primary)),
        Span::styled(
            format!(" {label}"),
            Style::default().fg(theme.text.tertiary),
        ),
    ]
}

pub(super) fn wrap_text(text: &str, width: u16) -> Vec<String> {
    let width = usize::from(width.max(1));
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let separator = usize::from(!current.is_empty());
        if !current.is_empty() && current.len() + separator + word.len() > width {
            lines.push(current);
            current = String::new();
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

pub(super) fn visible_select_rows(root: Rect, row_count: u16) -> u16 {
    let cap = root.height.saturating_div(2).saturating_sub(6).max(3);
    row_count.min(cap).max(1)
}

pub(super) fn visible_window(
    total: usize,
    selected: usize,
    height: u16,
) -> impl Iterator<Item = usize> {
    let height = usize::from(height).max(1);
    let start = if selected >= height {
        selected + 1 - height
    } else {
        0
    };
    let end = (start + height).min(total);
    start..end
}
