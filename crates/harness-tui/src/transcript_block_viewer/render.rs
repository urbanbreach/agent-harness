use std::ops::Range;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::theme::Theme;

use super::state::ViewerState;
use super::ViewerMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedLine {
    pub text: String,
    pub styled: Option<ratatui::text::Line<'static>>,
    pub selected: bool,
    pub current_match: bool,
    pub match_range: Option<Range<usize>>,
    pub selection_range: Option<Range<usize>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewerRenderSurface {
    pub mode: ViewerMode,
    pub title: String,
    pub status: String,
    pub lines: Vec<RenderedLine>,
    pub scroll_top: usize,
}

pub fn render_surface(state: &ViewerState, _area: Rect) -> ViewerRenderSurface {
    let current = state.search().current_match().map(|item| {
        (
            state.wrapped.point_for_byte(item.byte_range.start),
            state
                .wrapped
                .point_for_byte(item.byte_range.end.saturating_sub(1)),
        )
    });
    let selection = state.selection().map(|selection| {
        (
            selection.anchor.row.min(selection.focus.row),
            selection.anchor.row.max(selection.focus.row),
        )
    });
    let lines = (0..state.wrapped.row_count())
        .map(|line_index| {
            let line = state.wrapped.row_text(line_index);
            let match_range = current.as_ref().and_then(|(start, end)| {
                (start.row..=end.row).contains(&line_index).then_some(
                    if start.row == line_index {
                        start.cell
                    } else {
                        0
                    }..if end.row == line_index {
                        end.cell + 1
                    } else {
                        unicode_width::UnicodeWidthStr::width(line.as_str())
                    },
                )
            });
            let current_match = match_range.is_some();
            RenderedLine {
                text: line,
                styled: state.styled_lines.get(line_index).cloned(),
                selected: selection.is_some_and(|(start, end)| (start..=end).contains(&line_index)),
                current_match,
                match_range,
                selection_range: state.selection().and_then(|selection| {
                    let (start, end) = selection.normalized();
                    (start.row..=end.row).contains(&line_index).then_some(
                        if line_index == start.row {
                            start.cell
                        } else {
                            0
                        }..if line_index == end.row {
                            end.cell.saturating_add(1)
                        } else {
                            usize::MAX
                        },
                    )
                }),
            }
        })
        .collect::<Vec<_>>();
    let query = state.search().query();
    let status = if query.is_empty() {
        if state.search_editing() {
            "/".to_string()
        } else {
            "↑↓ scroll · / find · Shift+arrows select · Ctrl+C copy · r raw · Esc close".to_string()
        }
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
        .border_style(Style::default().fg(theme.terminal_colors.muted))
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
    let lines = surface
        .lines
        .iter()
        .skip(surface.scroll_top)
        .take(usize::from(body_height))
        .map(|line| render_line(line, theme))
        .collect::<Vec<_>>();
    Paragraph::new(lines).render(body, buffer);
    let footer = Rect {
        x: inner.x,
        y: inner.y + body_height,
        width: inner.width,
        height: 1,
    };
    Paragraph::new(surface.status.clone())
        .style(Style::default().fg(theme.terminal_colors.muted))
        .render(footer, buffer);
}

fn render_line(line: &RenderedLine, theme: &Theme) -> ratatui::text::Line<'static> {
    let mut column = 0;
    let spans = line
        .text
        .graphemes(true)
        .map(|grapheme| {
            let start = column;
            column += grapheme.width();
            let mut style = source_style(line, start)
                .unwrap_or_else(|| Style::default().fg(theme.terminal_colors.primary));
            if line
                .match_range
                .as_ref()
                .is_some_and(|range| range.start < column && range.end > start)
            {
                style = style
                    .bg(theme.terminal_colors.prompt_accent)
                    .fg(theme.surface.canvas)
                    .add_modifier(Modifier::BOLD);
            }
            if line
                .selection_range
                .as_ref()
                .is_some_and(|range| range.start < column && range.end > start)
            {
                style = style.bg(theme.text.accent).fg(theme.surface.canvas);
            }
            ratatui::text::Span::styled(grapheme.to_string(), style)
        })
        .collect::<Vec<_>>();
    ratatui::text::Line::from(spans)
}

const fn mode_label(mode: ViewerMode) -> &'static str {
    match mode {
        ViewerMode::Wrapped => "wrapped",
        ViewerMode::Raw => "raw",
    }
}

fn source_style(line: &RenderedLine, column: usize) -> Option<Style> {
    let mut end = 0;
    line.styled.as_ref()?.spans.iter().find_map(|span| {
        end += span.width();
        (column < end).then_some(span.style)
    })
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "finite nonnegative row counts saturate on conversion to the platform index type"
)]
pub(super) fn scroll_offset(value: f64) -> usize {
    if value.is_finite() && value > 0.0 {
        value.floor() as usize
    } else {
        0
    }
}
