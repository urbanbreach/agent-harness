use super::*;

pub(super) fn render_collapsed_composer(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    context: ComposerRenderContext<'_>,
) {
    let surface = composer_input_surface(theme);
    let text = app.composer_render_text();
    let glyph = format!("{} ", theme.live_shell.transcript_glyphs.user_marker);
    let body_width = usize::from(area.width)
        .saturating_sub(display_width(&glyph))
        .max(1);
    let resolved = super::presentation::resolve_composer(
        app,
        &app.composer.render_data(),
        &text,
        false,
        context.dock.composer_disabled,
        false,
        "Build anything",
        body_width,
        1,
        1,
        false,
    );
    let line = Line::from(vec![
        Span::styled(
            glyph,
            Style::default().fg(theme.terminal_colors.muted).bg(surface),
        ),
        Span::styled(
            resolved.body,
            Style::default()
                .fg(super::bordered::live_composer_content_color(
                    theme,
                    theme.terminal_colors.secondary,
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
