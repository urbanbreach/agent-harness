use std::time::{Duration, Instant};

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::{CellPoint, SelectionRange, ViewerState};

#[derive(Default)]
pub(super) struct ViewerPointer {
    last_click: Option<(Instant, CellPoint, u8)>,
    dragging: bool,
    scrollbar_dragging: bool,
}

impl ViewerState {
    pub(crate) fn pointer_point(&self, point: CellPoint) -> CellPoint {
        if !self.child {
            return point;
        }
        let row = point.row.min(self.wrapped.row_count().saturating_sub(1));
        let text = self.wrapped.row_text(row);
        if point.cell >= text.width() {
            let last = self.logical_rows(row).end - 1;
            return CellPoint::new(last, self.wrapped.row_text(last).width());
        }
        CellPoint::new(row, point.cell)
    }

    pub(super) fn copy_child_selection(&self, selection: SelectionRange) -> String {
        let (start, end) = selection.normalized();
        let mut output = String::new();
        for row in start.row..=end.row.min(self.wrapped.row_count().saturating_sub(1)) {
            if row > start.row {
                output.push_str(self.row_joiners.get(row - 1).map_or("\n", String::as_str));
            }
            let from = if row == start.row { start.cell } else { 0 };
            let to = if row == end.row { end.cell } else { usize::MAX };
            let mut cell = 0;
            for grapheme in self.wrapped.row_text(row).graphemes(true) {
                let next = cell + grapheme.width();
                if next > from && cell <= to {
                    output.push_str(grapheme);
                }
                cell = next;
            }
        }
        output
    }

