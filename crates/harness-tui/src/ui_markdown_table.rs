use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::theme::Theme;

use self::parse::{is_table_separator_row, parse_table_row};
use super::ui_chrome::display_width;
use super::ui_markdown::parse_inline_markdown;

#[path = "ui_markdown_table_parse.rs"]
mod parse;
use super::ui_transcript_surface::{
    wrap_surface_spans_with_links, SurfaceLinkRun, WrappedSurfaceRow,
};

#[cfg(test)]
#[path = "ui_markdown_table_tests.rs"]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TableLinkRun {
    pub(super) row: usize,
    pub(super) start_cell: usize,
    pub(super) end_cell: usize,
    pub(super) destination: String,
}

pub(super) fn try_render_markdown_table_block(
    rows: &[&str],
    color: Color,
    prefix: &str,
    theme: &Theme,
    width: u16,
) -> Option<(Vec<Line<'static>>, usize, Vec<TableLinkRun>)> {
    let header = parse_table_row(rows.first().copied()?)?;
    let separator = rows.get(1).copied()?;
    if !is_table_separator_row(separator, header.len()) {
        return None;
    }

    let body = rows
        .iter()
        .skip(2)
        .map_while(|row| parse_table_row(row))
        .collect::<Vec<_>>();
    if body.is_empty() {
        return None;
    }

    let column_count = header.len();
    let available = usize::from(width).saturating_sub(display_width(prefix));
    let border_and_padding = column_count.saturating_mul(3).saturating_add(1);
    let minimum_box_width = border_and_padding.saturating_add(column_count);
    if available < minimum_box_width {
        return None;
    }
    let column_widths = bounded_column_widths(&header, &body, column_count, available);
    let border_style = Style::default().fg(theme.border.subtle);
    let mut rendered = Vec::new();
    let mut links = Vec::new();
    rendered.push(border_line(
        prefix,
        &column_widths,
        '┌',
        '┬',
        '┐',
        border_style,
    ));
    append_table_row(
        &mut rendered,
        &header,
        &column_widths,
        prefix,
        Style::default()
            .fg(theme.markdown.heading_h1)
            .add_modifier(Modifier::BOLD),
        border_style,
        theme,
        &mut links,
    );
    rendered.push(border_line(
        prefix,
        &column_widths,
        '├',
        '┼',
        '┤',
        border_style,
    ));
    for row in &body {
        append_table_row(
            &mut rendered,
            row,
            &column_widths,
            prefix,
            Style::default().fg(color),
            border_style,
            theme,
            &mut links,
        );
    }
    rendered.push(border_line(
        prefix,
        &column_widths,
        '└',
        '┴',
        '┘',
        border_style,
    ));

    Some((rendered, body.len() + 2, links))
}

fn append_table_row(
    rendered: &mut Vec<Line<'static>>,
    row: &[String],
    column_widths: &[usize],
    prefix: &str,
    style: Style,
    border_style: Style,
    theme: &Theme,
    links: &mut Vec<TableLinkRun>,
) {
    let wrapped = column_widths
        .iter()
        .copied()
        .enumerate()
        .map(|(index, width)| {
            let cell = row.get(index).map_or("", String::as_str);
            let parsed =
                parse_inline_markdown(cell, style, style.fg.unwrap_or(Color::Reset), theme);
            let source_links = parsed
                .links
                .into_iter()
                .map(|link| SurfaceLinkRun {
                    start_cell: link.start_cell,
                    end_cell: link.end_cell,
                    destination: link.destination,
                })
                .collect::<Vec<_>>();
            let rows = wrap_surface_spans_with_links(parsed.spans, &source_links, width);
            if rows.is_empty() {
                vec![WrappedSurfaceRow {
                    spans: Vec::new(),
                    links: Vec::new(),
                }]
            } else {
                rows
            }
        })
        .collect::<Vec<_>>();
    let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);

    for line_index in 0..height {
        let mut spans = vec![
            Span::raw(prefix.to_string()),
            Span::styled("│", border_style),
        ];
        for (column_index, column_width) in column_widths.iter().copied().enumerate() {
            spans.push(Span::styled(" ", style));
            let cell_start = spans.iter().map(Span::width).sum::<usize>();
            let cell = wrapped[column_index].get(line_index);
            let cell_spans = cell.map(|row| row.spans.clone()).unwrap_or_default();
            let used = cell_spans.iter().map(Span::width).sum::<usize>();
            if let Some(cell) = cell {
                links.extend(cell.links.iter().map(|link| TableLinkRun {
                    row: rendered.len(),
                    start_cell: cell_start.saturating_add(link.start_cell),
                    end_cell: cell_start.saturating_add(link.end_cell),
                    destination: link.destination.clone(),
                }));
            }
            spans.extend(cell_spans);
            spans.push(Span::styled(
                " ".repeat(column_width.saturating_sub(used).saturating_add(1)),
                style,
            ));
            spans.push(Span::styled("│", border_style));
        }
        rendered.push(Line::from(spans));
    }
}

fn border_line(
    prefix: &str,
    column_widths: &[usize],
    left: char,
    join: char,
    right: char,
    style: Style,
) -> Line<'static> {
    let mut text = String::from(prefix);
    text.push(left);
    for (index, width) in column_widths.iter().copied().enumerate() {
        if index > 0 {
            text.push(join);
        }
        text.push_str(&"─".repeat(width.saturating_add(2)));
    }
    text.push(right);
    Line::from(Span::styled(text, style))
}

fn bounded_column_widths(
    header: &[String],
    body: &[Vec<String>],
    column_count: usize,
    available: usize,
) -> Vec<usize> {
    let mut widths = vec![1; column_count];
    for row in std::iter::once(header).chain(body.iter().map(Vec::as_slice)) {
        for (index, cell) in row.iter().take(column_count).enumerate() {
            let visible = parse_visible_cell(cell);
            widths[index] = widths[index].max(display_width(&visible));
        }
    }

    let overhead = column_count.saturating_mul(3).saturating_add(1);
    let content_budget = available.saturating_sub(overhead);
    while widths.iter().sum::<usize>() > content_budget {
        let Some((index, _)) = widths
            .iter()
            .enumerate()
            .filter(|(_, width)| **width > 1)
            .max_by_key(|(_, width)| **width)
        else {
            break;
        };
        widths[index] = widths[index].saturating_sub(1);
    }
    widths
}

fn parse_visible_cell(cell: &str) -> String {
    let theme = Theme::default();
    parse_inline_markdown(cell, Style::default(), Color::Reset, &theme)
        .spans
        .into_iter()
        .map(|span| span.content.into_owned())
        .collect()
}
