#[cfg(test)]
use std::cell::Cell;
use std::cell::RefCell;

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    Frame,
};

use crate::theme::Theme;

use super::ui_chrome::display_width;
use super::ui_lifecycle::LifecycleSelectionSurface;
use super::ui_markdown::{
    markdown_heading_text, markdown_list_prefix, markdown_rule, parse_inline_markdown_spans,
};
use super::ui_transcript_surface::wrap_surface_spans;

const TRANSCRIPT_SELECTION_RAIL_GLYPH: &str = "┃";

thread_local! {
    static TRANSCRIPT_SELECTION_CACHE: RefCell<Vec<TranscriptSelectionCacheEntry>> = const { RefCell::new(Vec::new()) };

    #[cfg(test)]
    static TRANSCRIPT_SELECTION_CACHE_BUILD_COUNT: Cell<usize> = const { Cell::new(0) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct TranscriptSelectionCell {
    pub row: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TranscriptSelection {
    pub anchor: TranscriptSelectionCell,
    pub focus: TranscriptSelectionCell,
}

impl TranscriptSelection {
    fn normalized(self) -> (TranscriptSelectionCell, TranscriptSelectionCell) {
        if self.anchor <= self.focus {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct TranscriptSelectionSnapshot {
    pub(super) viewport: Rect,
    pub(super) scroll_top: usize,
    pub(super) rows: Vec<TranscriptSelectionRow>,
}

#[derive(Debug, Clone)]
struct TranscriptSelectionCacheEntry {
    key: TranscriptSelectionCacheKey,
    snapshot: TranscriptSelectionSnapshot,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct TranscriptSelectionCacheKey {
    pub(super) render_width: u16,
    pub(super) app_instance_id: u64,
    pub(super) render_key: u64,
    pub(super) theme: Theme,
    pub(super) area: Rect,
    pub(super) follow_mode: bool,
    pub(super) transcript_scroll: usize,
}

impl TranscriptSelectionCacheKey {
    fn matches(self, other: Self) -> bool {
        self.render_width == other.render_width
            && self.app_instance_id == other.app_instance_id
            && self.render_key == other.render_key
            && self.theme == other.theme
            && self.area == other.area
            && self.follow_mode == other.follow_mode
            && self.transcript_scroll == other.transcript_scroll
    }
}

#[derive(Debug, Clone)]
pub(super) struct TranscriptSelectionRow {
    pub(super) cells: Vec<String>,
    pub(super) continues_previous: bool,
    pub(super) copy_offset: usize,
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct TranscriptSelectionDebugSnapshot {
    pub viewport: Rect,
    pub rows: Vec<String>,
}

impl TranscriptSelectionSnapshot {
    pub(super) fn hit(&self, column: u16, row: u16) -> Option<TranscriptSelectionCell> {
        if !rect_contains(self.viewport, column, row) {
            return None;
        }

        Some(TranscriptSelectionCell {
            row: self
                .scroll_top
                .saturating_add(usize::from(row.saturating_sub(self.viewport.y))),
            column: usize::from(column.saturating_sub(self.viewport.x)),
        })
    }

    pub(super) fn selection_text(&self, selection: TranscriptSelection) -> Option<String> {
        let (start, end) = selection.normalized();
        if start == end || self.rows.is_empty() {
            return None;
        }

        let last_row = self.rows.len().saturating_sub(1);
        let start_row = start.row.min(last_row);
        let end_row = end.row.min(last_row);
        if start_row > end_row {
            return None;
        }

        let mut lines = Vec::new();
        for row_idx in start_row..=end_row {
            let row = self.rows.get(row_idx)?;
            if row.cells.is_empty() {
                lines.push(String::new());
                continue;
            }
            let content_start = selection_row_content_start(row);
            let Some(content_end) = selection_row_content_end(row, content_start) else {
                if row_idx == start_row || !row.continues_previous || lines.is_empty() {
                    lines.push(String::new());
                }
                continue;
            };

            let row_start = if row_idx == start_row {
                start
                    .column
                    .max(content_start.max(row.copy_offset))
                    .min(row.cells.len().saturating_sub(1))
            } else {
                content_start.max(row.copy_offset)
            };
            let row_end = if row_idx == end_row {
                end.column.min(content_end)
            } else {
                content_end
            };
            if row_start > row_end {
                lines.push(String::new());
                continue;
            }

            let mut text = String::new();
            for cell in row
                .cells
                .iter()
                .skip(row_start)
                .take(row_end - row_start + 1)
            {
                text.push_str(cell);
            }
            if row_idx != start_row && row.continues_previous && !lines.is_empty() {
                let continuation = text.trim_start_matches(' ');
                let current = lines.last_mut().expect("continuation has previous line");
                if !continuation.is_empty() && !current.ends_with(char::is_whitespace) {
                    current.push(' ');
                }
                current.push_str(continuation);
            } else {
                lines.push(text);
            }
        }

        Some(lines.join("\n"))
    }

    #[cfg(test)]
    pub(super) fn visible_rows(&self) -> Vec<String> {
        self.rows
            .iter()
            .skip(self.scroll_top)
            .take(usize::from(self.viewport.height))
            .map(|row| row.cells.join(""))
            .collect()
    }
}

fn selection_row_content_start(row: &TranscriptSelectionRow) -> usize {
    let mut index = usize::from(
        row.cells
            .first()
            .is_some_and(|cell| cell.as_str() == TRANSCRIPT_SELECTION_RAIL_GLYPH),
    );
    if index > 0 {
        while row
            .cells
            .get(index)
            .is_some_and(|cell| cell.as_str() == " ")
        {
            index += 1;
        }
    }
    index
}

fn selection_row_content_end(row: &TranscriptSelectionRow, content_start: usize) -> Option<usize> {
    row.cells
        .iter()
        .enumerate()
        .rev()
        .find(|(idx, cell)| *idx >= content_start && !cell.is_empty() && cell.as_str() != " ")
        .map(|(idx, _)| idx)
}

pub(super) fn with_cached_transcript_selection_snapshot<R>(
    key: TranscriptSelectionCacheKey,
    build_snapshot: impl FnOnce() -> Option<TranscriptSelectionSnapshot>,
    render: impl FnOnce(&TranscriptSelectionSnapshot) -> R,
) -> Option<R> {
    let mut render = Some(render);
    let mut build_snapshot = Some(build_snapshot);

    TRANSCRIPT_SELECTION_CACHE.with(|cache| {
        {
            let cache = cache.borrow();
            if let Some(entry) = cache.iter().find(|entry| entry.key.matches(key)) {
                let render = render.take().expect("render closure is available");
                return Some(render(&entry.snapshot));
            }
        }

        let snapshot = build_snapshot.take().expect("builder is available")()?;

        #[cfg(test)]
        TRANSCRIPT_SELECTION_CACHE_BUILD_COUNT
            .with(|count| count.set(count.get().saturating_add(1)));

        {
            let mut cache = cache.borrow_mut();
            cache.retain(|entry| {
                entry.key.app_instance_id != key.app_instance_id
                    || entry.key.render_key == key.render_key
            });
            cache.push(TranscriptSelectionCacheEntry { key, snapshot });
            if cache.len() > 8 {
                let overflow = cache.len().saturating_sub(8);
                cache.drain(0..overflow);
            }
        }

        let cache = cache.borrow();
        let entry = cache
            .iter()
            .find(|entry| entry.key.matches(key))
            .expect("cached transcript selection snapshot should be present after insertion");
        let render = render.take().expect("render closure is available");
        Some(render(&entry.snapshot))
    })
}

pub(super) fn transcript_selection_line_rows(
    line: &Line<'static>,
    width: usize,
) -> Vec<Vec<String>> {
    let mut row = Vec::new();
    let mut rows = Vec::new();

    for span in &line.spans {
        for ch in span.content.chars() {
            let display = ch.to_string();
            let cell_width = display_width(&display).max(1);
            if row.len() + cell_width > width {
                row.resize(width, " ".to_string());
                rows.push(std::mem::take(&mut row));
            }

            row.push(display);
            for _ in 1..cell_width {
                if row.len() == width {
                    rows.push(std::mem::take(&mut row));
                }
                row.push(String::new());
            }

            if row.len() == width {
                rows.push(std::mem::take(&mut row));
            }
        }
    }

    if rows.is_empty() && row.is_empty() {
        row.resize(width, " ".to_string());
        rows.push(row);
        return rows;
    }

    if !row.is_empty() {
        row.resize(width, " ".to_string());
        rows.push(row);
    }

    rows
}

pub(super) fn selection_rows_for_markdownish_text_block(
    text: &str,
    color: Color,
    prefix: &str,
    theme: &Theme,
    width: u16,
) -> Vec<TranscriptSelectionRow> {
    let base_style = Style::default().fg(color);
    let mut rows = Vec::new();

    for line in text.lines() {
        rows.extend(selection_rows_for_markdownish_line(
            line, color, prefix, base_style, theme, width,
        ));
    }

    if text.is_empty() {
        rows.extend(selection_rows_for_prefixed_wrapped_spans(
            prefix,
            base_style,
            Vec::new(),
            width,
            display_width(prefix),
        ));
    }

    rows
}

fn selection_rows_for_markdownish_line(
    line: &str,
    color: Color,
    prefix: &str,
    base_style: Style,
    theme: &Theme,
    width: u16,
) -> Vec<TranscriptSelectionRow> {
    if line.is_empty() {
        return selection_rows_for_prefixed_wrapped_spans(
            prefix,
            base_style,
            Vec::new(),
            width,
            display_width(prefix),
        );
    }

    let indent_width = line.chars().take_while(|ch| ch.is_whitespace()).count();
    let indent = " ".repeat(indent_width);
    let trimmed = line.trim_start();
    let content_width = usize::from(width)
        .saturating_sub(display_width(prefix))
        .max(1);

    if let Some(text) = markdown_heading_text(trimmed) {
        return selection_rows_for_prefixed_wrapped_spans(
            &format!("{prefix}{indent}"),
            base_style,
            parse_inline_markdown_spans(
                text,
                base_style
                    .fg(theme.text.accent)
                    .add_modifier(Modifier::BOLD),
                theme.text.accent,
                theme,
            ),
            width,
            display_width(prefix),
        );
    }

    if markdown_rule(trimmed) {
        return selection_rows_for_prefixed_wrapped_spans(
            prefix,
            base_style,
            vec![Span::styled(
                "─".repeat(content_width),
                Style::default().fg(theme.text.secondary),
            )],
            width,
            display_width(prefix),
        );
    }

    if let Some(text) = trimmed.strip_prefix("> ") {
        return selection_rows_for_prefixed_wrapped_spans(
            &format!("{prefix}{indent}▍ "),
            Style::default().fg(theme.text.secondary),
            parse_inline_markdown_spans(
                text,
                Style::default()
                    .fg(theme.text.secondary)
                    .add_modifier(Modifier::ITALIC),
                theme.text.secondary,
                theme,
            ),
            width,
            display_width(prefix),
        );
    }

    if let Some((list_prefix, text, list_style, text_style)) = markdown_list_prefix(trimmed, theme)
    {
        return selection_rows_for_prefixed_wrapped_spans(
            &format!("{prefix}{indent}{list_prefix}"),
            list_style,
            parse_inline_markdown_spans(text, text_style, color, theme),
            width,
            display_width(prefix),
        );
    }

    selection_rows_for_prefixed_wrapped_spans(
        prefix,
        base_style,
        parse_inline_markdown_spans(trimmed, base_style, color, theme),
        width,
        display_width(prefix),
    )
}

fn selection_rows_for_prefixed_wrapped_spans(
    prefix: &str,
    prefix_style: Style,
    content_spans: Vec<Span<'static>>,
    width: u16,
    copy_offset: usize,
) -> Vec<TranscriptSelectionRow> {
    let rendered_lines = if content_spans.is_empty() {
        vec![Line::from(Span::styled(prefix.to_string(), prefix_style))]
    } else {
        let prefix_width = display_width(prefix);
        let content_width = usize::from(width).saturating_sub(prefix_width).max(1);
        wrap_surface_spans(content_spans, content_width)
            .into_iter()
            .map(|row| {
                let mut spans = vec![Span::styled(prefix.to_string(), prefix_style)];
                spans.extend(row);
                Line::from(spans)
            })
            .collect::<Vec<_>>()
    };

    rendered_lines
        .into_iter()
        .enumerate()
        .map(|(idx, row)| TranscriptSelectionRow {
            cells: transcript_selection_line_rows(&row, usize::from(width))
                .into_iter()
                .next()
                .unwrap_or_else(|| vec![" ".to_string(); usize::from(width)]),
            continues_previous: idx > 0,
            copy_offset,
        })
        .collect()
}

pub(super) fn blank_selection_row(width: u16) -> TranscriptSelectionRow {
    TranscriptSelectionRow {
        cells: vec![" ".to_string(); usize::from(width.max(1))],
        continues_previous: false,
        copy_offset: 0,
    }
}

pub(super) fn lifecycle_selection_snapshot(
    surface: LifecycleSelectionSurface,
) -> Option<TranscriptSelectionSnapshot> {
    let width = usize::from(surface.viewport.width.max(1));
    let height = usize::from(surface.viewport.height);
    if height == 0 {
        return None;
    }

    let mut rows = vec![
        TranscriptSelectionRow {
            cells: vec![" ".to_string(); width],
            continues_previous: false,
            copy_offset: 0,
        };
        height
    ];

    for text in surface.text_rows {
        let rendered_rows = aligned_selection_rows_for_line(&text.line, width, text.alignment);
        let max_height = usize::from(text.max_height).min(rendered_rows.len());
        for (offset, mut row) in rendered_rows.into_iter().take(max_height).enumerate() {
            let target = text.row.saturating_add(offset);
            if target >= rows.len() {
                break;
            }
            row.continues_previous = offset > 0;
            rows[target] = row;
        }
    }

    Some(TranscriptSelectionSnapshot {
        viewport: surface.viewport,
        scroll_top: 0,
        rows,
    })
}

fn aligned_selection_rows_for_line(
    line: &Line<'static>,
    width: usize,
    alignment: Alignment,
) -> Vec<TranscriptSelectionRow> {
    let mut rows = transcript_selection_line_rows(line, width.max(1));
    let mut copy_offsets = vec![0; rows.len()];
    if !matches!(alignment, Alignment::Left) {
        for (idx, cells) in rows.iter_mut().enumerate() {
            copy_offsets[idx] = align_selection_cells(cells, alignment);
        }
    }

    rows.into_iter()
        .enumerate()
        .map(|(idx, cells)| TranscriptSelectionRow {
            cells,
            continues_previous: idx > 0,
            copy_offset: copy_offsets[idx],
        })
        .collect()
}

fn align_selection_cells(cells: &mut Vec<String>, alignment: Alignment) -> usize {
    let width = cells.len();
    let content_end = cells
        .iter()
        .enumerate()
        .rev()
        .find(|(_, cell)| !cell.is_empty() && cell.as_str() != " ")
        .map(|(idx, _)| idx.saturating_add(1))
        .unwrap_or(0);
    if content_end == 0 || content_end >= width {
        return 0;
    }

    let leading = match alignment {
        Alignment::Center => width.saturating_sub(content_end) / 2,
        Alignment::Right => width.saturating_sub(content_end),
        Alignment::Left => 0,
    };
    if leading == 0 {
        return 0;
    }

    let mut shifted = vec![" ".to_string(); width];
    let copy_len = content_end.min(width.saturating_sub(leading));
    shifted[leading..leading + copy_len].clone_from_slice(&cells[..copy_len]);
    *cells = shifted;
    leading
}

pub(super) fn render_transcript_selection(
    frame: &mut Frame,
    selection: Option<TranscriptSelection>,
    snapshot: Option<&TranscriptSelectionSnapshot>,
    area: Rect,
    theme: &Theme,
) {
    let Some(selection) = selection else {
        return;
    };
    let Some(snapshot) = snapshot else {
        return;
    };
    if snapshot.rows.is_empty() {
        return;
    }

    let (start, end) = selection.normalized();
    if start == end {
        return;
    }

    let visible_height = usize::from(area.height);
    let buffer = frame.buffer_mut();
    let max_row = snapshot.rows.len().saturating_sub(1);
    let start_row = start.row.min(max_row);
    let end_row = end.row.min(max_row);

    for local_row in 0..visible_height {
        let absolute_row = snapshot.scroll_top.saturating_add(local_row);
        if absolute_row < start_row || absolute_row > end_row {
            continue;
        }

        let row = &snapshot.rows[absolute_row];
        let row_start = if absolute_row == start_row {
            start.column.min(row.cells.len().saturating_sub(1))
        } else {
            0
        };
        let row_end = if absolute_row == end_row {
            end.column.min(row.cells.len().saturating_sub(1))
        } else {
            row.cells.len().saturating_sub(1)
        };
        if row_start > row_end {
            continue;
        }

        let y = area
            .y
            .saturating_add(u16::try_from(local_row).unwrap_or(u16::MAX));
        for column in row_start..=row_end {
            let x = area
                .x
                .saturating_add(u16::try_from(column).unwrap_or(u16::MAX));
            if x >= area.right() || y >= area.bottom() {
                continue;
            }

            let cell = &mut buffer[(x, y)];
            cell.set_fg(theme.text.inverse);
            cell.set_bg(theme.status.info);
        }
    }
}

fn rect_contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}

#[cfg(test)]
pub(crate) fn reset_transcript_selection_cache_metrics_for_test() {
    TRANSCRIPT_SELECTION_CACHE.with(|cache| cache.borrow_mut().clear());
    TRANSCRIPT_SELECTION_CACHE_BUILD_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn transcript_selection_cache_build_count_for_test() -> usize {
    TRANSCRIPT_SELECTION_CACHE_BUILD_COUNT.with(Cell::get)
}
