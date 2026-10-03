use super::*;

pub(super) fn code_background(line: &ratatui::text::Line<'_>, theme: &Theme) -> Option<Color> {
    line.style.bg.or_else(|| {
        line.spans.iter().find_map(|span| {
            span.style
                .bg
                .filter(|color| *color == theme.markdown.code_background)
        })
    })
}

pub(super) fn render_line<'a>(
    line: &'a RenderedLine,
    visual_mode: bool,
    theme: &Theme,
) -> ratatui::text::Line<'a> {
    let mut column = 0;
    let mut source_styles = line
        .styled
        .iter()
        .flat_map(|line| &line.spans)
        .scan(0, |end, span| {
            *end += span.width();
            Some((*end, span.style))
        })
        .peekable();
    let spans = line
        .text
        .graphemes(true)
        .map(|grapheme| {
            let start = column;
            column += grapheme.width();
            while source_styles.peek().is_some_and(|(end, _)| *end <= start) {
                let _ = source_styles.next();
            }
            let mut style = source_styles.peek().map_or_else(
                || Style::default().fg(theme.terminal_colors.primary),
                |(_, style)| *style,
            );
            if line
                .match_ranges
                .iter()
                .any(|range| range.start < column && range.end > start)
            {
                style = style.add_modifier(Modifier::REVERSED);
            }
            if !visual_mode
                && line
                    .selection_range
                    .as_ref()
                    .is_some_and(|range| range.start < column && range.end > start)
            {
                style = style.bg(theme.text.accent).fg(theme.surface.canvas);
            }
            ratatui::text::Span::styled(grapheme, style)
        })
        .collect::<Vec<_>>();
    ratatui::text::Line::from(spans)
}
