use super::*;

pub(super) fn render_settings_editor_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    root: Rect,
) {
    if root.width == 0 || root.height == 0 {
        return;
    }

    render_overlay_dim_backdrop(frame, root);

    let overlay_width = root.width.clamp(48, 88);
    let overlay_height = root.height.clamp(10, 28);
    let overlay_x = root.x + (root.width.saturating_sub(overlay_width)) / 2;
    let overlay_y = root.y + (root.height.saturating_sub(overlay_height)) / 2;
    let overlay = Rect::new(overlay_x, overlay_y, overlay_width, overlay_height);

    let surface = ui_chrome::command_palette_surface(theme);
    let title_style = Style::default()
        .fg(theme.text.primary)
        .bg(surface)
        .add_modifier(Modifier::BOLD);
    let row_style = Style::default().bg(surface);
    let selected_style = ui_chrome::overlay_focus_row_style(theme);
    let muted_style = Style::default().fg(theme.text.secondary).bg(surface);
    let text_style = Style::default().fg(theme.text.primary).bg(surface);

    if !paint_modal_panel(
        frame,
        app,
        theme,
        overlay,
        ModalSurfaceKey::Overlay {
            kind: OverlayKind::SettingsEditor,
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

    let title = "Settings";
    let title_area = Rect::new(inner.x, inner.y, inner.width, 1);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(title, title_style),
            Span::styled(
                " ".repeat(usize::from(inner.width).saturating_sub(title.len() + 3)),
                Style::default().bg(surface),
            ),
            Span::styled("esc", muted_style),
        ])),
        title_area,
    );

    let summary = app.settings_editor_summary();
    let summary_line = summary.overlay_line();
    let summary_area = Rect::new(inner.x, inner.y.saturating_add(1), inner.width, 1);
    if inner.height >= 2 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(summary_line, muted_style))),
            summary_area,
        );
    }

    let rows = app.settings_editor_rows();
    let list_y = inner.y.saturating_add(2);
    let list_width = usize::from(inner.width);
    let list_height = inner.height.saturating_sub(2);

    if rows.is_empty() {
        if list_height == 0 {
            return;
        }
        let empty_area = Rect::new(inner.x + 1, list_y, inner.width.saturating_sub(2), 1);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No settings registered",
                muted_style,
            ))),
            empty_area,
        );
        return;
    }

    let visible_rows = usize::from(list_height);
    let selected = app
        .settings_editor_selected_index()
        .min(rows.len().saturating_sub(1));
    let default_scroll = selected.saturating_sub(visible_rows.saturating_sub(1));
    let scroll = app.modal_visual_offset(
        ModalSurfaceKey::Overlay {
            kind: OverlayKind::SettingsEditor,
            view: ModalViewKey::Primary,
        },
        default_scroll,
        rows.len().saturating_sub(visible_rows),
    );

    for (row, entry) in rows.iter().enumerate().skip(scroll).take(visible_rows) {
        let row_y = list_y + u16::try_from(row - scroll).unwrap_or(u16::MAX);
        let row_area = Rect::new(inner.x, row_y, inner.width, 1);
        let is_selected = row == selected;
        let style = if is_selected {
            selected_style
        } else {
            row_style
        };
        frame.render_widget(Block::default().style(style), row_area);

        let meta = format!(
            "{} · {}{}",
            entry.surface,
            entry.sensitivity,
            if entry.editable { " · edit" } else { "" }
        );
        let label = match entry.effective_value.as_deref() {
            Some(value) => format!("{} = {}", entry.setting_id, value),
            None => entry.setting_id.clone(),
        };
        let id_budget = list_width.saturating_sub(meta.chars().count() + 3);
        let id_text = if label.chars().count() > id_budget {
            let mut out = label
                .chars()
                .take(id_budget.saturating_sub(1))
                .collect::<String>();
            out.push('…');
            out
        } else {
            label
        };

        let id_style = if is_selected {
            Style::default()
                .fg(theme.text.inverse)
                .bg(style.bg.unwrap_or(surface))
        } else {
            text_style
        };
        let meta_style = if is_selected {
            Style::default()
                .fg(theme.text.inverse)
                .bg(style.bg.unwrap_or(surface))
        } else {
            muted_style
        };

        let pad = list_width
            .saturating_sub(id_text.chars().count())
            .saturating_sub(meta.chars().count());
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!(" {id_text}"), id_style),
                Span::styled(" ".repeat(pad.saturating_sub(1)), style),
                Span::styled(meta, meta_style),
            ])),
            row_area,
        );
    }
}
