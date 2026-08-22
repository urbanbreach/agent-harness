use crate::terminal::char_display_width;
use crate::theme_tokens::DESIGN_TOKENS;

use super::regions::{ShellRegions, ShellState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorPlacement {
    pub position: (u16, u16),
    pub display_column: u16,
    pub visible: bool,
    pub clipped: bool,
}

pub fn cursor_for(
    regions: &ShellRegions,
    state: ShellState,
    text: &str,
    cursor_chars: usize,
) -> CursorPlacement {
    let padding = DESIGN_TOKENS.spacing.composer_padding_x;
    let inner_width = regions
        .composer
        .width
        .saturating_sub(padding.saturating_mul(2))
        .max(1);
    let inner_x = regions
        .composer
        .x
        .saturating_add(padding.min(regions.composer.width));
    let inner_y = regions
        .composer
        .y
        .saturating_add(1.min(regions.composer.height));
    let (row, column) = cursor_cell(text, cursor_chars, inner_width);
    let max_row = regions.composer.height.saturating_sub(1);
    let max_column = inner_width.saturating_sub(1);
    CursorPlacement {
        position: (
            inner_x.saturating_add(column.min(max_column)),
            inner_y.saturating_add(row.min(max_row)),
        ),
        display_column: column,
        visible: state.is_editable(),
        clipped: false,
    }
}

fn cursor_cell(text: &str, cursor_chars: usize, inner_width: u16) -> (u16, u16) {
    let mut row = 0u16;
    let mut column = 0u16;
    let width = inner_width.max(1);
    for character in text.chars().take(cursor_chars) {
        if character == '\n' {
            row = row.saturating_add(1);
            column = 0;
            continue;
        }
        let character_width = char_display_width(character);
        if character_width > 0 && column > 0 && column.saturating_add(character_width) > width {
            row = row.saturating_add(1);
            column = 0;
        }
        column = column.saturating_add(character_width).min(width);
    }
    (row, column)
}
