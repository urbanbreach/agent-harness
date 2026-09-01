// allow: SIZE_OK — TUI transcript rendering (indivisible view model)
use crate::UnwrapOrAbort;
use std::cell::{Cell, RefCell};

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    Frame,
};

use crate::composer_atoms::split_graphemes;
use crate::theme::Theme;

use super::ui_chrome::display_width;
use super::ui_fenced_text::{
    parse_fenced_text_blocks, parse_streaming_fenced_text_blocks, ParsedTextBlock,
};
use super::ui_lifecycle::LifecycleSelectionSurface;
use super::ui_markdown::{
    markdown_heading_text, markdown_list_prefix, markdown_rule, parse_inline_markdown,
    ParsedInlineMarkdown,
};
use super::ui_markdown_table::{try_render_markdown_table_block, TableLinkRun};
use super::ui_transcript_mermaid::is_mermaid_language;
use super::ui_transcript_surface::{
    wrap_surface_spans, wrap_surface_spans_with_links, SurfaceLinkRun,
};

const TRANSCRIPT_SELECTION_RAIL_GLYPH: &str = " ";

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectionRow {
    pub line_index: usize,
    pub start_cell: usize,
    pub end_cell: usize,
    pub links: Vec<TranscriptSelectionLink>,
}

impl SelectionRow {
    fn has_content(&self) -> bool {
        self.end_cell >= self.start_cell
    }
}

#[derive(Debug, Clone)]
pub(super) struct TranscriptSelectionSnapshot {
    pub(super) viewport: Rect,
    pub(super) visible_rows: Vec<usize>,
    pub(super) rows: Vec<SelectionRow>,
    pub(super) line_texts: Vec<String>,
    pub(super) continues_previous: Vec<bool>,
    pub(super) row_width: usize,
    pub(super) resolved_selection: Cell<Option<TranscriptSelection>>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptSelectionLink {
    pub(super) start_cell: usize,
    pub(super) end_cell: usize,
    pub(super) destination: String,
}

#[derive(Debug, Clone)]
pub(super) struct TranscriptSelectionRow {
    pub(super) cells: Vec<String>,
    pub(super) continues_previous: bool,
    pub(super) copy_offset: usize,
    pub(super) links: Vec<TranscriptSelectionLink>,
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
            row: *self
                .visible_rows
                .get(usize::from(row.saturating_sub(self.viewport.y)))?,
            column: usize::from(column.saturating_sub(self.viewport.x)),
        })
    }

    pub(super) fn selection_text(&self, selection: TranscriptSelection) -> Option<String> {
        self.selection_text_inner(selection, false)
    }

    pub(super) fn selection_text_with_destinations(
        &self,
        selection: TranscriptSelection,
    ) -> Option<String> {
        self.selection_text_inner(selection, true)
    }

    fn selection_text_inner(
        &self,
        _selection: TranscriptSelection,
        include_destinations: bool,
    ) -> Option<String> {
        let selection = self.resolved_selection.get()?;
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
        let mut destinations = Vec::new();
        for row_idx in start_row..=end_row {
            let row = self.rows.get(row_idx)?;
            let continues_previous = self
                .continues_previous
                .get(row_idx)
                .copied()
                .unwrap_or(false);

            if !row.has_content() {
                if row_idx == start_row || !continues_previous || lines.is_empty() {
                    lines.push(String::new());
                }
                continue;
            }

            let line_text = self
                .line_texts
                .get(row.line_index)
                .map(|s| s.as_str())
                .unwrap_or("");

            let row_start = if row_idx == start_row {
                start
                    .column
                    .max(row.start_cell)
                    .min(self.row_width.saturating_sub(1))
            } else {
                row.start_cell
            };
            let row_end = if row_idx == end_row {
                end.column.min(row.end_cell)
            } else {
                row.end_cell
            };
            if row_start > row_end {
                lines.push(String::new());
                continue;
            }

            for link in &row.links {
                if link.start_cell <= row_end
                    && link.end_cell > row_start
                    && !destinations.contains(&link.destination)
                {
                    destinations.push(link.destination.clone());
                }
            }
            let text = extract_text_by_display_columns(line_text, row_start, row_end);
            if row_idx != start_row && continues_previous && !lines.is_empty() {
                let continuation = text.trim_start_matches(' ');
                let current = lines.last_mut().unwrap_or_abort();
                if !continuation.is_empty() && !current.ends_with(char::is_whitespace) {
                    current.push(' ');
                }
                current.push_str(continuation);
            } else {
                lines.push(text);
            }
        }

        let mut text = lines.join("\n");
        if include_destinations && !destinations.is_empty() {
            text.push_str("\n\nLinks:\n");
            text.push_str(&destinations.join("\n"));
        }
        Some(text)
    }

    #[cfg(test)]
    pub(super) fn visible_rows(&self) -> Vec<String> {
        self.visible_rows
            .iter()
            .map(|row_index| {
                let row = self.rows.get(*row_index);
                self.line_texts
                    .get(row.map_or(0, |row| row.line_index))
                    .cloned()
                    .unwrap_or_default()
            })
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

pub(super) fn compact_selection_row(
    row: &TranscriptSelectionRow,
    line_index: usize,
) -> SelectionRow {
    let content_start = selection_row_content_start(row);
    let start_cell = content_start.max(row.copy_offset);
    match selection_row_content_end(row, content_start) {
        Some(end_cell) => SelectionRow {
            line_index,
            start_cell,
            end_cell,
            links: row.links.clone(),
        },
        None => SelectionRow {
            line_index,
            start_cell: 1,
            end_cell: 0,
            links: Vec::new(),
        },
    }
}

pub(super) fn selection_row_line_text(row: &TranscriptSelectionRow) -> String {
    row.cells.join("")
}

fn extract_text_by_display_columns(text: &str, start_col: usize, end_col: usize) -> String {
    let mut result = String::new();
    let mut cell = 0usize;
    for cluster in split_graphemes(text) {
        let cluster_width = usize::from(cluster.display_width());
        let cluster_end = cell.saturating_add(cluster_width);
        if cell <= end_col && cluster_end > start_col {
            result.push_str(cluster.as_str());
        }
        cell = cluster_end;
        if cell > end_col {
            break;
        }
    }
    result
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
                let render = render.take().unwrap_or_abort();
                return Some(render(&entry.snapshot));
            }
        }

        let snapshot = build_snapshot.take().unwrap_or_abort()()?;

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
            .unwrap_or_abort();
        let render = render.take().unwrap_or_abort();
        Some(render(&entry.snapshot))
    })
}

