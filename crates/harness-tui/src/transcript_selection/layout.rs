use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::{
    Autoscroll, CellPoint, DragResult, Grapheme, GraphemeRange, NavigationKey, SelectionError,
    SelectionMode, SelectionRange, Viewport,
};

#[derive(Debug, Clone)]
struct Row {
    bytes: Range<usize>,
    graphemes: Range<usize>,
    line_start: usize,
    width: usize,
}

struct Cluster<'a> {
    text: &'a str,
    bytes: Range<usize>,
    cells: Range<usize>,
}

/// One source string, compact byte/cell ends, and display-row ranges.
/// Geometry is immutable; no owned grapheme strings or mutable paint cache.
#[derive(Debug, Clone)]
pub(crate) struct TextLayout {
    text: String,
    rows: Vec<Row>,
    ends: Vec<(usize, usize)>,
}

impl TextLayout {
    pub(crate) fn new(text: String, width: usize) -> Result<Self, SelectionError> {
        if width == 0 {
            return Err(SelectionError::ZeroWidth);
        }
        let mut rows = Vec::new();
        let mut ends = Vec::new();
        let mut line_start = 0;
        for line in text.split('\n') {
            let mut first = ends.len();
            let mut bytes = line_start..line_start;
            let mut cell = 0;
            for (offset, value) in line.grapheme_indices(true) {
                let byte = line_start + offset;
                let cells = value.width().max(1);
                let wrapped = cell > 0 && cell + cells > width;
                if wrapped {
                    rows.push(Row {
                        bytes,
                        graphemes: first..ends.len(),
                        line_start,
                        width: cell,
                    });
                    bytes = byte..byte;
                    first = ends.len();
                    cell = 0;
                }
                // Only the whitespace that triggered the soft wrap is omitted.
                if wrapped && value.chars().all(char::is_whitespace) {
                    bytes = byte + value.len()..byte + value.len();
                    continue;
                }
                if cell == 0 {
                    bytes.start = byte;
                }
                bytes.end = byte + value.len();
                cell += cells;
                ends.push((bytes.end, cell));
            }
            rows.push(Row {
                bytes,
                graphemes: first..ends.len(),
                line_start,
                width: cell,
            });
            line_start += line.len() + 1;
        }
        Ok(Self { text, rows, ends })
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub(crate) fn row_text(&self, row: usize) -> &str {
        self.rows
            .get(row)
            .map_or("", |row| &self.text[row.bytes.clone()])
    }

    pub(crate) fn point_for_byte(&self, byte: usize) -> CellPoint {
        let cluster = self.ends.partition_point(|(end, _)| *end <= byte);
        let row = self
            .rows
            .partition_point(|row| row.graphemes.end <= cluster);
        if let Some(geometry) = self.rows.get(row) {
            let cell = if cluster == geometry.graphemes.start {
                0
            } else {
                self.ends[cluster - 1].1
            };
            return CellPoint::new(row, cell);
        }
        CellPoint::new(
            self.rows.len() - 1,
            self.rows.last().map_or(0, |row| row.width),
        )
    }

    pub(crate) fn drag_with_autoscroll(&self, focus: CellPoint, viewport: Viewport) -> DragResult {
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

    pub(crate) fn select(&self, point: CellPoint, mode: SelectionMode) -> SelectionRange {
        let row = point.row.min(self.rows.len() - 1);
        match mode {
            SelectionMode::Character => SelectionRange::new(point, point),
            SelectionMode::Line => SelectionRange::new(
                CellPoint::new(row, 0),
                CellPoint::new(row, self.last_cell(row)),
            ),
            SelectionMode::Word => {
                let mut first = 0;
                let mut clusters = self.clusters(row);
                let Some(found) = clusters.find(|cluster| {
                    if cluster.cells.contains(&point.cell) {
                        return true;
                    }
                    if cluster.text.chars().all(char::is_whitespace) {
                        first = cluster.cells.end;
                    }
                    false
                }) else {
                    let point = CellPoint::new(row, point.cell);
                    return SelectionRange::new(point, point);
                };
                let mut end = found.cells.end;
                if found.text.chars().all(char::is_whitespace) {
                    first = found.cells.start;
                } else {
                    end = clusters
                        .take_while(|cluster| !cluster.text.chars().all(char::is_whitespace))
                        .last()
                        .map_or(end, |cluster| cluster.cells.end);
                }
                SelectionRange::new(CellPoint::new(row, first), CellPoint::new(row, end - 1))
            }
        }
    }

    pub(crate) fn copy(&self, selection: SelectionRange) -> Result<String, SelectionError> {
        let (start, end) = selection.normalized();
        let first = start.row.min(self.rows.len() - 1);
        let last = end.row.min(self.rows.len() - 1);
        let from = self
            .clusters(first)
            .find(|cluster| cluster.cells.end > start.cell)
            .map_or(self.rows[first].line_start, |cluster| cluster.bytes.start);
        let to = self
            .clusters(last)
            .filter(|cluster| cluster.cells.start <= end.cell)
            .last()
            .map_or(self.rows[last].line_start, |cluster| cluster.bytes.end);
        if from >= to {
            return Err(SelectionError::EmptySelection);
        }
        self.text
            .get(from..to)
            .map(str::to_owned)
            .ok_or(SelectionError::InvalidPoint)
    }

    pub(crate) fn move_focus(&self, point: CellPoint, key: NavigationKey) -> CellPoint {
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
                let mut previous = None;
                let mut clusters = self.clusters(point.row);
                let found = clusters.find(|cluster| {
                    if cluster.cells.contains(&point.cell) {
                        return true;
                    }
                    previous = Some(cluster.cells.start);
                    false
                });
                if found.is_none() {
                    return point;
                }
                if key == NavigationKey::Left {
                    if let Some(cell) = previous {
                        CellPoint::new(point.row, cell)
                    } else if point.row > 0 {
                        CellPoint::new(point.row - 1, self.last_cell(point.row - 1))
                    } else {
                        point
                    }
                } else if let Some(next) = clusters.next() {
                    CellPoint::new(point.row, next.cells.start)
                } else if point.row + 1 < self.rows.len() {
                    CellPoint::new(point.row + 1, 0)
                } else {
                    point
                }
            }
        }
    }

