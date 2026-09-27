// allow: SIZE_OK — TUI transcript rendering (indivisible view model)
use crate::UnwrapOrAbort;

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    Frame,
};

use crate::theme::Theme;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::ui_chrome::display_width;
use super::ui_fenced_text::{
    parse_fenced_text_blocks, parse_streaming_fenced_text_blocks, ParsedTextBlock,
};
use super::ui_lifecycle::LifecycleSelectionSurface;
use super::ui_markdown::{
    markdown_display_source, markdown_heading_text, markdown_list_prefix, markdown_quote_prefix,
    markdown_rule, parse_inline_markdown, ParsedInlineMarkdown,
};
use super::ui_markdown_table::{try_render_markdown_table_block, TableLinkRun};
use super::ui_transcript_mermaid::is_mermaid_language;
use super::ui_transcript_surface::{
    wrap_surface_spans, wrap_surface_spans_with_links, SurfaceLinkRun,
};

#[path = "ui_transcript_selection/markdown.rs"]
mod markdown;
#[path = "ui_transcript_selection/rows.rs"]
mod rows;
pub(super) use markdown::{
    selection_rows_for_markdownish_text_block, selection_rows_for_rich_text_block,
};
use rows::aligned_selection_rows_for_line;
pub(super) use rows::{selection_rows_for_rendered_line, surface_selection_rows};

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
    pub text: String,
    pub width: usize,
    pub continues_previous: bool,
    pub copy_joiner: Option<String>,
    pub start_cell: usize,
    pub end_cell: usize,
    pub links: Vec<TranscriptSelectionLink>,
}

impl SelectionRow {
    fn exclude_prefix(&mut self, offset: usize) {
        if self.has_content() {
            self.start_cell = self.start_cell.max(offset);
        }
    }

    fn pad_to(&mut self, width: usize) {
        self.text
            .extend(std::iter::repeat_n(' ', width.saturating_sub(self.width)));
        self.width = self.width.max(width);
    }

    fn has_content(&self) -> bool {
        self.end_cell >= self.start_cell
    }
}

#[derive(Debug, Clone)]
pub(super) struct TranscriptSelectionSnapshot {
    pub(super) viewport: Rect,
    pub(super) visible_rows: Vec<Option<usize>>,
    pub(super) rows: Vec<SelectionRow>,
    pub(super) total_rows: usize,
    pub(super) row_width: usize,
    pub(super) resolved_selection: Option<TranscriptSelection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptSelectionLink {
    pub(super) start_cell: usize,
    pub(super) end_cell: usize,
    pub(super) destination: String,
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
                .visible_rows
                .get(usize::from(row.saturating_sub(self.viewport.y)))
                .copied()
                .flatten()?,
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
        let selection = self.resolved_selection?;
        let (start, end) = selection.normalized();
        if start == end || self.rows.is_empty() {
            return None;
        }

        let last_row = self.total_rows.saturating_sub(1);
        let start_row = start.row.min(last_row);
        let end_row = end.row.min(last_row);
        if start_row > end_row {
            return None;
        }

        let mut lines = Vec::new();
        let mut destinations = Vec::new();
        for row_idx in start_row..=end_row {
            let index = self
                .rows
                .binary_search_by_key(&row_idx, |row| row.line_index)
                .ok()?;
            let row = &self.rows[index];
            let continues_previous = row.continues_previous;

            if !row.has_content() {
                if row_idx == start_row || !continues_previous || lines.is_empty() {
                    lines.push(String::new());
                }
                continue;
            }

            let line_text = &row.text;

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
                if let Some(joiner) = row.copy_joiner.as_deref() {
                    current.push_str(joiner);
                } else if !continuation.is_empty() && !current.ends_with(char::is_whitespace) {
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
                row_index
                    .and_then(|row_index| {
                        self.rows
                            .binary_search_by_key(&row_index, |row| row.line_index)
                            .ok()
                    })
                    .map(|index| self.rows[index].text.clone())
                    .unwrap_or_default()
            })
            .collect()
    }
}

fn extract_text_by_display_columns(text: &str, start_col: usize, end_col: usize) -> String {
    let mut result = String::new();
    let mut cell = 0usize;
    for cluster in text.graphemes(true) {
        let cluster_width = cluster.width();
        let cluster_end = cell.saturating_add(cluster_width);
        if cell <= end_col && cluster_end > start_col {
            result.push_str(cluster);
        }
        cell = cluster_end;
        if cell > end_col {
            break;
        }
    }
    result
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
            text: " ".repeat(width),
            width,
            continues_previous: false,
            copy_joiner: None,
            start_cell: 1,
            end_cell: 0,
            links: Vec::new(),
        })
        .collect();

    for text in surface.text_rows {
        let rendered_rows = aligned_selection_rows_for_line(&text.line, width, text.alignment);
        let max_height = usize::from(text.max_height).min(rendered_rows.len());
        for (offset, row) in rendered_rows.into_iter().take(max_height).enumerate() {
            let target = text.row.saturating_add(offset);
            if target >= rows.len() {
                break;
            }
            rows[target] = row;
            rows[target].line_index = target;
            rows[target].continues_previous = offset > 0;
        }
    }

    Some(TranscriptSelectionSnapshot {
        viewport: surface.viewport,
        visible_rows: (0..height).map(Some).collect(),
        rows,
        total_rows: height,
        row_width: width,
        resolved_selection: None,
    })
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
    let Some(selection) = snapshot.resolved_selection else {
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
    let max_row = snapshot.total_rows.saturating_sub(1);
    let start_row = start.row.min(max_row);
    let end_row = end.row.min(max_row);

    for (local_row, absolute_row) in snapshot
        .visible_rows
        .iter()
        .copied()
        .take(visible_height)
        .enumerate()
    {
        let Some(absolute_row) = absolute_row else {
            continue;
        };
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
#[path = "ui_transcript_selection_grammar_tests.rs"]
mod grammar_tests;

#[cfg(test)]
#[path = "ui_transcript_selection/tests.rs"]
mod tests;

pub(super) fn blank_selection_row() -> SelectionRow {
    SelectionRow {
        line_index: 0,
        text: String::new(),
        width: 0,
        continues_previous: false,
        copy_joiner: None,
        start_cell: 1,
        end_cell: 0,
        links: Vec::new(),
    }
}
