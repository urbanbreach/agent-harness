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

    let overlay = modal_chrome::centered_popup(root, 48, 88, 10, 28);
    let chrome = modal_chrome::settings_chrome(app.settings_editor_tab());
    let surface = ui_chrome::command_palette_surface(theme);
    let muted_style = Style::default().fg(theme.text.secondary).bg(surface);
    let key = ModalSurfaceKey::Overlay {
        kind: OverlayKind::SettingsEditor,
        view: ModalViewKey::Primary,
    };

    if !paint_modal_panel(frame, app, theme, overlay, key, chrome.title) {
        return;
    }
    modal_chrome::render_body(frame, theme, overlay, chrome);
    let inner = inset_rect(overlay, 1.min(overlay.width.saturating_sub(1)), 1);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let summary = app.settings_editor_summary();
    let summary_line = summary.overlay_line();
    let summary_area = Rect::new(
        overlay.x.saturating_add(2),
        overlay.y.saturating_add(3),
        overlay.width.saturating_sub(4),
        1,
    );
    if inner.height >= 4 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(summary_line, muted_style))),
            summary_area,
        );
    }

    let rows = app.settings_editor_rows();
    let list_y = overlay.y.saturating_add(4);
    let list_bottom = overlay.bottom().saturating_sub(2);
    let list_height = list_bottom.saturating_sub(list_y);

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
    let max_scroll = rows.len().saturating_sub(visible_rows);
    let list_area = Rect::new(inner.x, list_y, inner.width, list_height);

    for (row, entry) in rows.iter().enumerate().skip(scroll).take(visible_rows) {
        let row_y = list_y + u16::try_from(row - scroll).unwrap_or(u16::MAX);
        let row_area = Rect::new(inner.x, row_y, inner.width, 1);
        let is_selected = row == selected;
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
        let list_width = usize::from(presentation.layout.content.width);
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

        let id_style = modal_list_row_text_style(style, theme.text.primary);
        let meta_style = modal_list_row_text_style(style, theme.text.tertiary);

        let pad = list_width
            .saturating_sub(id_text.chars().count())
            .saturating_sub(meta.chars().count());
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!(" {id_text}"), id_style),
                Span::styled(" ".repeat(pad.saturating_sub(1)), style),
                Span::styled(meta, meta_style),
            ])),
            presentation.layout.content,
        );
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
