use std::ops::Range;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::theme::Theme;

use super::state::ViewerState;
use super::{RenderedLine, ViewerMode, ViewerRenderSurface};
use crate::transcript_selection::CellPoint;

pub fn render_surface(state: &ViewerState, _area: Rect) -> ViewerRenderSurface {
    project_rows(state, 0..state.wrapped.row_count())
}

pub(crate) fn render_viewer(buffer: &mut Buffer, area: Rect, state: &ViewerState, theme: &Theme) {
    let body = super::viewer_layout(area).content_body(state.input_active() || state.visual_mode);
    let first = state.scroll_top();
    let count = state.wrapped.row_count();
    let rows = first..first.saturating_add(usize::from(body.height)).min(count);
    let surface = project_rows(state, rows);
    paint(buffer, area, &surface, first, count, theme);
}

fn project_rows(state: &ViewerState, rows: Range<usize>) -> ViewerRenderSurface {
    let mut filter_search = super::SearchState::new();
    if state.filter_editing() {
        let _ = filter_search.set_query(state.wrapped.text(), &regex::escape(&state.filter_query));
    }
    let highlights = if state.filter_editing() {
        &filter_search
    } else {
        state.search()
    };
    let mut match_ranges = vec![Vec::new(); rows.len()];
    for item in highlights.matches() {
        let start = state.wrapped.point_for_byte(item.byte_range.start);
        let end = state
            .wrapped
            .point_for_byte(item.byte_range.end.saturating_sub(1));
        for row in start.row.max(rows.start)..end.row.saturating_add(1).min(rows.end) {
            if let Some(range) = cells_in_row(start, end, row) {
                match_ranges[row - rows.start].push(range);
            }
        }
    }

    let current = state.search().current_match().map(|item| {
        (
            state.wrapped.point_for_byte(item.byte_range.start),
            state
                .wrapped
                .point_for_byte(item.byte_range.end.saturating_sub(1)),
        )
    });
    let selection = state.selection().map(|selection| selection.normalized());
    let lines = rows
        .zip(match_ranges)
        .map(|(row, match_ranges)| {
            let selection_range = selection.and_then(|(start, end)| cells_in_row(start, end, row));
            RenderedLine {
                text: state.wrapped.row_text(row).to_owned(),
                styled: styled_row(state, row),
                selected: selection_range.is_some(),
                current_match: current
                    .is_some_and(|(start, end)| (start.row..=end.row).contains(&row)),
                match_ranges,
                selection_range,
            }
        })
        .collect();
    let query = state.search().query();
    let filtering = state.filter_editing() || !state.filter_query.is_empty();
    let editing = state.search_editing() || state.filter_editing();
    let label = if filtering { "filter" } else { "search" };
    let query = if filtering {
        &state.filter_query
    } else {
        query
    };
    let status = if state.input_active() {
        if editing {
            format!("{label}: {query}")
        } else {
            format!("[{label}: {query}]  ")
        }
    } else if state.visual_mode {
        let count = selection.map_or(1, |(start, end)| {
            (start.row..=end.row)
                .filter(|row| {
                    *row == start.row
                        || state
                            .row_joiners
                            .get(row - 1)
                            .is_some_and(|joiner| joiner == "\n")
                })
                .count()
        });
        format!(
            "Selected: {count} line{}",
            if count == 1 { "" } else { "s" }
        )
    } else {
        String::new()
    };
    ViewerRenderSurface {
        mode: state.mode(),
        title: format!("Block Viewer · {}", mode_label(state.mode())),
        status,
        lines,
        scroll_top: state.scroll_top(),
        body_start: state.body_start,
        cursor_rows: if state.following {
            0..0
        } else {
            state.logical_rows(state.cursor.row)
        },
        output_panel: matches!(
            state.content().preamble,
            Some(super::ViewerPreamble::Command { .. })
        ),
        copy_path: matches!(
            state.content().preamble,
            Some(super::ViewerPreamble::Read { .. })
        ),
        markdown: state.content().markdown,
        close_hovered: state.close_hovered,
        search_active: state.input_active(),
        editing,
        input_cursor: query
            .grapheme_indices(true)
            .nth(state.input.editor.cursor().insertion_index())
            .map_or(query.len(), |(byte, _)| byte),
        following: state.following,
        child: state.child,
        filtering,
        visual_mode: state.visual_mode,
        wrap_enabled: state.wrap_enabled,
    }
}

fn styled_row(state: &ViewerState, row: usize) -> Option<ratatui::text::Line<'static>> {
    let mut line = state.styled_lines.get(row)?.clone();
    if state.child && state.content().markdown {
        for span in &mut line.spans {
            if span.style.fg == Some(state.theme().text.primary) {
                span.style.fg = Some(viewer_secondary(state.theme()));
            }
        }
    }
    Some(line)
}

