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
    let badge = if context.dock.variant == crate::view_model::ControlDockVariant::Startup {
        startup_composer_badge(badge)
    } else {
        badge
    };
    let content_lines = context.composer_lines.max(1);
    let strip = composer_strip(area, content_lines);
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
    let input = composer_text_area(strip);
    let draft_width = usize::from(input.width).max(1);
    let max_visible = usize::from(inner.height.min(content_lines).max(1));
    let show_cursor = !context.dock.composer_disabled && focused;
    let ghost_visible = app.composer_ghost_eligible();
    let placeholder =
        bordered_composer_placeholder(app, &context, focused, composer_empty, ghost_visible);
    let Some(resolved) = super::presentation::resolve_composer(
        app,
        &composer_text,
        context.dock.composer_focused,
        context.dock.composer_disabled,
        context.dock.variant == crate::view_model::ControlDockVariant::Startup,
        placeholder,
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
            (String::new(), border_style)
        } else {
            (
                format!(" {badge} "),
                Style::default()
                    .fg(live_composer_caption_color(theme, focused))
                    .bg(surface),
            )
        };
        let block = if badge_title.is_empty() {
            block
        } else {
            block.title_bottom(Line::from(Span::styled(badge_title, badge_style)).right_aligned())
        };
        block
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
        theme.terminal_colors.secondary
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
                    theme.terminal_colors.prompt_accent
                } else {
                    theme.terminal_colors.muted
                },
                focused,
            ))
            .bg(composer_surface)
    };

    let viewport = &resolved.viewport;

    let base_style = Style::default().fg(body_color).bg(composer_surface);
    let tag_style = base_style
        .fg(theme.status.warning)
        .add_modifier(Modifier::BOLD);
    let selection = super::file_tags::composer_selection(app);
    let plain_text = composer_empty
        || context.dock.composer_disabled
        || app.collapsed_paste_presentation().is_some();
    let body_lines = viewport
        .lines
        .iter()
        .zip(&viewport.line_starts)
        .enumerate()
        .map(|(row, (line, start))| {
            let body = if plain_text {
                Line::from(Span::styled(line.clone(), base_style))
            } else {
                composer_line_with_file_tags(
                    line,
                    *start,
                    &app.file_mention_tags,
                    base_style,
                    tag_style,
                    selection.clone(),
                )
            };
            let mut spans = vec![if row == 0 {
                Span::styled(glyph_prefix.clone(), glyph_style)
            } else {
                Span::styled(" ".repeat(glyph_cols), base_style)
            }];
            spans.extend(body.spans);
            if ghost_visible
                && viewport.cursor.is_some_and(|(cursor_row, cursor_col)| {
                    cursor_row == row && cursor_col == display_width(line)
                })
            {
                if let Some(ghost) = composer_view.ghost.as_ref() {
                    let cursor_col = viewport.cursor.map_or(0, |(_, cursor_col)| cursor_col);
                    let available_width = usize::from(inner.width)
                        .saturating_sub(glyph_cols.saturating_add(cursor_col));
                    spans.push(Span::styled(
                        super::ghost::truncate_to_width(&ghost.text, available_width),
                        ghost.style,
                    ));
                }
            }
            Line::from(spans)
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

fn composer_strip(area: Rect, content_lines: u16) -> Rect {
    Rect {
        height: area.height.min(content_lines.max(1).saturating_add(2)),
        ..area
    }
}

fn composer_text_area(strip: Rect) -> Rect {
    Rect::new(
        strip.x.saturating_add(4),
        strip.y.saturating_add(1),
        strip.width.saturating_sub(6),
        strip.height.saturating_sub(2),
    )
}

pub(crate) fn composer_input_viewport(
    app: &AppState,
    frame_area: Rect,
) -> Option<(Rect, ComposerViewport)> {
    let composer = FrameLayoutPlan::for_app(app, frame_area).composer?;
    let text = app.composer_render_text();
    let lines = if app.startup_shell_visible() {
        startup_composer_input_height(&text, composer.width, frame_area.height)
    } else {
        composer_input_height(&text, composer.width)
    };
    let input = composer_text_area(composer_strip(composer, lines));
    if input.width == 0 || input.height == 0 {
        return None;
    }
    let resolved = super::presentation::resolve_composer(
        app,
        &text,
        app.focus == Focus::Prompt,
        app.composer_disabled(),
        app.startup_shell_visible(),
        "",
        usize::from(input.width),
        usize::from(input.height),
        input.height.saturating_add(2),
        app.focus == Focus::Prompt,
    )?;
    Some((input, resolved.viewport))
}

fn bordered_composer_placeholder(
    app: &AppState,
    context: &DocumentComposerRenderContext<'_>,
    focused: bool,
    composer_empty: bool,
    ghost_visible: bool,
) -> &'static str {
    let live_empty_guidance = crate::ui::live_empty_composer_guidance_visible(app);
    let fallback = if focused { "" } else { "Build anything" };
    if ghost_visible {
        ""
    } else if context.dock.variant == crate::view_model::ControlDockVariant::Startup
        || context.dock.composer_disabled
        || !composer_empty
    {
        fallback
    } else if live_empty_guidance {
        "Ask Harness to inspect, edit, or explain…"
    } else if app.shell_mode() && focused {
        "run a shell command…"
    } else {
        fallback
    }
}

fn startup_composer_badge(badge: String) -> String {
    badge
        .strip_suffix("Demo")
        .map_or(badge.clone(), |prefix| format!("{prefix}Demo mode"))
}

pub(crate) fn connect_waiting_owns_input(app: &AppState) -> bool {
    app.connect_dialog.visible
        && app.connect_dialog.step == crate::app::auth_dialog::ConnectDialogStep::Waiting
}

pub(super) const fn live_composer_border_color(theme: &Theme, focused: bool) -> Color {
    if focused {
        theme.terminal_colors.prompt_border_active
    } else {
        theme.terminal_colors.prompt_border
    }
}

pub(crate) fn live_composer_content_color(theme: &Theme, color: Color, focused: bool) -> Color {
    if focused {
        color
    } else {
        blend_color(theme.terminal_colors.canvas, color, 0.66)
    }
}

fn live_composer_caption_color(theme: &Theme, focused: bool) -> Color {
    let opacity = if focused { 0.6 } else { 0.4 };
    blend_color(
        theme.terminal_colors.canvas,
        theme.terminal_colors.prompt_accent,
        opacity,
    )
}

#[cfg(test)]
mod active_thinking_color_tests {
    use super::*;

    #[test]
    fn empty_startup_badge_keeps_continuous_bottom_border_title() {
        // arrange
        // Given: startup has no model or supplemental identity to display.
        let badge = String::new();

        // act
        // When: startup reserves its right-aligned identity field.
        let rendered = startup_composer_badge(badge);

        // assert
        // Then: the empty title path remains truly empty so Ratatui draws the whole border.
        assert!(rendered.is_empty(), "empty badge became {rendered:?}");
    }
}
