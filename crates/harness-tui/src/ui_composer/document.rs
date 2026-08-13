use super::*;

pub(crate) fn render_document_composer_content(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    context: DocumentComposerRenderContext<'_>,
) {
    let bordered_composer = matches!(
        context.dock.variant,
        crate::view_model::ControlDockVariant::Startup
            | crate::view_model::ControlDockVariant::Live
    );
    if bordered_composer {
        render_bordered_composer(frame, app, area, theme, context);
        return;
    }
    let surface = control_dock_surface(theme, context.dock.variant);
    let composer_surface = composer_input_surface(theme);
    let prompt_area = area;
    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);

    if prompt_area.width == 0 || prompt_area.height == 0 {
        return;
    }

    let main_columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(prompt_area);
    let rail_area = main_columns[0];
    let body_area = main_columns[1];

    let shell_rows = if body_area.height > 1 {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(body_area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(0)])
            .split(body_area)
    };
    let composer_body_area = shell_rows[0];

    frame.render_widget(
        Block::default().style(Style::default().bg(composer_surface)),
        composer_body_area,
    );

    if body_area.width == 0 || body_area.height == 0 {
        return;
    }

    let body_inner = inset_rect(
        composer_body_area,
        theme
            .live_shell
            .rhythm
            .composer_padding_x
            .min(composer_body_area.width.saturating_sub(1)),
        0,
    );
    if body_inner.width == 0 || body_inner.height == 0 {
        return;
    }

    let metadata_height = u16::from(body_inner.height >= 2);
    let metadata_gap = u16::from(metadata_height > 0 && body_inner.height >= 4);
    let top_padding = u16::from(
        body_inner.height
            >= context
                .composer_lines
                .saturating_add(metadata_height)
                .saturating_add(metadata_gap)
                .saturating_add(1),
    );
    let available_input_height = body_inner
        .height
        .saturating_sub(top_padding)
        .saturating_sub(metadata_gap)
        .saturating_sub(metadata_height)
        .max(1);
    let input_height = context
        .composer_lines
        .clamp(1, available_input_height)
        .max(1);
    let trailing_fill = body_inner
        .height
        .saturating_sub(top_padding)
        .saturating_sub(input_height)
        .saturating_sub(metadata_gap)
        .saturating_sub(metadata_height);

    let pre_input_fill = 0;
    let post_metadata_fill = trailing_fill;

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(top_padding.saturating_add(pre_input_fill)),
            Constraint::Length(input_height),
            Constraint::Length(metadata_gap),
            Constraint::Length(metadata_height),
            Constraint::Length(post_metadata_fill),
        ])
        .split(body_inner);
    let input_area = rows[1];
    let input_width = usize::from(input_area.width);
    let composer_text = app.composer_render_text();
    let placeholder_visible = composer_text.is_empty()
        && matches!(
            context.dock.variant,
            crate::view_model::ControlDockVariant::Startup
        );
    let placeholder = if placeholder_visible {
        context.dock.composer_body.as_str()
    } else if app.shell_mode() && composer_text.is_empty() {
        "run a shell command…"
    } else {
        ""
    };
    let show_cursor = !context.dock.composer_disabled
        && !footer_suppressed_by_overlay(app)
        && (placeholder_visible || context.dock.composer_focused);
    let Some(resolved) = super::presentation::resolve_composer(
        app,
        &composer_text,
        context.dock.composer_focused,
        context.dock.composer_disabled,
        context.dock.variant == crate::view_model::ControlDockVariant::Startup,
        placeholder,
        input_width,
        usize::from(input_area.height.max(1)),
        body_inner.height,
        show_cursor,
    ) else {
        return;
    };
    let mode_style = composer_mode_style(theme, resolved.tone, context.dock.composer_focused);
    let shell_mode_active = resolved.tone == crate::composer_integration::ComposerTone::Shell
        && !context.dock.composer_disabled;
    let body_color = if context.dock.composer_disabled {
        theme.status.disabled
    } else if placeholder_visible {
        theme.reference_terminal.secondary
    } else if shell_mode_active {
        theme.status.warning
    } else {
        composer_input_text(theme)
    };
    let glyph_style = if context.dock.composer_disabled {
        Style::default().fg(theme.status.disabled).bg(surface)
    } else if matches!(
        resolved.tone,
        crate::composer_integration::ComposerTone::Shell
            | crate::composer_integration::ComposerTone::Plan
    ) {
        Style::default().fg(mode_style.accent).bg(surface)
    } else {
        Style::default().fg(composer_input_text(theme)).bg(surface)
    };

    if rail_area.height > 0 && rail_area.width > 0 {
        let height = usize::from(rail_area.height);
        let mut rail_lines = Vec::with_capacity(height.max(1));
        if height > 0 {
            rail_lines.push(Line::from(Span::styled(
                resolved
                    .surface
                    .marker()
                    .unwrap_or(theme.live_shell.transcript_glyphs.user_marker),
                glyph_style,
            )));
            rail_lines.extend(
                std::iter::repeat_with(|| Line::from(Span::styled(" ", glyph_style)))
                    .take(height.saturating_sub(1)),
            );
        }
        frame.render_widget(
            Paragraph::new(rail_lines).style(Style::default().bg(surface)),
            rail_area,
        );
    }

    let viewport = &resolved.viewport;
    let base_style = Style::default().fg(body_color).bg(composer_surface);
    let tag_style = Style::default()
        .fg(theme.status.warning)
        .bg(composer_surface)
        .add_modifier(Modifier::BOLD);
    let body_lines = viewport
        .lines
        .iter()
        .zip(viewport.line_starts.iter().copied())
        .map(|(line, start)| {
            if placeholder_visible || context.dock.composer_disabled {
                Line::from(Span::styled(line.clone(), base_style))
            } else {
                composer_line_with_file_tags(
                    line,
                    start,
                    &app.file_mention_tags,
                    base_style,
                    tag_style,
                )
            }
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(body_lines).style(Style::default().bg(composer_surface)),
        input_area,
    );

    if !connect_waiting_owns_input(app) {
        if let Some((cursor_row, cursor_col)) = viewport.cursor {
            let cursor_x = input_area
                .x
                .saturating_add(u16::try_from(cursor_col).unwrap_or(u16::MAX));
            let cursor_y = input_area
                .y
                .saturating_add(u16::try_from(cursor_row).unwrap_or(u16::MAX));
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }

    if metadata_height > 0
        && rows[3].width > 0
        && resolved
            .chrome
            .contains(&crate::composer_integration::ComposerChrome::Metadata)
    {
        frame.render_widget(
            Paragraph::new(composer_metadata_line(
                app,
                context.dock,
                resolved.surface.right_label(),
                context.disclosure_visible,
                usize::from(rows[3].width),
                theme,
                composer_surface,
            ))
            .style(Style::default().bg(composer_surface)),
            rows[3],
        );
    }
}