fn cells_in_row(start: CellPoint, end: CellPoint, row: usize) -> Option<Range<usize>> {
    (start.row..=end.row).contains(&row).then_some(
        (if row == start.row { start.cell } else { 0 })..(if row == end.row {
            end.cell.saturating_add(1)
        } else {
            usize::MAX
        }),
    )
}

pub fn render_to_buffer(
    buffer: &mut Buffer,
    area: Rect,
    surface: &ViewerRenderSurface,
    theme: &Theme,
) {
    paint(buffer, area, surface, 0, surface.lines.len(), theme);
}

fn paint(
    buffer: &mut Buffer,
    area: Rect,
    surface: &ViewerRenderSurface,
    first_row: usize,
    row_count: usize,
    theme: &Theme,
) {
    let layout = super::viewer_layout(area);
    if layout.popup.width < 12 || layout.popup.height < 5 {
        return;
    }
    for y in layout.overlay.y..layout.overlay.bottom() {
        for x in layout.overlay.x..layout.overlay.right() {
            let cell = &mut buffer[(x, y)];
            cell.fg = dim(cell.fg, theme.surface.shell);
            if !surface.child {
                cell.bg = dim(cell.bg, theme.surface.shell);
            }
        }
    }
    Clear.render(layout.popup, buffer);
    buffer.set_style(
        layout.popup,
        Style::default()
            .fg(theme.text.primary)
            .bg(theme.surface.shell),
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.terminal_colors.muted))
        .style(Style::default().bg(theme.surface.shell));
    block.render(layout.popup, buffer);
    Paragraph::new("[x]")
        .style(if surface.close_hovered {
            Style::default()
                .fg(theme.text.primary)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.terminal_colors.muted)
        })
        .render(layout.close, buffer);
    let body = layout.content_body(surface.search_active || surface.visual_mode);
    for (offset, line) in surface
        .lines
        .iter()
        .skip(surface.scroll_top.saturating_sub(first_row))
        .take(usize::from(body.height))
        .enumerate()
    {
        let index = surface.scroll_top + offset;
        let row = Rect::new(
            body.x,
            body.y + u16::try_from(offset).unwrap_or(u16::MAX),
            body.width
                + if surface.filtering && row_count <= usize::from(body.height) {
                    2
                } else {
                    0
                },
            1,
        );
        let background = if surface.visual_mode && line.selected {
            visual_background(theme)
        } else if surface.cursor_rows.contains(&index) {
            theme.surface.selected_card
        } else if surface.output_panel && index >= surface.body_start {
            theme.markdown.code_background
        } else {
            theme.surface.shell
        };
        buffer.set_style(row, Style::default().bg(background));
        Paragraph::new(render_line(line, surface.visual_mode, theme)).render(row, buffer);
        // Selection is an overlay: keep the source text's foreground and modifiers.
        if surface.cursor_rows.contains(&index) || (surface.visual_mode && line.selected) {
            buffer.set_style(row, Style::default().bg(background));
        }
        if !surface.wrap_enabled && line.text.width() > usize::from(row.width) {
            buffer[(row.right() - 1, row.y)].set_symbol("…");
        }
    }
    render_scrollbar(buffer, body, surface.scroll_top, row_count, theme);
    if surface.following && !body.is_empty() {
        place_indicator(buffer, body, body.bottom() - 1, "▶", theme);
        buffer[(body.right() - 1, body.bottom() - 1)].set_fg(theme.status.warning);
    }
    super::render_input::paint_status(buffer, &layout, body, surface, theme);
    render_shortcuts(buffer, layout.shortcuts, surface, theme);
}

fn dim(color: Color, base: Color) -> Color {
    if let (Color::Rgb(r, g, b), Color::Rgb(br, bg, bb)) = (color, base) {
        let half = |a: u8, b: u8| {
            u8::try_from((u16::from(a) + u16::from(b)).div_ceil(2)).unwrap_or(u8::MAX)
        };
        Color::Rgb(half(r, br), half(g, bg), half(b, bb))
    } else {
        color
    }
}

fn render_scrollbar(
    buffer: &mut Buffer,
    body: Rect,
    scroll_top: usize,
    row_count: usize,
    theme: &Theme,
) {
    if row_count <= usize::from(body.height) || body.is_empty() {
        return;
    }
    let area = Rect::new(body.right() + 1, body.y, 1, body.height);
    let scrollbar = tui_scrollbar::ScrollBar::vertical(tui_scrollbar::ScrollLengths {
        content_len: row_count,
        viewport_len: usize::from(body.height),
    })
    .offset(scroll_top)
    .glyph_set(tui_scrollbar::GlyphSet {
        thumb_vertical_lower: ['█'; 8],
        thumb_vertical_upper: ['█'; 8],
        ..Default::default()
    })
    .track_style(Style::default().bg(theme.surface.shell))
    .thumb_style(
        Style::default()
            .fg(theme.surface.selected_card)
            .bg(theme.surface.selected_card),
    );
    (&scrollbar).render(area, buffer);
    if scroll_top > 0 {
        place_indicator(buffer, body, body.y, "▲", theme);
    }
    if scroll_top + usize::from(body.height) < row_count {
        place_indicator(buffer, body, body.bottom() - 1, "▼", theme);
    }
}

