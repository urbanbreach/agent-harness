use crate::composer_atoms::{AtomBuffer, AtomCursor, AtomKind};
use crate::shell_geometry::{cursor_for, layout_for_rect, ShellState};
use ratatui::layout::{Position, Rect};

/// Read-only inputs used to anchor a completion dropdown to the composer.
pub struct CompletionGeometryInput<'a> {
    pub viewport: Rect,
    pub state: ShellState,
    pub buffer: &'a AtomBuffer,
    pub cursor: AtomCursor,
    pub item_count: usize,
    pub max_rows: u16,
}

/// Geometry for the visible dropdown and its composer anchor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompletionDropdownGeometry {
    pub rect: Rect,
    pub anchor: Position,
    pub wrapped_lines: usize,
}

/// Calculates dropdown placement after composer wrapping and viewport clamping.
pub struct ShellCompletionGeometry;

impl ShellCompletionGeometry {
    pub fn calculate(input: &CompletionGeometryInput<'_>) -> CompletionDropdownGeometry {
        let regions = layout_for_rect(input.viewport, input.state);
        let wrap_width = regions.composer.width.saturating_sub(2).max(1);
        let wrapped_lines = input.buffer.wrap(wrap_width).len();
        let cursor_chars = input
            .buffer
            .atoms()
            .iter()
            .take(input.cursor.insertion_index())
            .map(|atom| match &atom.kind {
                AtomKind::Text(cluster) => cluster.as_str().chars().count(),
                AtomKind::Newline => 1,
                AtomKind::FileMention(_) | AtomKind::Attachment(_) => 0,
            })
            .sum();
        let anchor_tuple =
            cursor_for(&regions, input.state, &input.buffer.text(), cursor_chars).position;
        let anchor = Position::new(anchor_tuple.0, anchor_tuple.1);
        let width = input
            .viewport
            .width
            .min(regions.composer.width)
            .saturating_sub(2)
            .clamp(1, 32);
        let height_limit = usize::from(input.max_rows.max(1))
            .min(usize::from(input.viewport.height.max(1)))
            .min(usize::from(u16::MAX));
        let height = u16::try_from(input.item_count.max(1).min(height_limit)).unwrap_or(u16::MAX);
        let x = anchor.x.min(input.viewport.right().saturating_sub(width));
        let y = anchor
            .y
            .saturating_sub(height)
            .min(input.viewport.bottom().saturating_sub(height));
        CompletionDropdownGeometry {
            rect: Rect::new(x, y, width, height),
            anchor,
            wrapped_lines,
        }
    }
}
