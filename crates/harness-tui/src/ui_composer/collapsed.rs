use super::*;

pub(super) fn render_collapsed_composer(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    context: DocumentComposerRenderContext<'_>,
) {
    let surface = composer_input_surface(theme);
    let text = app.composer_render_text();
    let glyph = format!("{} ", theme.live_shell.transcript_glyphs.user_marker);
    let body_width = usize::from(area.width)
        .saturating_sub(display_width(&glyph))
        .max(1);
    let Some(resolved) = super::presentation::resolve_composer(
        app,
        &text,
        false,
        context.dock.composer_disabled,
        false,
        "Build anything",
        body_width,
        1,
        1,
        false,
    ) else {
        return;
    };
    let line = Line::from(vec![
        Span::styled(
            glyph,
            Style::default()
                .fg(theme.reference_terminal.muted)
                .bg(surface),
        ),
        Span::styled(
            resolved.body,
            Style::default()
                .fg(super::bordered::live_composer_content_color(
                    theme,
                    theme.reference_terminal.secondary,
                    false,
                ))
                .bg(surface),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(line).style(Style::default().bg(surface)),
        area,
    );
}
