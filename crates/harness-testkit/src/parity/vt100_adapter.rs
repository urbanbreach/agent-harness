//! Optional vt100 Screen → SemanticFrame adapter.
//!
//! Uses the same default color resolution as harness-tui visual_renderer cell
//! reads. Semantic capture helper only — not a pixel oracle.

use super::cells::{
    CellModifiers, CursorShape, CursorState, ResolvedRgb, SemanticCell, SemanticFrame, DEFAULT_BG,
    DEFAULT_FG,
};

/// Capture a dense semantic frame from a vt100 screen.
pub fn semantic_frame_from_vt100_screen(screen: &vt100::Screen) -> SemanticFrame {
    let (rows, cols) = screen.size();
    let (cursor_row, cursor_col) = screen.cursor_position();
    let cursor = CursorState {
        row: cursor_row,
        col: cursor_col,
        visible: !screen.hide_cursor(),
        shape: CursorShape::Block,
    };
    let mut frame = SemanticFrame::new(cols, rows, cursor);
    frame.alternate_screen = screen.alternate_screen();

    for row in 0..rows {
        for col in 0..cols {
            let Some(src) = screen.cell(row, col) else {
                continue;
            };
            let continuation = src.is_wide_continuation();
            let width = if continuation {
                0
            } else if src.is_wide() {
                2
            } else {
                1
            };
            let grapheme = if continuation {
                String::new()
            } else {
                src.contents().to_owned()
            };
            let cell = SemanticCell {
                row,
                col,
                grapheme,
                width,
                continuation,
                fg: resolve_vt100_color(src.fgcolor(), true),
                bg: resolve_vt100_color(src.bgcolor(), false),
                modifiers: CellModifiers {
                    bold: src.bold(),
                    dim: src.dim(),
                    italic: src.italic(),
                    underline: src.underline(),
                    inverse: src.inverse(),
                },
                hyperlink: None,
            };
            if let Some(slot) = frame.cell_mut(row, col) {
                *slot = cell;
            }
        }
    }
    frame
}

fn resolve_vt100_color(color: vt100::Color, foreground: bool) -> ResolvedRgb {
    match color {
        vt100::Color::Default => {
            if foreground {
                ResolvedRgb::from_array(DEFAULT_FG)
            } else {
                ResolvedRgb::from_array(DEFAULT_BG)
            }
        }
        vt100::Color::Idx(idx) => ResolvedRgb::from_array(xterm_256_color(idx)),
        vt100::Color::Rgb(r, g, b) => ResolvedRgb::new(r, g, b),
    }
}

fn xterm_256_color(idx: u8) -> [u8; 3] {
    const ANSI_16: [[u8; 3]; 16] = [
        [0, 0, 0],
        [205, 49, 49],
        [13, 188, 121],
        [229, 229, 16],
        [36, 114, 200],
        [188, 63, 188],
        [17, 168, 205],
        [229, 229, 229],
        [102, 102, 102],
        [241, 76, 76],
        [35, 209, 139],
        [245, 245, 67],
        [59, 142, 234],
        [214, 112, 214],
        [41, 184, 219],
        [255, 255, 255],
    ];
    match idx {
        0..=15 => ANSI_16[usize::from(idx)],
        16..=231 => {
            let value = idx - 16;
            let r = value / 36;
            let g = (value % 36) / 6;
            let b = value % 6;
            [cube_level(r), cube_level(g), cube_level(b)]
        }
        232..=255 => {
            let gray = 8_u8.saturating_add((idx - 232) * 10);
            [gray, gray, gray]
        }
    }
}

fn cube_level(level: u8) -> u8 {
    if level == 0 {
        0
    } else {
        level.saturating_mul(40).saturating_add(55)
    }
}
