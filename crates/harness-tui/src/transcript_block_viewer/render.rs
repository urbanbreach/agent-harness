use std::ops::Range;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};

use crate::theme::Theme;

use super::state::ViewerState;
use super::ViewerMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedLine {
    pub text: String,
    pub selected: bool,
    pub current_match: bool,
    pub match_range: Option<Range<usize>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewerRenderSurface {
    pub mode: ViewerMode,
    pub title: String,
    pub status: String,
    pub lines: Vec<RenderedLine>,
    pub scroll_top: u16,
}

pub fn render_surface(state: &ViewerState, _area: Rect) -> ViewerRenderSurface {
    let text = match (state.mode(), state.content().raw()) {
        (ViewerMode::Raw, Some(raw)) => raw,
        (ViewerMode::Wrapped, _) | (ViewerMode::Raw, None) => state.content().content(),
    };
    let current = state
        .search()
        .current_match()
        .map(|item| item.byte_range.clone());
    let mut offset = 0;
    let selection = state.selection().map(|selection| {
        (
            selection.anchor.row.min(selection.focus.row),
            selection.anchor.row.max(selection.focus.row),
        )
    });
    let lines = text
        .split('\n')
        .enumerate()
        .map(|(line_index, line)| {
            let range = offset..offset + line.len();
            let match_range = current.as_ref().and_then(|candidate| {
                let start = candidate.start.max(range.start);
                let end = candidate.end.min(range.end);
                (start < end).then_some(start - range.start..end - range.start)
            });
            let current_match = match_range.is_some();
            offset += line.len() + 1;
            RenderedLine {
                text: line.to_owned(),
                selected: selection.is_some_and(|(start, end)| (start..=end).contains(&line_index)),
                current_match,
                match_range,
            }
        })
        .collect::<Vec<_>>();
    let query = state.search().query();
    let status = if query.is_empty() {
        "search: idle".to_string()
    } else if state.search().no_result() {
        format!("search: {query} · no results")
    } else {
        format!(
            "search: {query} · {}/{}",
            state
                .search()
                .current_match_index()
                .map_or(0, |index| index + 1),
            state.search().matches().len()
        )
    };
    ViewerRenderSurface {
        mode: state.mode(),
        title: format!("Block Viewer · {}", mode_label(state.mode())),
        status,
        lines,
        scroll_top: state
            .scroll_anchor()
            .map_or(0, |anchor| scroll_offset(anchor.within_block())),
    }
}

pub fn render_to_buffer(
    buffer: &mut Buffer,
    area: Rect,
    surface: &ViewerRenderSurface,
    theme: &Theme,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.reference_terminal.muted))
        .title(surface.title.clone());
    let inner = block.inner(area);
    block.render(area, buffer);
    if inner.height == 0 {
        return;
    }
    let body_height = inner.height.saturating_sub(1);
    let body = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: body_height,
    };
    let lines = surface.lines.iter().map(render_line).collect::<Vec<_>>();
    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((surface.scroll_top, 0))
        .render(body, buffer);
    let footer = Rect {
        x: inner.x,
        y: inner.y + body_height,
        width: inner.width,
        height: 1,
    };
    Paragraph::new(surface.status.clone())
        .style(Style::default().fg(Color::DarkGray))
        .render(footer, buffer);
}

fn render_line(line: &RenderedLine) -> ratatui::text::Line<'static> {
    let base = if line.selected {
        Style::default().bg(Color::Blue).fg(Color::White)
    } else {
        Style::default().fg(Color::White)
    };
    let highlight = if line.current_match {
        base.bg(Color::Yellow)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        base
    };
    ratatui::text::Line::from(ratatui::text::Span::styled(line.text.clone(), highlight))
}

const fn mode_label(mode: ViewerMode) -> &'static str {
    match mode {
        ViewerMode::Wrapped => "wrapped",
        ViewerMode::Raw => "raw",
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "scroll offsets are clamped to the ratatui u16 viewport contract"
)]
fn scroll_offset(value: f64) -> u16 {
    if !value.is_finite() || value <= 0.0 {
        0
    } else if value >= f64::from(u16::MAX) {
        u16::MAX
    } else {
        value.floor() as u16
    }
}
