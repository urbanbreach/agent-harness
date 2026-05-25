pub(crate) fn anchored_region(
    anchor: (u16, u16),
    bounds: (u16, u16),
    width: u16,
    height: u16,
    pad_rows: u16,
    pad_cols: u16,
) -> (u16, u16, u16, u16) {
    let row = anchor.0.saturating_sub(pad_rows);
    let col = anchor.1.saturating_sub(pad_cols);
    let available_rows = bounds.0.saturating_sub(row).max(1);
    let available_cols = bounds.1.saturating_sub(col).max(1);
    (
        row,
        col,
        height.min(available_rows),
        width.min(available_cols),
    )
}