pub(super) fn transcript_selection_line_rows(
    line: &Line<'static>,
    width: usize,
) -> Vec<Vec<String>> {
    let mut row = Vec::<String>::new();
    let mut rows = Vec::new();

    for span in &line.spans {
        for cluster in split_graphemes(span.content.as_ref()) {
            let cell_width = usize::from(cluster.display_width());
            if cell_width == 0 {
                if let Some(cell) = row.iter_mut().rev().find(|cell| !cell.is_empty()) {
                    cell.push_str(cluster.as_str());
                }
                continue;
            }
            if row.len() + cell_width > width {
                row.resize(width, " ".to_string());
                rows.push(std::mem::take(&mut row));
            }

            row.push(cluster.as_str().to_string());
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

pub(super) fn selection_rows_for_rendered_line(
    line: &Line<'static>,
    width: u16,
) -> Vec<TranscriptSelectionRow> {
    transcript_selection_line_rows(line, usize::from(width.max(1)))
        .into_iter()
        .enumerate()
        .map(|(idx, cells)| TranscriptSelectionRow {
            cells,
            continues_previous: idx > 0,
            copy_offset: 0,
            links: Vec::new(),
        })
        .collect()
}

pub(super) fn selection_rows_for_markdownish_text_block(
    text: &str,
    color: Color,
    prefix: &str,
    theme: &Theme,
    width: u16,
) -> Vec<TranscriptSelectionRow> {
    let base_style = Style::default().fg(color);
    let source_rows = text.lines().collect::<Vec<_>>();
    let mut rows = Vec::new();
    let mut index = 0;

    while let Some(line) = source_rows.get(index).copied() {
        if let Some((table_lines, consumed, table_links)) =
            try_render_markdown_table_block(&source_rows[index..], color, prefix, theme, width)
        {
            rows.extend(selection_rows_for_rendered_table_lines(
                table_lines,
                width,
                display_width(prefix),
                &table_links,
            ));
            index += consumed;
            continue;
        }

        rows.extend(selection_rows_for_markdownish_line(
            line, color, prefix, base_style, theme, width,
        ));
        index += 1;
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

pub(super) fn selection_rows_for_rich_text_block(
    text: &str,
    color: Color,
    prefix: &str,
    theme: &Theme,
    width: u16,
    is_streaming: bool,
) -> Option<Vec<TranscriptSelectionRow>> {
    let Some(blocks) = text
        .contains("```")
        .then(|| {
            if is_streaming {
                Some(parse_streaming_fenced_text_blocks(text))
            } else {
                parse_fenced_text_blocks(text)
            }
        })
        .flatten()
    else {
        return Some(selection_rows_for_markdownish_text_block(
            text, color, prefix, theme, width,
        ));
    };

    let base_style = Style::default().fg(color);
    let copy_offset = display_width(prefix);
    let mut rows = Vec::new();
    for block in blocks {
        match block {
            ParsedTextBlock::Plain(plain) => {
                rows.extend(selection_rows_for_markdownish_text_block(
                    &plain, color, prefix, theme, width,
                ));
                if rows.last().is_some_and(|row| !selection_row_is_blank(row)) {
                    rows.push(blank_selection_row(width));
                }
            }
            ParsedTextBlock::Code { language, body, .. } => {
                if is_mermaid_language(language.as_deref())
                    || matches!(language.as_deref(), Some("diff" | "patch"))
                {
                    return None;
                }
                if rows.last().is_some_and(|row| !selection_row_is_blank(row)) {
                    rows.push(blank_selection_row(width));
                }
                for line in body.lines() {
                    rows.extend(selection_rows_for_prefixed_wrapped_spans(
                        prefix,
                        base_style,
                        vec![Span::styled(line.to_string(), base_style)],
                        width,
                        copy_offset,
                    ));
                }
                rows.push(blank_selection_row(width));
                rows.push(blank_selection_row(width));
            }
        }
    }
    Some(rows)
}

fn selection_row_is_blank(row: &TranscriptSelectionRow) -> bool {
    row.cells
        .iter()
        .all(|cell| cell.is_empty() || cell.chars().all(char::is_whitespace))
}

fn selection_rows_for_rendered_table_lines(
    lines: Vec<Line<'static>>,
    width: u16,
    copy_offset: usize,
    links: &[TableLinkRun],
) -> Vec<TranscriptSelectionRow> {
    lines
        .into_iter()
        .enumerate()
        .flat_map(|(line_index, line)| {
            transcript_selection_line_rows(&line, usize::from(width.max(1)))
                .into_iter()
                .enumerate()
                .map(move |(wrapped_index, cells)| TranscriptSelectionRow {
                    cells,
                    continues_previous: wrapped_index > 0,
                    copy_offset,
                    links: links
                        .iter()
                        .filter(|link| link.row == line_index)
                        .map(|link| TranscriptSelectionLink {
                            start_cell: link.start_cell,
                            end_cell: link.end_cell,
                            destination: link.destination.clone(),
                        })
                        .collect(),
                })
        })
        .collect()
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
        return selection_rows_for_prefixed_wrapped_inline(
            &format!("{prefix}{indent}"),
            base_style,
            parse_inline_markdown(
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
        return selection_rows_for_prefixed_wrapped_inline(
            &format!("{prefix}{indent}▍ "),
            Style::default().fg(theme.text.secondary),
            parse_inline_markdown(
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
        return selection_rows_for_prefixed_wrapped_inline(
            &format!("{prefix}{indent}{list_prefix}"),
            list_style,
            parse_inline_markdown(text, text_style, color, theme),
            width,
            display_width(prefix),
        );
    }

    selection_rows_for_prefixed_wrapped_inline(
        prefix,
        base_style,
        parse_inline_markdown(trimmed, base_style, color, theme),
        width,
        display_width(prefix),
    )
}

fn selection_rows_for_prefixed_wrapped_inline(
    prefix: &str,
    prefix_style: Style,
    parsed: ParsedInlineMarkdown,
    width: u16,
    copy_offset: usize,
) -> Vec<TranscriptSelectionRow> {
    let prefix_width = display_width(prefix);
    let content_width = usize::from(width).saturating_sub(prefix_width).max(1);
    let source_links = parsed
        .links
        .into_iter()
        .map(|link| SurfaceLinkRun {
            start_cell: link.start_cell,
            end_cell: link.end_cell,
            destination: link.destination,
        })
        .collect::<Vec<_>>();
    wrap_surface_spans_with_links(parsed.spans, &source_links, content_width)
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            let mut spans = vec![Span::styled(prefix.to_string(), prefix_style)];
            spans.extend(row.spans);
            TranscriptSelectionRow {
                cells: transcript_selection_line_rows(&Line::from(spans), usize::from(width))
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| vec![" ".to_string(); usize::from(width)]),
                continues_previous: index > 0,
                copy_offset,
                links: row
                    .links
                    .into_iter()
                    .map(|link| TranscriptSelectionLink {
                        start_cell: prefix_width.saturating_add(link.start_cell),
                        end_cell: prefix_width.saturating_add(link.end_cell),
                        destination: link.destination,
                    })
                    .collect(),
            }
        })
        .collect()
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
            links: Vec::new(),
        })
        .collect()
}

pub(super) fn blank_selection_row(width: u16) -> TranscriptSelectionRow {
    TranscriptSelectionRow {
        cells: vec![" ".to_string(); usize::from(width.max(1))],
        continues_previous: false,
        copy_offset: 0,
        links: Vec::new(),
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

    let mut rows: Vec<SelectionRow> = (0..height)
        .map(|line_index| SelectionRow {
            line_index,
            start_cell: 1,
            end_cell: 0,
            links: Vec::new(),
        })
        .collect();
    let mut line_texts = vec![" ".repeat(width); height];
    let mut continues_previous = vec![false; height];

    for text in surface.text_rows {
        let rendered_rows = aligned_selection_rows_for_line(&text.line, width, text.alignment);
        let max_height = usize::from(text.max_height).min(rendered_rows.len());
        for (offset, row) in rendered_rows.into_iter().take(max_height).enumerate() {
            let target = text.row.saturating_add(offset);
            if target >= rows.len() {
                break;
            }
            rows[target] = compact_selection_row(&row, target);
            line_texts[target] = selection_row_line_text(&row);
            continues_previous[target] = offset > 0;
        }
    }

    Some(TranscriptSelectionSnapshot {
        viewport: surface.viewport,
        visible_rows: (0..height).collect(),
        rows,
        line_texts,
        continues_previous,
        row_width: width,
        resolved_selection: Cell::new(None),
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
            links: Vec::new(),
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
    let Some(_selection) = selection else {
        return;
    };
    let Some(snapshot) = snapshot else {
        return;
    };
    let Some(selection) = snapshot.resolved_selection.get() else {
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

    for (local_row, absolute_row) in snapshot
        .visible_rows
        .iter()
        .copied()
        .take(visible_height)
        .enumerate()
    {
        if absolute_row < start_row || absolute_row > end_row {
            continue;
        }

        let row_start = if absolute_row == start_row {
            start.column.min(snapshot.row_width.saturating_sub(1))
        } else {
            0
        };
        let row_end = if absolute_row == end_row {
            end.column.min(snapshot.row_width.saturating_sub(1))
        } else {
            snapshot.row_width.saturating_sub(1)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fenced_selection_rows_preserve_logical_code_lines_through_rewrap() {
        // arrange
        // Given: a fenced assistant body whose code line wraps at a narrow width.
        let Some(rows) = selection_rows_for_rich_text_block(
            "```rust\nlet value = a_very_long_identifier;\n```",
            Color::White,
            "  ",
            &Theme::default(),
            16,
            false,
        ) else {
            panic!("ordinary code must remain selectable");
        };

        // When: the selection model records the rendered rows.
        let rendered = rows
            .iter()
            .map(selection_row_line_text)
            .collect::<Vec<_>>()
            .join("\n");

        // act
        // Then: only painted code is selectable and wrapped rows retain one logical line.
        // assert
        assert!(!rendered.contains("```rust"));
        assert!(rendered.contains("let value"));
        assert!(rows.iter().any(|row| row.continues_previous));
    }

    #[test]
    fn open_fence_selection_rows_match_streaming_code_body() {
        // arrange
        // act
        let Some(rows) = selection_rows_for_rich_text_block(
            "Before\n```rust\nlet value = 42;",
            Color::White,
            "  ",
            &Theme::default(),
            24,
            true,
        ) else {
            panic!("streaming code must remain selectable");
        };
        let rendered = rows
            .iter()
            .map(selection_row_line_text)
            .collect::<Vec<_>>()
            .join("\n");

        // assert
        assert!(rendered.contains("Before"));
        assert!(rendered.contains("let value = 42;"));
        assert!(!rendered.contains("```rust"));
    }

    #[test]
    fn markdown_selection_copy_retains_safe_destination_metadata() {
        // Given: a production markdown selection row containing a labeled link.
        let rows = selection_rows_for_markdownish_text_block(
            "Read [docs](https://example.com/docs)",
            Color::White,
            "  ",
            &Theme::default(),
            40,
        );
        let compact = rows
            .iter()
            .enumerate()
            .map(|(index, row)| compact_selection_row(row, index))
            .collect::<Vec<_>>();
        let snapshot = TranscriptSelectionSnapshot {
            viewport: Rect::new(0, 0, 40, 1),
            visible_rows: vec![0],
            line_texts: rows.iter().map(selection_row_line_text).collect(),
            continues_previous: vec![false],
            rows: compact,
            row_width: 40,
            resolved_selection: Cell::new(Some(TranscriptSelection {
                anchor: TranscriptSelectionCell { row: 0, column: 2 },
                focus: TranscriptSelectionCell { row: 0, column: 10 },
            })),
        };

        // When: selected cells are copied.
        let copied = snapshot
            .selection_text_with_destinations(TranscriptSelection {
                anchor: TranscriptSelectionCell { row: 0, column: 2 },
                focus: TranscriptSelectionCell { row: 0, column: 10 },
            })
            .expect("selected text");

        // Then: visible text and the safe destination survive together.
        assert_eq!(copied, "Read docs\n\nLinks:\nhttps://example.com/docs");
    }

    #[test]
    fn destination_export_includes_only_half_open_runs_intersecting_selection() {
        // Given: one safe link occupying display cells [3, 7).
        let row = TranscriptSelectionRow {
            cells: "aa link zz".chars().map(|ch| ch.to_string()).collect(),
            continues_previous: false,
            copy_offset: 0,
            links: vec![TranscriptSelectionLink {
                start_cell: 3,
                end_cell: 7,
                destination: "https://example.com/link".to_string(),
            }],
        };
        let snapshot_for = |anchor, focus| TranscriptSelectionSnapshot {
            viewport: Rect::new(0, 0, 10, 1),
            visible_rows: vec![0],
            rows: vec![compact_selection_row(&row, 0)],
            line_texts: vec![selection_row_line_text(&row)],
            continues_previous: vec![false],
            row_width: 10,
            resolved_selection: Cell::new(Some(TranscriptSelection {
                anchor: TranscriptSelectionCell {
                    row: 0,
                    column: anchor,
                },
                focus: TranscriptSelectionCell {
                    row: 0,
                    column: focus,
                },
            })),
        };

        // When: selections land before, after, and on the final linked cell.
        let before = snapshot_for(0, 1)
            .selection_text_with_destinations(TranscriptSelection {
                anchor: TranscriptSelectionCell { row: 0, column: 0 },
                focus: TranscriptSelectionCell { row: 0, column: 1 },
            })
            .expect("before text");
        let after = snapshot_for(8, 9)
            .selection_text_with_destinations(TranscriptSelection {
                anchor: TranscriptSelectionCell { row: 0, column: 8 },
                focus: TranscriptSelectionCell { row: 0, column: 9 },
            })
            .expect("after text");
        let boundary = snapshot_for(6, 7)
            .selection_text_with_destinations(TranscriptSelection {
                anchor: TranscriptSelectionCell { row: 0, column: 6 },
                focus: TranscriptSelectionCell { row: 0, column: 7 },
            })
            .expect("boundary text");

        // Then: only the exact half-open overlap exports the destination.
        assert!(!before.contains("Links:") && !after.contains("Links:"));
        assert!(boundary.ends_with("Links:\nhttps://example.com/link"));
    }

    #[test]
    fn inline_link_ranges_survive_repeated_labels_wrapping_and_wide_graphemes() {
        // Given: plain duplicate text, repeated linked labels, whitespace, CJK, and a ZWJ emoji.
        let rows = selection_rows_for_markdownish_text_block(
            "same [same](https://example.com/one) 👩‍💻中 [same](https://example.com/two) [two words](https://example.com/words)",
            Color::White,
            "",
            &Theme::default(),
            7,
        );

        // When: rendered row-local link runs are inspected.
        let links = rows
            .iter()
            .flat_map(|row| row.links.iter().map(move |link| (row, link)))
            .collect::<Vec<_>>();

        // Then: each URL has its own exact non-empty run, including both words when wrapped.
        assert_eq!(
            links
                .iter()
                .map(|(_, link)| link.destination.as_str())
                .collect::<Vec<_>>(),
            vec![
                "https://example.com/one",
                "https://example.com/two",
                "https://example.com/words",
                "https://example.com/words",
            ]
        );
        for (row, link) in links {
            assert!(link.start_cell < link.end_cell);
            assert!(link.end_cell <= row.cells.len());
        }
    }

    #[test]
    fn selection_cells_treat_combining_and_zwj_sequences_as_single_graphemes() {
        // Given: combining text and a ZWJ emoji before a trailing link-like label.
        let line = Line::from("e\u{301}👩‍💻x");

        // When: the rendered line is projected into terminal cells.
        let rows = transcript_selection_line_rows(&line, 8);

        // Then: each grapheme starts in one cell and wide continuation cells stay empty.
        assert_eq!(rows[0][0], "e\u{301}");
        assert_eq!(rows[0][1], "👩‍💻");
        assert_eq!(rows[0][2], "");
        assert_eq!(rows[0][3], "x");
        assert_eq!(extract_text_by_display_columns("e\u{301}👩‍💻x", 1, 2), "👩‍💻");
    }

    #[test]
    fn streaming_and_settled_rows_preserve_link_metadata_before_open_fence() {
        // Given: visible linked prose before an unfinished code fence.
        let theme = Theme::default();
        let streaming = selection_rows_for_rich_text_block(
            "See [docs](https://example.com/docs)\n```rust\nfn main() {}",
            Color::White,
            "  ",
            &theme,
            40,
            true,
        )
        .expect("streaming rows");

        // When: the closing fence settles the same document.
        let settled = selection_rows_for_rich_text_block(
            "See [docs](https://example.com/docs)\n```rust\nfn main() {}\n```",
            Color::White,
            "  ",
            &theme,
            40,
            false,
        )
        .expect("settled rows");

        // Then: already-visible link geometry and destination stay stable.
        assert_eq!(streaming[0].links, settled[0].links);
        assert_eq!(
            streaming[0].links[0].destination,
            "https://example.com/docs"
        );
    }

    #[test]
    fn transformed_fences_fail_closed_for_semantic_selection() {
        // arrange
        // act
        for source in [
            "```mermaid\ngraph TD\nA --> B\n```",
            "```diff\n-old\n+new\n```",
        ] {
            // assert
            assert!(selection_rows_for_rich_text_block(
                source,
                Color::White,
                "  ",
                &Theme::default(),
                40,
                false,
            )
            .is_none());
        }
    }

    #[test]
    fn unresolved_semantic_selection_does_not_fall_back_to_stale_cells() {
        // arrange
        // Given: a snapshot whose anchored surface disappeared during reflow.
        let snapshot = TranscriptSelectionSnapshot {
            viewport: Rect::new(0, 0, 5, 1),
            visible_rows: vec![0],
            rows: vec![SelectionRow {
                line_index: 0,
                start_cell: 0,
                end_cell: 4,
                links: Vec::new(),
            }],
            line_texts: vec!["stale".to_string()],
            continues_previous: vec![false],
            row_width: 5,
            resolved_selection: Cell::new(None),
        };
        let stale_selection = TranscriptSelection {
            anchor: TranscriptSelectionCell { row: 0, column: 0 },
            focus: TranscriptSelectionCell { row: 0, column: 4 },
        };

        // When: copy resolves selection text from the reflowed snapshot.
        let text = snapshot.selection_text(stale_selection);

        // act
        // Then: unresolved semantic endpoints fail closed instead of selecting new content.
        // assert
        assert_eq!(text, None);
    }
}
