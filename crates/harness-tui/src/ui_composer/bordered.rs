use super::*;

pub(crate) fn render_bordered_composer(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    context: DocumentComposerRenderContext<'_>,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = composer_input_surface(theme);
    let composer_surface = surface;
    let focused = context.dock.composer_focused && !footer_suppressed_by_overlay(app);
    if area.height == 1 {
        super::collapsed::render_collapsed_composer(frame, app, area, theme, context);
        return;
    }
    let composer_view = app.composer_view_model_for_area(area);
    let mut extra_identity = Vec::new();
    if !composer_view.attachments.is_empty() {
        let labels = composer_view
            .attachments
            .iter()
            .map(|attachment| attachment.label.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        extra_identity.push(labels);
    }
    if let Some(completion) = composer_view.completion.as_ref() {
        extra_identity.push(format!("{} suggestions", completion.items.len()));
    }
    let badge = composer_model_badge(
        app,
        &extra_identity,
        usize::from(area.width.saturating_sub(5)),
    );
    let content_lines = context.composer_lines.max(1);
    let strip_height = area
        .height
        .min(content_lines.saturating_add(2))
        .max(3.min(area.height).max(1));
    let strip = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: strip_height,
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(Style::default().bg(surface));
    let inner = block.inner(strip);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let composer_text = app.composer_render_text();
    let composer_empty = composer_text.is_empty();
    let glyph_cols = 3_usize;
    let draft_width = usize::from(inner.width)
        .saturating_sub(glyph_cols.saturating_add(1))
        .max(1);
    let max_visible = usize::from(inner.height.min(content_lines).max(1));
    let show_cursor = !context.dock.composer_disabled && focused;
    let Some(resolved) = super::presentation::resolve_composer(
        app,
        &composer_text,
        context.dock.composer_focused,
        context.dock.composer_disabled,
        context.dock.variant == crate::view_model::ControlDockVariant::Startup,
        if focused { "" } else { "Build anything" },
        draft_width,
        max_visible,
        strip.height,
        show_cursor,
    ) else {
        return;
    };
    let glyph_prefix = format!(
        " {} ",
        resolved
            .surface
            .marker()
            .unwrap_or(theme.live_shell.transcript_glyphs.user_marker)
    );
    let glyph_cols = display_width(&glyph_prefix);
    let mode_style = composer_mode_style(theme, resolved.tone, focused);
    let border_style = Style::default().fg(mode_style.border).bg(surface);
    let block = block.border_style(border_style);
    let block = if resolved
        .chrome
        .contains(&crate::composer_integration::ComposerChrome::Title)
    {
        let badge = resolved.surface.right_label().unwrap_or(badge.as_str());
        let (badge_title, badge_style) = if badge.is_empty() {
            ("  ─".to_string(), border_style)
        } else {
            (
                format!(" {badge} ─"),
                Style::default()
                    .fg(live_composer_caption_color(theme, focused))
                    .bg(surface),
            )
        };
        block.title_bottom(Line::from(Span::styled(badge_title, badge_style)).right_aligned())
    } else {
        block
    };
    frame.render_widget(block, strip);
    let shell_mode_active = resolved.tone == crate::composer_integration::ComposerTone::Shell
        && !context.dock.composer_disabled;
    let body_color = if context.dock.composer_disabled {
        theme.status.disabled
    } else if shell_mode_active {
        theme.status.warning
    } else if composer_empty {
        theme.reference_terminal.secondary
    } else {
        composer_input_text(theme)
    };
    let body_color = live_composer_content_color(theme, body_color, focused);
    let glyph_style = if context.dock.composer_disabled {
        Style::default()
            .fg(theme.status.disabled)
            .bg(composer_surface)
    } else if matches!(
        resolved.tone,
        crate::composer_integration::ComposerTone::Shell
            | crate::composer_integration::ComposerTone::Plan
    ) {
        Style::default().fg(mode_style.accent).bg(composer_surface)
    } else {
        Style::default()
            .fg(live_composer_content_color(
                theme,
                if focused {
                    theme.reference_terminal.prompt_accent
                } else {
                    theme.reference_terminal.muted
                },
                focused,
            ))
            .bg(composer_surface)
    };

    let viewport = &resolved.viewport;

    let base_style = Style::default().fg(body_color).bg(composer_surface);
    let body_lines = viewport
        .lines
        .iter()
        .enumerate()
        .map(|(row, line)| {
            if row == 0 {
                Line::from(vec![
                    Span::styled(glyph_prefix.clone(), glyph_style),
                    Span::styled(line.clone(), base_style),
                ])
            } else {
                Line::from(vec![
                    Span::styled(" ".repeat(glyph_cols), base_style),
                    Span::styled(line.clone(), base_style),
                ])
            }
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(body_lines).style(Style::default().bg(composer_surface)),
        inner,
    );

    if !connect_waiting_owns_input(app) {
        if let Some((cursor_row, cursor_col)) = viewport.cursor {
            let cursor_x = inner
                .x
                .saturating_add(
                    u16::try_from(glyph_cols.saturating_add(cursor_col)).unwrap_or(u16::MAX),
                )
                .min(inner.x.saturating_add(inner.width.saturating_sub(1)));
            let cursor_y = inner
                .y
                .saturating_add(u16::try_from(cursor_row).unwrap_or(u16::MAX))
                .min(inner.y.saturating_add(inner.height.saturating_sub(1)));
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}

pub(crate) fn connect_waiting_owns_input(app: &AppState) -> bool {
    app.connect_dialog.visible
        && app.connect_dialog.step == crate::app::auth_dialog::ConnectDialogStep::Waiting
}

pub(super) const fn live_composer_border_color(theme: &Theme, focused: bool) -> Color {
    if focused {
        theme.reference_terminal.prompt_border_active
    } else {
        theme.reference_terminal.prompt_border
    }
}

pub(crate) fn live_composer_content_color(theme: &Theme, color: Color, focused: bool) -> Color {
    if focused {
        color
    } else {
        blend_color(theme.reference_terminal.canvas, color, 0.66)
    }
}

fn live_composer_caption_color(theme: &Theme, focused: bool) -> Color {
    let opacity = if focused { 0.6 } else { 0.4 };
    blend_color(
        theme.reference_terminal.canvas,
        theme.reference_terminal.prompt_accent,
        opacity,
    )
}

#[cfg(test)]
mod active_thinking_color_tests {
    use super::*;

    #[test]
    fn live_composer_border_matches_the_groknight_active_prompt() {
        let theme = Theme::harness_chat();

        assert_eq!(
            live_composer_border_color(&theme, true),
            Color::Rgb(80, 80, 88)
        );
        assert_eq!(
            live_composer_border_color(&theme, false),
            Color::Rgb(50, 50, 55)
        );
    }

    #[test]
    fn plan_composer_uses_reference_warning_marker_and_bright_border() {
        // Given: the active Harness chat theme and a focused plan surface.
        let theme = Theme::harness_chat();

        // When: the shared composer presentation style is resolved.
        let style = composer_mode_style(
            &theme,
            crate::composer_integration::ComposerTone::Plan,
            true,
        );

        // Then: the marker is warning-yellow and the border is terminal-primary bright.
        assert_eq!(style.accent, Color::LightYellow);
        assert_eq!(style.border, theme.reference_terminal.primary);
    }
}
