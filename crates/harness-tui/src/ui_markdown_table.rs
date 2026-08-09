use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::theme::Theme;

use super::ui_chrome::display_width;
use super::ui_transcript_surface::append_prefixed_wrapped_spans_line;

const TABLE_COLUMN_GAP: &str = "  ";

pub(super) fn try_render_markdown_table_block(
    rows: &[&str],
    color: Color,
    prefix: &str,
    theme: &Theme,
    width: u16,
) -> Option<(Vec<Line<'static>>, usize)> {
    let header = parse_table_row(rows.first().copied()?)?;
    let separator = rows.get(1).copied()?;
    if !is_table_separator_row(separator, header.len()) {
        return None;
    }

    let body_rows = rows
        .iter()
        .skip(2)
        .map_while(|row| parse_table_row(row))
        .collect::<Vec<_>>();
    if body_rows.is_empty() {
        return None;
    }

    let column_count = header.len();
    let column_widths = table_column_widths(&header, &body_rows, column_count);
    let mut rendered = Vec::with_capacity(body_rows.len() + 1);
    append_table_row(
        &mut rendered,
        &header,
        &column_widths,
        prefix,
        Style::default()
            .fg(theme.markdown.heading_h1)
            .add_modifier(Modifier::BOLD),
        width,
    );
    for row in &body_rows {
        append_table_row(
            &mut rendered,
            row,
            &column_widths,
            prefix,
            Style::default().fg(color),
            width,
        );
    }

    Some((rendered, body_rows.len() + 2))
}

fn append_table_row(
    rendered: &mut Vec<Line<'static>>,
    row: &[String],
    column_widths: &[usize],
    prefix: &str,
    style: Style,
    width: u16,
) {
    let mut spans = Vec::new();
    for (index, column_width) in column_widths.iter().copied().enumerate() {
        if index > 0 {
            spans.push(Span::styled(TABLE_COLUMN_GAP.to_string(), style));
        }
        let cell = row.get(index).map(String::as_str).unwrap_or(" ");
        spans.push(Span::styled(pad_cell(cell, column_width), style));
    }
    append_prefixed_wrapped_spans_line(rendered, prefix, style, spans, width);
}

fn table_column_widths(
    header: &[String],
    body_rows: &[Vec<String>],
    column_count: usize,
) -> Vec<usize> {
    let mut widths = vec![1; column_count];
    for row in std::iter::once(header).chain(body_rows.iter().map(Vec::as_slice)) {
        for (index, cell) in row.iter().take(column_count).enumerate() {
            widths[index] = widths[index].max(display_width(cell));
        }
    }
    widths
}

fn pad_cell(cell: &str, width: usize) -> String {
    let padding = width.saturating_sub(display_width(cell));
    format!("{cell}{}", " ".repeat(padding))
}

fn parse_table_row(row: &str) -> Option<Vec<String>> {
    let trimmed = row.trim();
    if !trimmed.contains('|') {
        return None;
    }

    let content = trimmed.trim_matches('|');
    let cells = split_unescaped_pipes(content)
        .into_iter()
        .map(|cell| conceal_table_cell(cell.trim()))
        .collect::<Vec<_>>();
    (cells.len() >= 2).then_some(cells)
}

fn split_unescaped_pipes(content: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut escaped = false;
    for ch in content.chars() {
        if escaped {
            cell.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '|' {
            cells.push(std::mem::take(&mut cell));
            continue;
        }
        cell.push(ch);
    }
    cells.push(cell);
    cells
}

fn conceal_table_cell(cell: &str) -> String {
    let mut text = conceal_links(cell);
    for marker in ["**", "__", "~~", "`", "*", "_"] {
        text = text.replace(marker, "");
    }
    text
}

fn conceal_links(cell: &str) -> String {
    let mut output = String::new();
    let mut remaining = cell;
    while let Some(start) = remaining.find('[') {
        output.push_str(&remaining[..start]);
        let after_open = &remaining[start + 1..];
        let Some(label_end) = after_open.find("](") else {
            output.push_str(&remaining[start..]);
            return output;
        };
        let after_label = &after_open[label_end + 2..];
        let Some(url_end) = after_label.find(')') else {
            output.push_str(&remaining[start..]);
            return output;
        };
        output.push_str(&after_open[..label_end]);
        remaining = &after_label[url_end + 1..];
    }
    output.push_str(remaining);
    output
}

fn is_table_separator_row(row: &str, column_count: usize) -> bool {
    let Some(cells) = parse_table_row(row) else {
        return false;
    };
    cells.len() == column_count
        && cells.iter().all(|cell| {
            let marker = cell.trim();
            marker.len() >= 3
                && marker
                    .chars()
                    .all(|ch| matches!(ch, '-' | ':') || ch.is_whitespace())
                && marker.chars().any(|ch| ch == '-')
        })
}
