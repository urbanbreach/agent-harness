use super::*;

pub(super) fn render_prompt_stash_list_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    root: Rect,
) {
    if root.width == 0 || root.height == 0 {
        return;
    }

    render_overlay_dim_backdrop(frame, root);

    let overlay_width = root.width.clamp(40, 80);
    let overlay_height = root.height.clamp(8, 24);
    let overlay_x = root.x + (root.width.saturating_sub(overlay_width)) / 2;
    let overlay_y = root.y + (root.height.saturating_sub(overlay_height)) / 2;
    let overlay = Rect::new(overlay_x, overlay_y, overlay_width, overlay_height);

    let surface = ui_chrome::command_palette_surface(theme);
    let title_style = Style::default()
        .fg(theme.text.primary)
        .bg(surface)
        .add_modifier(Modifier::BOLD);
    let muted_style = Style::default().fg(theme.text.secondary).bg(surface);

    if !paint_modal_panel(
        frame,
        app,
        theme,
        overlay,
        ModalSurfaceKey::Overlay {
            kind: OverlayKind::PromptStashList,
            view: ModalViewKey::Primary,
        },
        "Commands",
    ) {
        return;
    }
    let inner = inset_rect(overlay, 1.min(overlay.width.saturating_sub(1)), 1);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let title_area = Rect::new(inner.x, inner.y, inner.width, 1);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Prompt stash", title_style),
            Span::styled(
                " ".repeat(usize::from(inner.width).saturating_sub("Prompt stash".len() + 3)),
                Style::default().bg(surface),
            ),
            Span::styled("esc", muted_style),
        ])),
        title_area,
    );

    let list_y = inner.y.saturating_add(1);

    if app.prompt_stash.entries.is_empty() {
        let empty_area = Rect::new(inner.x + 1, list_y, inner.width.saturating_sub(2), 1);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled("No stashed prompts", muted_style))),
            empty_area,
        );
    } else {
        let visible_rows = usize::from(inner.height.saturating_sub(2));
        let selected = app
            .prompt_stash
            .list_selected
            .min(app.prompt_stash.entries.len().saturating_sub(1));
        let default_scroll = selected.saturating_sub(visible_rows.saturating_sub(1));
        let scroll = app.modal_visual_offset(
            ModalSurfaceKey::Overlay {
                kind: OverlayKind::PromptStashList,
                view: ModalViewKey::Primary,
            },
            default_scroll,
            app.prompt_stash.entries.len().saturating_sub(visible_rows),
        );
        let max_scroll = app.prompt_stash.entries.len().saturating_sub(visible_rows);
        let list_area = Rect::new(inner.x, list_y, inner.width, inner.height.saturating_sub(2));

        for (row, entry) in app
            .prompt_stash
            .entries
            .iter()
            .enumerate()
            .skip(scroll)
            .take(visible_rows)
        {
            let row_y = list_y + u16::try_from(row - scroll).unwrap_or(u16::MAX);
            let row_area = Rect::new(inner.x, row_y, inner.width, 1);
            let is_selected = row == selected;
            let key = ModalSurfaceKey::Overlay {
                kind: OverlayKind::PromptStashList,
                view: ModalViewKey::Primary,
            };
            let presentation = modal_list_row(
                theme,
                ModalListRowSpec {
                    area: row_area,
                    state: ModalListRowState {
                        selected: is_selected,
                        hovered: app.modal_target_hovered(key, ModalTarget::Row(row)),
                        dimmed: false,
                    },
                    max_scroll,
                },
            );
            let style = presentation.style;
            frame.render_widget(Block::default().style(style), presentation.layout.content);

            let timestamp = format_timestamp_short(entry.timestamp);
            let list_width = usize::from(presentation.layout.content.width);
            let preview = preview_text(
                &entry.text,
                list_width.saturating_sub(timestamp.chars().count() + 4),
            );

            let preview_style = modal_list_row_text_style(style, theme.text.primary);
            let timestamp_style = modal_list_row_text_style(style, theme.text.tertiary);

            let prefix = "  ";
            let line = Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(preview, preview_style),
                Span::styled(" ".to_string(), style),
                Span::styled(timestamp, timestamp_style),
            ]);
            frame.render_widget(Paragraph::new(line), presentation.layout.content);
        }
        render_modal_list_scrollbar(
            frame,
            theme,
            ModalListScrollbarSpec {
                area: list_area,
                offset: scroll,
                max_scroll,
            },
        );
    }

    let hint_y = inner.y + inner.height.saturating_sub(1);
    if hint_y < overlay.y + overlay.height {
        let hint_area = Rect::new(overlay.x + 1, hint_y, overlay.width.saturating_sub(2), 1);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " Enter restore · Ctrl+D delete · Esc close",
                muted_style,
            ))),
            hint_area,
        );
    }
}

fn preview_text(text: &str, max_width: usize) -> String {
    let single_line = text.replace('\n', " ");
    let collapsed = single_line.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_plain_text(&collapsed, max_width)
}

fn format_timestamp_short(timestamp_millis: u64) -> String {
    let secs = timestamp_millis / 1000;
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let minutes = (secs % 3_600) / 60;
    if days > 0 {
        format!("{days}d {hours:02}h")
    } else if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        "now".to_string()
    }
}