    pub(super) fn owned_graphemes(&self) -> Vec<Vec<Grapheme>> {
        self.rows
            .iter()
            .enumerate()
            .map(|(row, geometry)| {
                self.clusters(row)
                    .map(|cluster| Grapheme {
                        text: cluster.text.to_owned(),
                        end: CellPoint::new(0, cluster.cells.end - 1),
                        range: GraphemeRange {
                            byte_range: cluster.bytes.start - geometry.line_start
                                ..cluster.bytes.end - geometry.line_start,
                            cell_range: cluster.cells,
                        },
                    })
                    .collect()
            })
            .collect()
    }

    fn clusters(&self, row: usize) -> impl Iterator<Item = Cluster<'_>> {
        let (range, first) = self
            .rows
            .get(row)
            .map_or((0..0, 0), |row| (row.graphemes.clone(), row.bytes.start));
        self.ends[range]
            .iter()
            .scan((first, 0), move |start, &(end, cell)| {
                let (byte, column) = *start;
                *start = (end, cell);
                Some(Cluster {
                    text: &self.text[byte..end],
                    bytes: byte..end,
                    cells: column..cell,
                })
            })
    }

    fn last_cell(&self, row: usize) -> usize {
        self.rows
            .get(row)
            .map_or(0, |row| row.width.saturating_sub(1))
    }

    fn snap_point(&self, point: CellPoint) -> CellPoint {
        let row = point.row.min(self.rows.len() - 1);
        let point = CellPoint::new(row, point.cell.min(self.last_cell(row)));
        self.clusters(row)
            .find(|cluster| cluster.cells.contains(&point.cell))
            .map_or(point, |cluster| CellPoint::new(row, cluster.cells.start))
    }
}
