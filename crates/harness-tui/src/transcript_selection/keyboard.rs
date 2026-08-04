use super::selection::WrappedRow;
use super::selection_types::{CellPoint, NavigationKey};

pub(crate) fn move_focus(rows: &[WrappedRow], point: CellPoint, key: NavigationKey) -> CellPoint {
    let point = snap_point(rows, point);
    match key {
        NavigationKey::Left => move_left(rows, point),
        NavigationKey::Right => move_right(rows, point),
        NavigationKey::Up => move_vertical(rows, point, -1),
        NavigationKey::Down => move_vertical(rows, point, 1),
        NavigationKey::Home => CellPoint::new(point.row, 0),
        NavigationKey::End => CellPoint::new(
            point.row,
            rows.get(point.row)
                .map_or(0, |row| row.width.saturating_sub(1)),
        ),
    }
}

fn move_left(rows: &[WrappedRow], point: CellPoint) -> CellPoint {
    let Some(row) = rows.get(point.row) else {
        return point;
    };
    let Some(index) = cluster_index(row, point.cell) else {
        return point;
    };
    if index > 0 {
        return CellPoint::new(point.row, row.graphemes[index - 1].range.cell_range.start);
    }
    point
        .row
        .checked_sub(1)
        .and_then(|row_index| rows.get(row_index))
        .map_or(point, |row| {
            CellPoint::new(point.row - 1, row.width.saturating_sub(1))
        })
}

fn move_right(rows: &[WrappedRow], point: CellPoint) -> CellPoint {
    let Some(row) = rows.get(point.row) else {
        return point;
    };
    let Some(index) = cluster_index(row, point.cell) else {
        return point;
    };
    if let Some(next) = row.graphemes.get(index + 1) {
        return CellPoint::new(point.row, next.range.cell_range.start);
    }
    rows.get(point.row + 1)
        .map_or(point, |_| CellPoint::new(point.row + 1, 0))
}

fn move_vertical(rows: &[WrappedRow], point: CellPoint, direction: isize) -> CellPoint {
    let row = if direction.is_negative() {
        point.row.saturating_sub(direction.unsigned_abs())
    } else {
        point.row.saturating_add(direction.unsigned_abs())
    };
    snap_point(rows, CellPoint::new(row, point.cell))
}

fn cluster_index(row: &WrappedRow, cell: usize) -> Option<usize> {
    row.graphemes
        .iter()
        .position(|cluster| cluster.range.cell_range.contains(&cell))
}

fn snap_point(rows: &[WrappedRow], point: CellPoint) -> CellPoint {
    let row = point.row.min(rows.len().saturating_sub(1));
    let Some(data) = rows.get(row) else {
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
