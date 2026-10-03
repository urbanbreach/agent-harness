use super::*;
use crate::composer_atoms::AtomKind;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(super) fn render_bar(frame: &mut Frame, app: &AppState, inner: Rect, theme: &Theme) {
    let search = &app.transcript_view.search;
    if search.has_bar() && !app.current_subagent_session_present() {
        render_parent_bar(frame, app, inner, theme);
        return;
    }
    if !search.has_bar() || inner.width == 0 {
        return;
    }
    let query = search.editor.text();
    let counter = if app.transcript_view.search_match_count > 0 {
        format!(
            "{}/{}",
            app.transcript_view.search_match + 1,
            app.transcript_view.search_match_count
        )
    } else if search.active && search.regex.is_none() {
        "bad pattern".into()
    } else if search.active {
        "no matches".into()
    } else {
        String::new()
    };
    let area = Rect::new(inner.x, inner.bottom(), inner.width, 2);
    let label = " search: ";
    let available = usize::from(area.width).saturating_sub(label.width());
    let reserved = if !counter.is_empty() && available >= counter.width() + 2 {
        counter.width() + 1
    } else {
        0
    };
    let input_width = available.saturating_sub(reserved);
    frame.render_widget(
        Paragraph::new("─".repeat(usize::from(area.width)))
            .style(Style::default().fg(theme.terminal_colors.muted)),
        Rect { height: 1, ..area },
    );
    let (displayed, cursor) = query_viewport(search, &query, input_width);
    let row = Rect::new(
        area.x,
        area.y + 1,
        area.width
            .saturating_sub(u16::try_from(reserved).unwrap_or(area.width)),
        1,
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(label, Style::default().fg(theme.text.secondary)),
            Span::styled(displayed, Style::default().fg(theme.text.primary)),
        ])),
        row,
    );
    if search.editing && input_width > 0 {
        let x = row.x
            + u16::try_from(label.width()).unwrap_or(row.width)
            + u16::try_from(cursor).unwrap_or(row.width);
        if let Some(cell) = frame.buffer_mut().cell_mut((x, row.y)) {
            cell.set_style(
                Style::default()
                    .fg(theme.surface.canvas)
                    .bg(theme.text.primary),
            );
        }
    }
    if reserved > 0 {
        frame.render_widget(
            Paragraph::new(counter.clone()).style(Style::default().fg(theme.text.secondary)),
            Rect::new(
                area.right() - u16::try_from(counter.width()).unwrap_or(area.width),
                row.y,
                u16::try_from(counter.width()).unwrap_or(area.width),
                1,
            ),
        );
    }
}

fn query_viewport(
    search: &crate::app::pane_query::PaneQuery,
    text: &str,
    width: usize,
) -> (String, usize) {
    if !search.editing {
        return (
            text.graphemes(true)
                .scan(0, |used, glyph| {
                    *used += glyph.width();
                    (*used <= width).then_some(glyph)
                })
                .collect(),
            0,
        );
    }
    let cursor: usize = search
        .editor
        .buffer()
        .atoms()
        .iter()
        .take(search.editor.cursor().insertion_index())
        .filter_map(|atom| match &atom.kind {
            AtomKind::Text(text) => Some(text.as_str().len()),
            _ => None,
        })
        .sum();
    let mut start = cursor;
    let mut remaining = width.saturating_sub(1);
    for (byte, glyph) in text[..cursor].grapheme_indices(true).rev() {
        if glyph.width() > remaining {
            break;
        }
        start = byte;
        remaining -= glyph.width();
    }
    let displayed = text[start..]
        .graphemes(true)
        .scan(0, |used, glyph| {
            *used += glyph.width();
            (*used <= width).then_some(glyph)
        })
        .collect();
    (displayed, text[start..cursor].width())
}

pub(super) fn highlight(frame: &mut Frame, app: &AppState, area: Rect) {
    if !app.current_subagent_session_present() {
        return;
    }
    let Some(regex) = &app.transcript_view.search.regex else {
        return;
    };
    for row in area.y..area.bottom() {
        let mut text = String::new();
        let mut columns = Vec::new();
        for column in area.x..area.right() {
            let cell = &frame.buffer_mut()[(column, row)];
            columns.push((text.len(), column));
            text.push_str(cell.symbol());
        }
        for found in regex.find_iter(&text).filter(|found| !found.is_empty()) {
            for &(_, column) in columns
                .iter()
                .filter(|(byte, _)| found.range().contains(byte))
            {
                frame.buffer_mut()[(column, row)]
                    .set_style(Style::default().add_modifier(Modifier::REVERSED));
            }
        }
    }
}

fn render_parent_bar(frame: &mut Frame, app: &AppState, inner: Rect, theme: &Theme) {
    let state = &app.transcript_view;
    let query = state.search.editor.text();
    let label = if state.search_match_count == 0 {
        format!("/{query} · no results")
    } else {
        format!(
            "/{query} · {}/{} · n/N next/previous",
            state.search_match + 1,
            state.search_match_count
        )
    };
    let footer = Rect::new(inner.x, inner.bottom(), inner.width, 1);
    frame.render_widget(Clear, footer);
    frame.render_widget(
        Paragraph::new(label).style(
            Style::default()
                .fg(theme.text.primary)
                .bg(theme.surface.canvas),
        ),
        footer,
    );
}
