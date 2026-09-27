use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::selection_types::{
    Autoscroll, CellPoint, DragResult, Grapheme, GraphemeRange, NavigationKey, SelectionError,
    SelectionMode, SelectionRange, Viewport,
};

#[derive(Debug, Clone)]
struct Row {
    graphemes: Range<usize>,
    source_offset: usize,
    width: usize,
}

#[derive(Debug, Clone)]
pub struct WrappedText {
    source: String,
    graphemes: Vec<Grapheme>,
    rows: Vec<Row>,
}

impl WrappedText {
    pub fn new(text: &str, width: usize) -> Result<Self, SelectionError> {
        if width == 0 {
            return Err(SelectionError::ZeroWidth);
        }
        let mut graphemes = Vec::new();
        let mut rows = Vec::new();
        let mut source_offset = 0;
        for line in text.split('\n') {
            let mut start = graphemes.len();
            let mut cell = 0;
            for (byte, value) in line.grapheme_indices(true) {
                let cells = value.width().max(1);
                let wrapped = cell + cells > width && start < graphemes.len();
                if wrapped {
                    rows.push(Row {
                        graphemes: start..graphemes.len(),
                        source_offset,
                        width: cell,
                    });
                    start = graphemes.len();
                    cell = 0;
                }
                // Only the whitespace that caused a soft wrap is omitted.
                if wrapped && value.chars().all(char::is_whitespace) {
                    continue;
                }
                graphemes.push(Grapheme {
                    text: value.to_owned(),
                    range: GraphemeRange {
                        byte_range: byte..byte + value.len(),
                        cell_range: cell..cell + cells,
                    },
                    end: CellPoint::new(0, cell + cells - 1),
                });
                cell += cells;
            }
            rows.push(Row {
                graphemes: start..graphemes.len(),
                source_offset,
                width: cell,
            });
            source_offset += line.len() + 1;
        }
        Ok(Self {
            source: text.to_owned(),
            graphemes,
            rows,
        })
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn row_text(&self, row: usize) -> String {
        let Some(row) = self.rows.get(row) else {
            return String::new();
        };
        let clusters = &self.graphemes[row.graphemes.clone()];
        match (clusters.first(), clusters.last()) {
            (Some(first), Some(last)) => self.source[row.source_offset
                + first.range.byte_range.start
                ..row.source_offset + last.range.byte_range.end]
                .to_owned(),
            _ => String::new(),
        }
    }

    pub fn point_for_byte(&self, byte: usize) -> CellPoint {
        for (index, row) in self.rows.iter().enumerate() {
            let clusters = &self.graphemes[row.graphemes.clone()];
            let cluster = clusters.partition_point(|cluster| {
                row.source_offset + cluster.range.byte_range.end <= byte
            });
            if let Some(cluster) = clusters.get(cluster) {
                return CellPoint::new(index, cluster.range.cell_range.start);
            }
        }
        CellPoint::new(
            self.rows.len().saturating_sub(1),
            self.rows.last().map_or(0, |row| row.width),
        )
    }

    pub fn grapheme_at(&self, point: CellPoint) -> Option<&Grapheme> {
        self.clusters(point.row)
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
        let last = viewport
            .top
            .saturating_add(viewport.height.saturating_sub(1));
        let lines = if focus.row < viewport.top {
            -i32::try_from(viewport.top - focus.row).unwrap_or(i32::MAX)
        } else if focus.row > last {
            i32::try_from(focus.row - last).unwrap_or(i32::MAX)
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
        let clusters = self.clusters(row);
        match mode {
            SelectionMode::Character => SelectionRange::new(point, point),
            SelectionMode::Line => SelectionRange::new(
                CellPoint::new(row, 0),
                CellPoint::new(row, self.last_cell(row)),
            ),
            SelectionMode::Word => {
                let Some(index) = clusters
                    .iter()
                    .position(|cluster| cluster.range.cell_range.contains(&point.cell))
                else {
                    let point = CellPoint::new(row, point.cell);
                    return SelectionRange::new(point, point);
                };
                let mut first = index;
                let mut last = index;
                if !clusters[index].text.chars().all(char::is_whitespace) {
                    while first > 0 && !clusters[first - 1].text.chars().all(char::is_whitespace) {
                        first -= 1;
                    }
                    while last + 1 < clusters.len()
                        && !clusters[last + 1].text.chars().all(char::is_whitespace)
                    {
                        last += 1;
                    }
                }
                SelectionRange::new(
                    CellPoint::new(row, clusters[first].range.cell_range.start),
                    CellPoint::new(row, clusters[last].range.cell_range.end - 1),
                )
            }
        }
    }

    pub fn copy(&self, selection: SelectionRange) -> Result<String, SelectionError> {
        let (start, end) = selection.normalized();
        let first_row = start.row.min(self.rows.len().saturating_sub(1));
        let last_row = end.row.min(self.rows.len().saturating_sub(1));
        let from = self.rows[first_row].source_offset
            + self
                .clusters(first_row)
                .iter()
                .find(|cluster| cluster.range.cell_range.end > start.cell)
                .map_or(0, |cluster| cluster.range.byte_range.start);
        let to = self.rows[last_row].source_offset
            + self
                .clusters(last_row)
                .iter()
                .rev()
                .find(|cluster| cluster.range.cell_range.start <= end.cell)
                .map_or(0, |cluster| cluster.range.byte_range.end);
        if from >= to {
            return Err(SelectionError::EmptySelection);
        }
        self.source
            .get(from..to)
            .map(str::to_owned)
            .ok_or(SelectionError::InvalidPoint)
    }

    pub fn move_focus(&self, point: CellPoint, key: NavigationKey) -> CellPoint {
        let point = self.snap_point(point);
        match key {
            NavigationKey::Up => {
                self.snap_point(CellPoint::new(point.row.saturating_sub(1), point.cell))
            }
            NavigationKey::Down => {
                self.snap_point(CellPoint::new(point.row.saturating_add(1), point.cell))
            }
            NavigationKey::Home => CellPoint::new(point.row, 0),
            NavigationKey::End => CellPoint::new(point.row, self.last_cell(point.row)),
            NavigationKey::Left | NavigationKey::Right => {
                let clusters = self.clusters(point.row);
                let Some(index) = clusters
                    .iter()
                    .position(|cluster| cluster.range.cell_range.contains(&point.cell))
                else {
                    return point;
                };
                if key == NavigationKey::Left {
                    if index > 0 {
                        CellPoint::new(point.row, clusters[index - 1].range.cell_range.start)
                    } else if point.row > 0 {
                        CellPoint::new(point.row - 1, self.last_cell(point.row - 1))
                    } else {
                        point
                    }
                } else if let Some(next) = clusters.get(index + 1) {
                    CellPoint::new(point.row, next.range.cell_range.start)
                } else if point.row + 1 < self.rows.len() {
                    CellPoint::new(point.row + 1, 0)
                } else {
                    point
                }
            }
        }
    }

    fn clusters(&self, row: usize) -> &[Grapheme] {
        self.rows
            .get(row)
            .map_or(&[], |row| &self.graphemes[row.graphemes.clone()])
    }

    fn last_cell(&self, row: usize) -> usize {
        self.rows
            .get(row)
            .map_or(0, |row| row.width.saturating_sub(1))
    }

    fn snap_point(&self, point: CellPoint) -> CellPoint {
        let row = point.row.min(self.rows.len().saturating_sub(1));
        let point = CellPoint::new(row, point.cell.min(self.last_cell(row)));
        self.grapheme_at(point).map_or(point, |cluster| {
            CellPoint::new(row, cluster.range.cell_range.start)
        })
    }
}
