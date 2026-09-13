use super::ui_tool_question_todo::TranscriptTodoStatus;
use super::*;
use crate::app::todo_pane::TodoQueryMode;
use crate::composer_atoms::AtomKind;
use unicode_segmentation::UnicodeSegmentation as _;
use unicode_width::UnicodeWidthStr as _;

pub(super) fn render_todo_pane(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let state = &app.todo_pane;
    let focused = app.todo_pane_focused() && app.overlay_stack().top().is_none();
    let content = Rect::new(
        area.x.saturating_add(3),
        area.y,
        area.width.saturating_sub(5),
        area.height,
    );
    let visible = state.visible_items();
    let has_status_rows = state.items.iter().any(|item| {
        !state.hide_done
            || !matches!(
                item.status,
                TranscriptTodoStatus::Completed | TranscriptTodoStatus::Cancelled
            )
    });
    if !has_status_rows {
        frame.render_widget(
            Paragraph::new(state.placeholder()).style(Style::default().fg(theme.text.tertiary)),
            content,
        );
    } else {
        let viewport = state.viewport_height(content.height);
        let scroll = state.scroll_offset(content.height);
        let overflow = visible.len() > viewport && content.width > 2;
        let list = Rect {
            width: content.width.saturating_sub(if overflow { 2 } else { 0 }),
            ..content
        };
        for (row, (index, item)) in visible.iter().skip(scroll).take(viewport).enumerate() {
            let has_indicator = (row == 0 && scroll > 0)
                || (row + 1 == viewport && scroll.saturating_add(viewport) < visible.len());
            let area = Rect::new(
                content.x,
                content
                    .y
                    .saturating_add(u16::try_from(row).unwrap_or(u16::MAX)),
                list.width.saturating_sub(if has_indicator { 2 } else { 0 }),
                1,
            );
            let selected = focused && state.selected == Some(*index);
            render_item(frame, area, item, selected, app, theme);
        }
        if scroll > 0 {
            draw_cell(
                frame,
                list.right().saturating_sub(1),
                content.y,
                "▲",
                theme.text.secondary,
            );
        }
        if scroll.saturating_add(viewport) < visible.len() {
            draw_cell(
                frame,
                list.right().saturating_sub(1),
                content
                    .y
                    .saturating_add(u16::try_from(viewport.saturating_sub(1)).unwrap_or(0)),
                "▼",
                theme.text.secondary,
            );
        }
        if overflow {
            render_scrollbar(
                frame,
                Rect::new(
                    content.right().saturating_sub(1),
                    content.y,
                    1,
                    u16::try_from(viewport).unwrap_or(u16::MAX),
                ),
                visible.len(),
                scroll,
                theme,
            );
        }
        if state.query.has_bar() && content.height > 1 {
            render_query(
                frame,
                app,
                Rect::new(
                    content.x,
                    content.bottom().saturating_sub(1),
                    content.width,
                    1,
                ),
                theme,
            );
        }
    }
    render_chrome(frame, area, focused, app, theme);
}

fn render_item(
    frame: &mut Frame,
    area: Rect,
    item: &TranscriptTodoItem,
    selected: bool,
    app: &AppState,
    theme: &Theme,
) {
    let ascii = theme.glyph_mode() == crate::theme::GlyphMode::Ascii;
    let (glyph, color) = match item.status {
        TranscriptTodoStatus::Pending => (if ascii { "o" } else { "□" }, theme.text.primary),
        TranscriptTodoStatus::InProgress => (if ascii { ">" } else { "▶" }, theme.status.warning),
        TranscriptTodoStatus::Completed => (
            theme.live_shell.transcript_glyphs.choice_checked,
            theme.status.success,
        ),
        TranscriptTodoStatus::Cancelled => (if ascii { "x" } else { "✗" }, theme.status.error),
    };
    let text = crate::text::collapse_inline_whitespace(&item.content);
    let mut spans = vec![
        Span::styled(glyph, Style::default().fg(color)),
        Span::raw(" "),
    ];
    let style = item.status.content_style(theme);
    let query = &app.todo_pane.query;
    let highlight = query.editing || query.mode == TodoQueryMode::Search;
    let mut offset = 0;
    if let Some(regex) = query.regex.as_ref().filter(|_| highlight) {
        for matched in regex.find_iter(&text) {
            spans.push(Span::styled(
                text[offset..matched.start()].to_string(),
                style,
            ));
            spans.push(Span::styled(
                matched.as_str().to_string(),
                style.add_modifier(Modifier::REVERSED),
            ));
            offset = matched.end();
        }
    }
    spans.push(Span::styled(text[offset..].to_string(), style));
    let background = if selected {
        theme.surface.selected_card
    } else {
        theme.surface.shell
    };
    frame.render_widget(
        Paragraph::new(truncate_styled_line(spans, usize::from(area.width)))
            .style(Style::default().bg(background)),
        area,
    );
}