fn place_indicator(buffer: &mut Buffer, body: Rect, y: u16, symbol: &str, theme: &Theme) {
    let x = body.right() - 1;
    if body.width >= 3
        && (!buffer[(x, y)].symbol().trim().is_empty()
            || !buffer[(x - 1, y)].symbol().trim().is_empty())
    {
        buffer[(x - 2, y)].set_symbol("…");
        buffer[(x - 1, y)].set_symbol(" ");
    }
    let cell = &mut buffer[(x, y)];
    cell.set_symbol(symbol).set_fg(theme.text.secondary);
    cell.modifier = Modifier::empty();
}

fn render_shortcuts(buffer: &mut Buffer, area: Rect, surface: &ViewerRenderSurface, theme: &Theme) {
    Clear.render(area, buffer);
    buffer.set_style(area, Style::default().bg(theme.surface.shell));
    let mut hints = vec![
        ("Esc", "close"),
        ("Enter", "quote"),
        ("/", "search"),
        ("f", "filter"),
        ("v", "select"),
        ("w", "wrap"),
    ];
    if surface.output_panel {
        hints.push(("Shift+y", "copy cmd"));
    } else if surface.copy_path {
        hints.push(("Shift+y", "copy path"));
    } else if surface.markdown {
        hints.push(("r", "raw"));
    }
    let mut x = area.x;
    let label_style = Style::default().fg(theme.text.secondary);
    for (index, (key, label)) in hints.into_iter().enumerate() {
        if index > 0 {
            if x + 5 > area.right() {
                break;
            }
            buffer.set_string(x, area.y, "  │  ", label_style.add_modifier(Modifier::DIM));
            x += 5;
        }
        let key_width = u16::try_from(key.width()).unwrap_or(u16::MAX);
        if x + key_width > area.right() {
            break;
        }
        buffer.set_string(
            x,
            area.y,
            key,
            Style::default()
                .fg(viewer_secondary(theme))
                .add_modifier(Modifier::BOLD),
        );
        x += key_width;
        if x >= area.right() {
            break;
        }
        buffer.set_string(x, area.y, ":", label_style);
        x += 1;
        let label_width = u16::try_from(label.width()).unwrap_or(u16::MAX);
        if x + label_width > area.right() {
            break;
        }
        buffer.set_string(x, area.y, label, label_style);
        x += label_width;
    }
}

fn render_line<'a>(
    line: &'a RenderedLine,
    visual_mode: bool,
    theme: &Theme,
) -> ratatui::text::Line<'a> {
    let mut column = 0;
    let mut source_styles = line
        .styled
        .iter()
        .flat_map(|line| &line.spans)
        .scan(0, |end, span| {
            *end += span.width();
            Some((*end, span.style))
        })
        .peekable();
    let spans = line
        .text
        .graphemes(true)
        .map(|grapheme| {
            let start = column;
            column += grapheme.width();
            while source_styles.peek().is_some_and(|(end, _)| *end <= start) {
                let _ = source_styles.next();
            }
            let mut style = source_styles.peek().map_or_else(
                || Style::default().fg(theme.terminal_colors.primary),
                |(_, style)| *style,
            );
            if line
                .match_ranges
                .iter()
                .any(|range| range.start < column && range.end > start)
            {
                style = style.add_modifier(Modifier::REVERSED);
            }
            if !visual_mode
                && line
                    .selection_range
                    .as_ref()
                    .is_some_and(|range| range.start < column && range.end > start)
            {
                style = style.bg(theme.text.accent).fg(theme.surface.canvas);
            }
            ratatui::text::Span::styled(grapheme, style)
        })
        .collect::<Vec<_>>();
    ratatui::text::Line::from(spans)
}

fn visual_background(theme: &Theme) -> Color {
    match theme.surface.shell {
        Color::Rgb(20, 20, 20) => Color::Rgb(54, 54, 54),
        Color::Rgb(250, 250, 250) => Color::Rgb(198, 198, 198),
        _ => theme.surface.selected_card,
    }
}

pub(super) fn viewer_secondary(theme: &Theme) -> Color {
    if theme.surface.shell == Color::Rgb(20, 20, 20) {
        Color::Rgb(200, 200, 200)
    } else {
        theme.text.secondary
    }
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
    reason = "finite nonnegative row counts saturate on conversion to the platform index type"
)]
pub(super) fn scroll_offset(value: f64) -> usize {
    if value.is_finite() && value > 0.0 {
        value.floor() as usize
    } else {
        0
    }
}
