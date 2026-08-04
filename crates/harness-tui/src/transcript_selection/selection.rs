use super::grapheme::segment;
use super::keyboard;
use super::selection_types::{
    Autoscroll, CellPoint, DragResult, Grapheme, NavigationKey, SelectionError, SelectionMode,
    SelectionRange, Viewport,
};

#[derive(Debug, Clone)]
pub(super) struct WrappedRow {
    pub(super) graphemes: Vec<Grapheme>,
    pub(super) hard_break_before: bool,
    pub(super) width: usize,
}

#[derive(Debug, Clone)]
pub struct WrappedText {
    rows: Vec<WrappedRow>,
}

impl WrappedText {
    pub fn new(text: &str, width: usize) -> Result<Self, SelectionError> {
        if width == 0 {
            return Err(SelectionError::ZeroWidth);
        }
        let mut rows = Vec::new();
        for (line_index, line) in text.split('\n').enumerate() {
            let clusters = segment(line);
            if clusters.is_empty() {
                rows.push(WrappedRow {
                    graphemes: Vec::new(),
                    hard_break_before: line_index > 0,
                    width: 0,
                });
                continue;
            }
            let mut current = Vec::new();
            let mut current_width = 0;
            let mut first_row = true;
            for cluster in clusters {
                let cluster_width = cluster.range.cell_range.len();
                if current_width + cluster_width > width && !current.is_empty() {
                    rows.push(WrappedRow {
                        graphemes: std::mem::take(&mut current),
                        hard_break_before: line_index > 0 && first_row,
                        width: current_width,
                    });
                    first_row = false;
                    current_width = 0;
                    if cluster.text.chars().all(char::is_whitespace) {
                        continue;
                    }
                }
                let mut cluster = cluster;
                cluster.range.cell_range = current_width..current_width + cluster_width;
                cluster.end = CellPoint::new(0, current_width + cluster_width.saturating_sub(1));
                current_width += cluster_width;
                current.push(cluster);
            }
            rows.push(WrappedRow {
                graphemes: current,
                hard_break_before: line_index > 0 && first_row,
                width: current_width,
            });
        }
        Ok(Self { rows })
    }

    pub fn grapheme_at(&self, point: CellPoint) -> Option<&Grapheme> {
        self.rows
            .get(point.row)?
            .graphemes
            .iter()
            .find(|cluster| cluster.range.cell_range.contains(&point.cell))
    }

    pub const fn drag(&self, anchor: CellPoint, focus: CellPoint) -> SelectionRange {
        SelectionRange::new(anchor, focus)
    }

    pub fn drag_with_autoscroll(
        &self,
        _anchor: CellPoint,
        focus: CellPoint,
        viewport: Viewport,
    ) -> DragResult {
        let last_visible = viewport
            .top
            .saturating_add(viewport.height.saturating_sub(1));
        let lines = if focus.row < viewport.top {
            -i32::try_from(viewport.top - focus.row).unwrap_or(i32::MAX)
        } else if focus.row > last_visible {
            i32::try_from(focus.row - last_visible).unwrap_or(i32::MAX)
        } else {
            0
        };
        DragResult {
            focus: self.snap_point(focus),
            autoscroll: Autoscroll { lines },
        }
    }

    pub fn select(&self, point: CellPoint, mode: SelectionMode) -> SelectionRange {
        let row = point.row.min(self.rows.len().saturating_sub(1));
        let Some(current) = self.rows.get(row) else {
            return SelectionRange::new(point, point);
        };
        match mode {
            SelectionMode::Character => SelectionRange::new(point, point),
            SelectionMode::Line => SelectionRange::new(
                CellPoint::new(row, 0),
                CellPoint::new(row, current.width.saturating_sub(1)),
            ),
            SelectionMode::Word => self.word_selection(row, point.cell),
        }
    }

    pub fn copy(&self, selection: SelectionRange) -> Result<String, SelectionError> {
        if self.rows.is_empty() {
            return Err(SelectionError::EmptyText);
        }
        let (start, end) = selection.normalized();
        let first_row = start.row.min(self.rows.len().saturating_sub(1));
        let last_row = end.row.min(self.rows.len().saturating_sub(1));
        let mut output = String::new();
        for row_index in first_row..=last_row {
            let row = self
                .rows
                .get(row_index)
                .ok_or(SelectionError::InvalidPoint)?;
            let mut row_output = String::new();
            for cluster in &row.graphemes {
                let starts_before_end =
                    row_index != last_row || cluster.range.cell_range.start <= end.cell;
                let ends_after_start =
                    row_index != first_row || cluster.range.cell_range.end > start.cell;
                if starts_before_end && ends_after_start {
                    row_output.push_str(&cluster.text);
                }
            }
            if row_index > first_row && row.hard_break_before {
                output.push('\n');
            } else if row_index > first_row
                && !row_output.is_empty()
                && !output.ends_with(char::is_whitespace)
            {
                output.push(' ');
            }
            output.push_str(&row_output);
        }
        if output.is_empty() {
            return Err(SelectionError::EmptySelection);
        }
        Ok(output)
    }

    pub fn move_focus(&self, point: CellPoint, key: NavigationKey) -> CellPoint {
        keyboard::move_focus(&self.rows, point, key)
    }

    fn word_selection(&self, row_index: usize, cell: usize) -> SelectionRange {
        let Some(row) = self.rows.get(row_index) else {
            return SelectionRange::new(
                CellPoint::new(row_index, cell),
                CellPoint::new(row_index, cell),
            );
        };
        let Some(index) = row
            .graphemes
            .iter()
            .position(|cluster| cluster.range.cell_range.contains(&cell))
        else {
            return SelectionRange::new(
                CellPoint::new(row_index, cell),
                CellPoint::new(row_index, cell),
            );
        };
        if row.graphemes[index].text.chars().all(char::is_whitespace) {
            return self.range_for(row_index, index, index);
        }
        let mut first = index;
        let mut last = index;
        while first > 0 && !self.is_whitespace(row_index, first - 1) {
            first -= 1;
        }
        while last + 1 < row.graphemes.len() && !self.is_whitespace(row_index, last + 1) {
            last += 1;
        }
        self.range_for(row_index, first, last)
    }

    fn is_whitespace(&self, row: usize, index: usize) -> bool {
        self.rows[row].graphemes[index]
            .text
            .chars()
            .all(char::is_whitespace)
    }

    fn range_for(&self, row: usize, first: usize, last: usize) -> SelectionRange {
        SelectionRange::new(
            CellPoint::new(row, self.rows[row].graphemes[first].range.cell_range.start),
            CellPoint::new(
                row,
                self.rows[row].graphemes[last]
                    .range
                    .cell_range
                    .end
                    .saturating_sub(1),
            ),
        )
    }

    fn snap_point(&self, point: CellPoint) -> CellPoint {
        let row = point.row.min(self.rows.len().saturating_sub(1));
        let Some(data) = self.rows.get(row) else {
            return point;
        };
        let cell = point.cell.min(data.width.saturating_sub(1));
        data.graphemes
            .iter()
            .find(|cluster| cluster.range.cell_range.contains(&cell))
            .map_or(CellPoint::new(row, cell), |cluster| {
                CellPoint::new(row, cluster.range.cell_range.start)
            })
    }
}