    pub(crate) fn pointer_scrollbar(
        &mut self,
        mouse: crossterm::event::MouseEvent,
        body: ratatui::layout::Rect,
    ) -> bool {
        use crossterm::event::{MouseButton, MouseEventKind};
        let total = self.wrapped.row_count();
        if !self.child || total <= usize::from(body.height) {
            return false;
        }
        let hit = mouse.column >= body.right()
            && mouse.column <= body.right() + 2
            && mouse.row >= body.y
            && mouse.row < body.bottom();
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => self.pointer.scrollbar_dragging = hit,
            // Native block-viewer drag handling consumes track drags without forwarding them.
            MouseEventKind::Drag(MouseButton::Left) if self.pointer.scrollbar_dragging => {
                return true
            }
            MouseEventKind::Up(MouseButton::Left) => {
                return std::mem::take(&mut self.pointer.scrollbar_dragging);
            }
            _ => return false,
        }
        if !self.pointer.scrollbar_dragging {
            return false;
        }
        self.selection = None;
        self.visual_mode = false;
        let cell = mouse.row.saturating_sub(body.y);
        if cell == 0 {
            self.select_edge(false);
            return true;
        }
        if cell >= body.height.saturating_sub(1) {
            self.select_edge(true);
            self.following = self.running;
            return true;
        }
        let scale = total / usize::from(u16::MAX) + 1;
        let metrics = tui_scrollbar::ScrollMetrics::new(
            tui_scrollbar::ScrollLengths {
                content_len: total / scale,
                viewport_len: usize::from(body.height),
            },
            0,
            body.height,
        );
        let position = usize::from(cell) * tui_scrollbar::SUBCELL + tui_scrollbar::SUBCELL / 2;
        let offset = metrics
            .offset_for_thumb_start(position.saturating_sub(metrics.thumb_len() / 2))
            * scale;
        self.exit_follow();
        self.scroll_screen_y = None;
        self.at_end = false;
        let delta = f64::from(u32::try_from(offset).unwrap_or(u32::MAX))
            - f64::from(u32::try_from(self.scroll_top()).unwrap_or(u32::MAX));
        let _ = self.scroll_by(delta);
        self.cursor = CellPoint::new(
            self.logical_rows(self.scroll_top() + usize::from(body.height) / 2)
                .start,
            0,
        );
        true
    }

    pub(crate) fn clear_pointer_selection(&mut self) -> bool {
        if !self.child || self.visual_mode || self.selection.is_none() {
            return false;
        }
        self.selection = None;
        self.pointer = Default::default();
        true
    }

    pub(crate) fn yank_text(&mut self) -> String {
        if !self.visual_mode {
            self.selection = None;
        }
        let text = self.quote_text();
        self.visual_mode = false;
        self.selection = None;
        self.pointer = Default::default();
        text
    }

    pub(crate) fn pointer_dragging(&self) -> bool {
        self.pointer.dragging
    }

    pub(crate) fn finish_pointer_drag(&mut self) {
        self.pointer.dragging = false;
    }

    pub(crate) fn pointer_down(&mut self, point: CellPoint, now: Instant) -> Option<String> {
        self.visual_mode = false;
        self.selection = None;
        if point.row >= self.wrapped.row_count() {
            self.pointer = Default::default();
            return None;
        }
        self.exit_follow();
        self.scroll_screen_y = None;
        self.at_end = false;
        self.cursor = CellPoint::new(self.logical_rows(point.row).start, 0);
        let count = self.pointer.last_click.map_or(1, |(time, last, count)| {
            if now.saturating_duration_since(time) <= Duration::from_millis(300)
                && self.logical_rows(last.row) == self.logical_rows(point.row)
                && last.cell.abs_diff(point.cell) <= 1
            {
                count % 3 + 1
            } else {
                1
            }
        });
        self.pointer.last_click = Some((now, point, count));
        self.selection = match count {
            2 => self.pointer_word(point),
            3 => self.pointer_paragraph(point),
            _ => None,
        };
        self.pointer.dragging = self.selection.is_none();
        self.selection.and_then(|_| self.copy_selection_text().ok())
    }

    fn pointer_word(&self, point: CellPoint) -> Option<SelectionRange> {
        let rows = self.logical_rows(point.row);
        let mut cells = Vec::new();
        for row in rows {
            let mut column = 0;
            for grapheme in self.wrapped.row_text(row).graphemes(true) {
                let width = grapheme.width();
                if width == 0 {
                    continue;
                }
                let class = match grapheme.chars().next() {
                    Some(c) if c.is_whitespace() => 0,
                    Some(c) if "!\"#$%&'()*+,-./:;<=>?@[\\]^`{|}~".contains(c) => 1,
                    _ => 2,
                };
                cells.push((CellPoint::new(row, column), width, class));
                column += width;
            }
            if self
                .row_joiners
                .get(row)
                .is_some_and(|joiner| joiner == " ")
            {
                cells.push((CellPoint::new(row, column), 1, 0));
            }
        }
        let last = cells.len().checked_sub(1)?;
        let target = cells
            .iter()
            .position(|(start, width, _)| {
                start.row == point.row && (start.cell..start.cell + width).contains(&point.cell)
            })
            .unwrap_or(last);
        let class = cells[target].2;
        let mut start = target;
        let mut end = target;
        while start > 0 && cells[start - 1].2 == class {
            start -= 1;
        }
        while end < last && cells[end + 1].2 == class {
            end += 1;
        }
        Some(SelectionRange::new(
            cells[start].0,
            CellPoint::new(cells[end].0.row, cells[end].0.cell + cells[end].1 - 1),
        ))
    }

    fn pointer_paragraph(&self, point: CellPoint) -> Option<SelectionRange> {
        if self.wrapped.row_text(point.row).is_empty() {
            return None;
        }
        let mut rows = self.logical_rows(point.row);
        while rows.start > self.body_start && !self.wrapped.row_text(rows.start - 1).is_empty() {
            rows.start -= 1;
        }
        while rows.end < self.wrapped.row_count() && !self.wrapped.row_text(rows.end).is_empty() {
            rows.end += 1;
        }
        Some(SelectionRange::new(
            CellPoint::new(rows.start, 0),
            CellPoint::new(
                rows.end - 1,
                self.wrapped
                    .row_text(rows.end - 1)
                    .width()
                    .saturating_sub(1),
            ),
        ))
    }
}