fn truncate_styled_line(spans: Vec<Span<'static>>, width: usize) -> Line<'static> {
    if width == 0 {
        return Line::default();
    }
    if spans.iter().map(Span::width).sum::<usize>() <= width {
        return Line::from(spans);
    }
    let mut remaining = width.saturating_sub(1);
    let mut clipped = Vec::new();
    for span in spans {
        if span.width() <= remaining {
            remaining = remaining.saturating_sub(span.width());
            clipped.push(span);
        } else {
            if remaining > 0 {
                clipped.push(Span::styled(
                    super::take_width_prefix(&span.content, remaining).to_string(),
                    span.style,
                ));
            }
            let style = clipped.last().map_or(Style::default(), |span| span.style);
            clipped.push(Span::styled("…", style));
            break;
        }
    }
    Line::from(clipped)
}

fn render_scrollbar(frame: &mut Frame, area: Rect, total: usize, scroll: usize, theme: &Theme) {
    use ratatui::widgets::Widget as _;
    let scrollbar = tui_scrollbar::ScrollBar::vertical(tui_scrollbar::ScrollLengths {
        content_len: total,
        viewport_len: usize::from(area.height),
    })
    .offset(scroll)
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
    (&scrollbar).render(area, frame.buffer_mut());
}

fn render_chrome(frame: &mut Frame, area: Rect, focused: bool, app: &AppState, theme: &Theme) {
    if !focused && !app.todo_pane.hovered {
        return;
    }
    let color = crate::theme::quantize_color(
        if focused {
            Color::Rgb(60, 60, 65)
        } else {
            Color::Rgb(30, 30, 34)
        },
        theme.color_level(),
    );
    let left = area.x.saturating_sub(1);
    let right = area.right();
    let ascii = theme.glyph_mode() == crate::theme::GlyphMode::Ascii;
    for y in area.y..area.bottom() {
        draw_cell(frame, left, y, if ascii { "|" } else { "│" }, color);
        draw_cell(frame, right, y, if ascii { "|" } else { "│" }, color);
    }
    draw_cell(
        frame,
        left,
        area.y.saturating_sub(1),
        if ascii { "+" } else { "┌" },
        color,
    );
    let close = if focused {
        if ascii {
            "x"
        } else {
            "✗"
        }
    } else if ascii {
        "+"
    } else {
        "┐"
    };
    draw_cell(
        frame,
        right,
        area.y.saturating_sub(1),
        close,
        if focused && app.todo_pane.close_hovered {
            theme.text.primary
        } else {
            color
        },
    );
    draw_cell(
        frame,
        left,
        area.bottom(),
        if ascii { "+" } else { "└" },
        color,
    );
    draw_cell(
        frame,
        right,
        area.bottom(),
        if ascii { "+" } else { "┘" },
        color,
    );
}

fn draw_cell(frame: &mut Frame, x: u16, y: u16, symbol: &str, color: Color) {
    if let Some(cell) = frame.buffer_mut().cell_mut((x, y)) {
        cell.set_symbol(symbol)
            .set_fg(color)
            .set_style(Style::default().remove_modifier(Modifier::all()));
    }
}

fn render_query(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let query = &app.todo_pane.query;
    let mode = if query.mode == TodoQueryMode::Filter {
        "filter"
    } else {
        "search"
    };
    let text = query.editor.text();
    let base = Style::default()
        .fg(theme.terminal_colors.prompt_accent)
        .bg(theme.surface.shell);
    if !query.editing {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("[{mode}: {text}]  "),
                base.add_modifier(Modifier::DIM),
            )))
            .alignment(Alignment::Right),
            area,
        );
        return;
    }
    let base = base.fg(theme.text.primary);
    let label = format!("{mode}: ");
    let budget = usize::from(area.width)
        .saturating_sub(label.width())
        .saturating_sub(1);
    let cursor = query
        .editor
        .buffer()
        .atoms()
        .iter()
        .take(query.editor.cursor().insertion_index())
        .filter_map(|atom| match &atom.kind {
            AtomKind::Text(text) => Some(text.as_str().len()),
            _ => None,
        })
        .sum::<usize>();
    let mut start = cursor;
    let mut remaining = budget;
    for (index, grapheme) in text[..cursor].grapheme_indices(true).rev() {
        if grapheme.width() > remaining {
            break;
        }
        start = index;
        remaining = remaining.saturating_sub(grapheme.width());
    }
    let before = text[start..cursor].to_string();
    let after = super::truncate_plain_text(&text[cursor..], remaining.saturating_add(1));
    let cursor_glyph = after.graphemes(true).next().unwrap_or(" ").to_string();
    let suffix = after.get(cursor_glyph.len()..).unwrap_or("").to_string();
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(label, base.fg(theme.status.warning)),
            Span::styled(before, base),
            Span::styled(cursor_glyph, base.add_modifier(Modifier::REVERSED)),
            Span::styled(suffix, base),
        ]))
        .style(base),
        area,
    );
}
