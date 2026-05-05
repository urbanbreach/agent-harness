pub(crate) fn anchored_region(
    anchor: (u16, u16),
    bounds: (u16, u16),
    width_cells: u16,
    height_cells: u16,
    top_padding_cells: u16,
    left_padding_cells: u16,
) -> (u16, u16, u16, u16) {
    let (anchor_row, anchor_col) = anchor;
    let (rows, cols) = bounds;
    let row_start = anchor_row.saturating_sub(top_padding_cells);
    let col_start = anchor_col.saturating_sub(left_padding_cells);

    let max_height = rows.saturating_sub(row_start).max(1);
    let max_width = cols.saturating_sub(col_start).max(1);

    let height = height_cells.min(max_height).max(1);
    let width = width_cells.min(max_width).max(1);

    (row_start, col_start, height, width)
}

#[cfg(test)]
mod tests {
    use super::anchored_region;

    #[test]
    fn anchored_region_applies_padding_before_anchor() {
        assert_eq!(
            anchored_region((5, 7), (20, 30), 10, 8, 2, 3),
            (3, 4, 8, 10)
        );
    }

    #[test]
    fn anchored_region_clamps_to_bounds_with_minimum_size() {
        assert_eq!(anchored_region((1, 1), (3, 4), 10, 10, 5, 5), (0, 0, 3, 4));
        assert_eq!(anchored_region((3, 4), (3, 4), 10, 10, 0, 0), (3, 4, 1, 1));
    }
}
