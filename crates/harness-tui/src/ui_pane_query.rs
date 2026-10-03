use super::*;
use crate::app::pane_query::{PaneQuery, PaneQueryMode};
use unicode_segmentation::UnicodeSegmentation as _;
use unicode_width::UnicodeWidthStr as _;

pub(super) fn render(frame: &mut Frame, query: &PaneQuery, area: Rect, theme: &Theme) {
    let mode = if query.mode == PaneQueryMode::Filter {
        "filter"
    } else {
        "search"
    };
    let text = query.editor.text();
    let base = Style::default()
        .fg(theme.terminal_colors.prompt_accent)
        .bg(theme.surface.shell);
    if !query.editing {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("[{mode}: {text}]  "),
                base.add_modifier(Modifier::DIM),
            )))
            .alignment(Alignment::Right),
            area,
        );
        return;
    }
    let cursor = text
        .grapheme_indices(true)
        .nth(query.editor.cursor().insertion_index())
        .map_or(text.len(), |(byte, _)| byte);
    frame.render_widget(
        Paragraph::new(editing_line(&text, cursor, mode, area.width, theme))
            .style(base.fg(theme.text.primary)),
        area,
    );
}

pub(crate) fn editing_line(
    text: &str,
    cursor: usize,
    mode: &str,
    width: u16,
    theme: &Theme,
) -> Line<'static> {
    let base = Style::default()
        .fg(theme.text.primary)
        .bg(theme.surface.shell);
    let label = format!("{mode}: ");
    let budget = usize::from(width)
        .saturating_sub(label.width())
        .saturating_sub(1);
    let mut start = cursor;
    let mut remaining = budget;
    for (index, grapheme) in text[..cursor].grapheme_indices(true).rev() {
        if grapheme.width() > remaining {
            break;
        }
        start = index;
        remaining = remaining.saturating_sub(grapheme.width());
    }
    let before = text[start..cursor].to_string();
    let after = super::truncate_plain_text(&text[cursor..], remaining.saturating_add(1));
    let cursor_glyph = after.graphemes(true).next().unwrap_or(" ").to_string();
    let suffix = after.get(cursor_glyph.len()..).unwrap_or("").to_string();
    Line::from(vec![
        Span::styled(label, base.fg(theme.status.warning)),
        Span::styled(before, base),
        Span::styled(cursor_glyph, base.add_modifier(Modifier::REVERSED)),
        Span::styled(suffix, base),
    ])
}
