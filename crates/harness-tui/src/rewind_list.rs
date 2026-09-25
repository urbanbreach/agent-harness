//! Grok rewind list geometry shared by rendering, hit testing, and panel height.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theme::Theme;

/// Rows shown before the list scrolls (matches the historical picker cap).
const MAX_ROWS: usize = 15;

/// List geometry: row count and cursor position.
/// Construct per call; every method derives the same scroll window from these two fields, so the render, hit-test, and height paths cannot drift.
pub struct ListOverlay {
    pub len: usize,
    pub selected: usize,
}

/// Per-row style context handed to the row-content closure.
pub struct RowCtx {
    pub is_cursor: bool,
    /// Resolved row background (cursor rows get the visual-selection bg).
    pub row_bg: Color,
    /// Width available for the row's content.
    pub content_width: u16,
}

impl ListOverlay {
    /// Overlay height: title plus rows (at most [`MAX_ROWS`]), capped at 60% of the screen, plus one padding row.
    pub fn height(&self, screen_h: u16) -> u16 {
        let rows = u16::try_from(self.len.min(MAX_ROWS)).unwrap_or(15);
        let h = 2 + rows;
        let cap = u16::try_from((u32::from(screen_h) * 60 / 100).max(6)).unwrap_or(screen_h);
        h.min(cap) + 1
    }

    /// Rows that fit in `area` (title and padding excluded).
    fn visible_rows(area: Rect) -> usize {
        area.height.saturating_sub(3) as usize
    }

    /// First visible row index (keeps the cursor inside the window).
    fn scroll_offset(&self, visible_rows: usize) -> usize {
        if visible_rows > 0 && self.selected >= visible_rows {
            self.selected - visible_rows + 1
        } else {
            0
        }
    }

    /// Row index under a screen position, or `None` when the position misses the rows.
    pub fn row_at(&self, area: Rect, col: u16, row: u16) -> Option<usize> {
        if area.height == 0 || area.width < 10 {
            return None;
        }
        if col < area.x || col >= area.x + area.width {
            return None;
        }
        if row < area.y || row >= area.y + area.height {
            return None;
        }
        let first = area.y + 2;
        if row < first {
            return None;
        }
        let visible_rows = Self::visible_rows(area);
        let rel = (row - first) as usize;
        if rel >= visible_rows {
            return None;
        }
        let idx = self.scroll_offset(visible_rows) + rel;
        (idx < self.len).then_some(idx)
    }

    /// Render the overlay: bg fill, accent bar, title, then the visible window of rows.
    /// `row_line(idx, ctx)` produces each row's content; cursor and row backgrounds are painted here.
    /// Applies the standard unfocus dim, so callers must not blend again.
    pub fn render(
        &self,
        buf: &mut Buffer,
        area: Rect,
        title: &str,
        focused: bool,
        theme: &Theme,
        mut row_line: impl FnMut(usize, &RowCtx) -> Line<'static>,
    ) {
        if area.height == 0 || area.width < 10 {
            return;
        }

        let bg = theme.surface.card;
        buf.set_style(area, Style::default().bg(bg));

        let accent_style = Style::default().fg(theme.terminal_colors.prompt_accent);
        for row in area.y..area.y + area.height {
            if let Some(cell) = buf.cell_mut((area.x, row)) {
                cell.set_symbol(theme.live_shell.transcript_glyphs.rail);
                cell.set_style(accent_style);
            }
        }

        let content_x = area.x + 3;
        let content_w = area.width.saturating_sub(5);

        let title_style = Style::default()
            .fg(theme.terminal_colors.prompt_accent)
            .add_modifier(Modifier::BOLD);
        let mut y = area.y + 1;
        buf.set_line(
            content_x,
            y,
            &Line::from(Span::styled(title.to_string(), title_style)),
            content_w,
        );
        y += 1;

        let visible_rows = Self::visible_rows(area);
        let scroll_offset = self.scroll_offset(visible_rows);

        for i in (scroll_offset..self.len).take(visible_rows) {
            if y >= area.y + area.height {
                break;
            }
            let is_cursor = i == self.selected;
            let row_rect = Rect {
                x: content_x.saturating_sub(1),
                y,
                width: content_w + 2,
                height: 1,
            };
            buf.set_style(row_rect, Style::default().bg(bg));

            let ctx = RowCtx {
                is_cursor,
                row_bg: bg,
                content_width: content_w,
            };
            let line = row_line(i, &ctx);
            buf.set_line(content_x, y, &line, content_w);
            // Selection band on RGB themes; reverse video on the terminal
            // theme (patched over the rendered row).
            if is_cursor && focused {
                buf.set_style(row_rect, selection_style(theme));
            }
            y += 1;
        }

        // Unfocus dim: blend foregrounds toward the panel bg so the overlay recedes when the prompt area is unfocused (prompt_widget pattern)
        if !focused {
            recede_area(buf, area, bg, 0.66);
        }
    }
}

pub(crate) fn selection_style(theme: &Theme) -> Style {
    if theme.surface.canvas == Color::Reset {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default().bg(crate::theme::quantize_color(
            if theme.is_dark() {
                Color::Rgb(54, 54, 54)
            } else {
                Color::Rgb(198, 198, 198)
            },
            theme.color_level(),
        ))
    }
}

pub(crate) fn truncate(text: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    let offset = |limit| {
        let mut used = 0;
        text.char_indices()
            .find_map(|(index, ch)| {
                used += UnicodeWidthChar::width(ch).unwrap_or(0);
                (used > limit).then_some(index)
            })
            .unwrap_or(text.len())
    };
    if offset(width) == text.len() {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    format!("{}…", &text[..offset(width - 1)])
}

pub(crate) fn recede_area(buf: &mut Buffer, area: Rect, bg: Color, opacity: f32) {
    let rgb = |color| match color {
        Color::Rgb(..) | Color::Indexed(_) => crate::theme::resolve_to_rgb(color),
        _ => None,
    };
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let Some(cell) = buf.cell_mut((x, y)) else {
                continue;
            };
            let Some((br, bg, bb)) = rgb(bg) else {
                cell.modifier.insert(Modifier::DIM);
                cell.modifier.remove(Modifier::BOLD);
                continue;
            };
            let Some((r, g, b)) = rgb(cell.fg) else {
                continue;
            };
            let (r, g, b) = (
                blend(br, r, opacity),
                blend(bg, g, opacity),
                blend(bb, b, opacity),
            );
            cell.fg = if matches!(cell.fg, Color::Indexed(_)) {
                Color::Indexed(crate::theme::nearest_indexed(r, g, b))
            } else {
                Color::Rgb(r, g, b)
            };
        }
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Rounded RGB channel is bounded to 0..255"
)]
fn blend(base: u8, fg: u8, opacity: f32) -> u8 {
    (f32::from(base) * (1.0 - opacity) + f32::from(fg) * opacity).round() as u8
}
