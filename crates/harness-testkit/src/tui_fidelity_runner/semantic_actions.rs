use crate::parity::SemanticFrame;
use crate::tui_fidelity::CellPoint;

use super::error::RunnerError;

pub fn semantic_click_bytes(
    frame: &SemanticFrame,
    text: &str,
    offset_col: u16,
) -> Result<Vec<u8>, RunnerError> {
    let point = find_text(frame, text).ok_or_else(|| missing(text))?;
    let col = point
        .col
        .checked_add(offset_col)
        .filter(|col| *col < frame.cols)
        .ok_or_else(|| missing(text))?;
    click_bytes(col, point.row)
}

fn click_bytes(col: u16, row: u16) -> Result<Vec<u8>, RunnerError> {
    let down = format!("\x1b[<0;{};{}M", col + 1, row + 1);
    let up = format!("\x1b[<0;{};{}m", col + 1, row + 1);
    Ok([down.as_bytes(), up.as_bytes()].concat())
}

pub(super) fn find_text(frame: &SemanticFrame, text: &str) -> Option<CellPoint> {
    (0..frame.rows).find_map(|row| {
        let mut rendered = String::new();
        let mut columns = Vec::new();
        for cell in frame
            .cells
            .iter()
            .filter(|cell| cell.row == row && !cell.continuation)
        {
            columns.push((rendered.len(), cell.col));
            rendered.push_str(&cell.grapheme);
        }
        let byte = rendered.find(text)?;
        let col = columns.iter().rev().find(|(start, _)| *start <= byte)?.1;
        Some(CellPoint { col, row })
    })
}

pub(super) fn find_text_nearest_row(
    frame: &SemanticFrame,
    text: &str,
    target_row: u16,
) -> Option<CellPoint> {
    (0..frame.rows)
        .filter_map(|row| find_text_in_row(frame, text, row))
        .min_by_key(|point| point.row.abs_diff(target_row))
}

fn find_text_in_row(frame: &SemanticFrame, text: &str, row: u16) -> Option<CellPoint> {
    let mut rendered = String::new();
    let mut columns = Vec::new();
    for cell in frame
        .cells
        .iter()
        .filter(|cell| cell.row == row && !cell.continuation)
    {
        columns.push((rendered.len(), cell.col));
        rendered.push_str(&cell.grapheme);
    }
    let byte = rendered.find(text)?;
    let col = columns.iter().rev().find(|(start, _)| *start <= byte)?.1;
    Some(CellPoint { col, row })
}

pub(super) fn click_point_bytes(point: CellPoint) -> Result<Vec<u8>, RunnerError> {
    click_bytes(point.col, point.row)
}

fn missing(text: &str) -> RunnerError {
    RunnerError::SemanticTargetMissing {
        text: text.to_owned(),
    }
}
