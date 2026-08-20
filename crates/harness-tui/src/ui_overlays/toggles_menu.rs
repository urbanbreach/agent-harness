use super::*;

pub(super) fn render_toggles_menu_list(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    area: Rect,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let list_area = area;
    if list_area.width == 0 || list_area.height == 0 {
        return;
    }

    frame.render_widget(
        Block::default().style(Style::default().bg(ui_chrome::command_palette_surface(theme))),
        list_area,
    );

    let rows = toggles_overlay_rows(app);
    if rows.is_empty() {
        render_palette_empty_message(frame, theme, list_area, "No toggles found");
        return;
    }

    let visible_rows = usize::from(list_area.height);
    let selected_row = rows
        .iter()
        .position(|row| matches!(row, TogglesOverlayRow::Toggle(toggle) if toggle.selected))
        .unwrap_or(0);
    let default_scroll = selected_row.saturating_sub(visible_rows.saturating_sub(1));
    let scroll = app.modal_visual_offset(
        ModalSurfaceKey::Overlay {
            kind: OverlayKind::TogglesMenu,
            view: ModalViewKey::Toggles,
        },
        default_scroll,
        rows.len().saturating_sub(visible_rows),
    );
    let max_scroll = rows.len().saturating_sub(visible_rows);
    let mut logical_index = rows
        .iter()
        .take(scroll)
        .filter(|row| matches!(row, TogglesOverlayRow::Toggle(_)))
        .count();

    for (row, toggle_row) in rows.iter().enumerate().skip(scroll).take(visible_rows) {
        let row_y = list_area
            .y
            .saturating_add(u16::try_from(row - scroll).unwrap_or(u16::MAX));
        let row_area = Rect::new(list_area.x, row_y, list_area.width, 1);
        match toggle_row {
            TogglesOverlayRow::Spacer => {
                frame.render_widget(
                    Block::default()
                        .style(Style::default().bg(ui_chrome::command_palette_surface(theme))),
                    modal_list_row_layout(row_area, max_scroll).content,
                );
            }
            TogglesOverlayRow::Section(section) => {
                let content = modal_list_row_layout(row_area, max_scroll).content;
                frame.render_widget(
                    Paragraph::new(command_palette_section_row(
                        section,
                        theme,
                        content.width,
                        false,
                    )),
                    content,
                );
            }
            TogglesOverlayRow::Toggle(toggle) => {
                let key = ModalSurfaceKey::Overlay {
                    kind: OverlayKind::TogglesMenu,
                    view: ModalViewKey::Toggles,
                };
                let presentation = modal_list_row(
                    theme,
                    ModalListRowSpec {
                        area: row_area,
                        state: ModalListRowState {
                            selected: toggle.selected,
                            hovered: app.modal_target_hovered(key, ModalTarget::Row(logical_index)),
                            dimmed: false,
                        },
                        max_scroll,
                    },
                );
                logical_index = logical_index.saturating_add(1);
                frame.render_widget(
                    Block::default().style(presentation.style),
                    presentation.layout.content,
                );
                frame.render_widget(
                    Paragraph::new(toggle_menu_row(
                        toggle,
                        theme,
                        presentation.layout.content.width,
                        presentation.style,
                    )),
                    presentation.layout.content,
                );
            }
        }
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

pub(super) enum TogglesOverlayRow {
    Spacer,
    Section(&'static str),
    Toggle(crate::app::ToggleMenuRow),
}

pub(super) fn toggles_overlay_rows(app: &AppState) -> Vec<TogglesOverlayRow> {
    let mut rows = Vec::new();
    let mut last_section = None;
    for toggle in app.toggle_menu_rows() {
        if Some(toggle.section) != last_section {
            if last_section.is_some() {
                rows.push(TogglesOverlayRow::Spacer);
            }
            rows.push(TogglesOverlayRow::Section(toggle.section));
            last_section = Some(toggle.section);
        }
        rows.push(TogglesOverlayRow::Toggle(toggle));
    }
    rows
}

fn toggle_menu_row(
    toggle: &crate::app::ToggleMenuRow,
    theme: &Theme,
    width: u16,
    row_style: Style,
) -> Line<'static> {
    let label_style = if toggle.selected {
        row_style.add_modifier(Modifier::BOLD)
    } else {
        row_style.fg(ui_chrome::command_palette_title(theme))
    };
    let description_style = if toggle.selected {
        row_style
    } else {
        row_style.fg(ui_chrome::command_palette_muted(theme))
    };
    let state = if toggle.enabled { "●" } else { "○" };
    let state_label = if toggle.enabled { "on" } else { "off" };
    let prefix = "  ";
    let label = format!("{prefix}{state} {}", sanitize_toggle_text(&toggle.label));
    let meta = format!(
        "{} · {state_label}",
        sanitize_toggle_text(&toggle.description)
    );
    split_title_meta_row(
        label,
        meta,
        label_style,
        description_style,
        row_style,
        width,
    )
}

fn sanitize_toggle_text(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_control())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn split_title_meta_row(
    title: String,
    meta: String,
    title_style: Style,
    meta_style: Style,
    row_style: Style,
    width: u16,
) -> Line<'static> {
    let row_width = usize::from(width);
    let meta_width = meta.chars().count().min(row_width / 2);
    let title_width = row_width.saturating_sub(meta_width).saturating_sub(1);
    let title = truncate_plain_text(&title, title_width);
    let used = title.chars().count();
    let gap = row_width.saturating_sub(used).saturating_sub(meta_width);
    let meta = truncate_plain_text(&meta, meta_width);
    Line::from(vec![
        Span::styled(title, title_style),
        Span::styled(" ".repeat(gap), row_style),
        Span::styled(meta, meta_style),
    ])
}

pub(super) struct YoloWarningLayout {
    pub(super) popup: Rect,
    pub(super) inner: Rect,
    pub(super) confirm: Rect,
    pub(super) cancel: Rect,
}

pub(super) fn yolo_warning_layout(overlay: Rect) -> Option<YoloWarningLayout> {
    let width = 54.min(overlay.width.saturating_sub(4));
    let height = 7.min(overlay.height);
    if width < 32 || height < 5 {
        return None;
    }
    let popup = Rect::new(
        overlay.x + overlay.width.saturating_sub(width) / 2,
        overlay.y + overlay.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let inner = inset_rect(popup, 2.min(popup.width.saturating_sub(1)), 1);
    if inner.width == 0 || inner.height == 0 {
        return None;
    }
    let footer_y = inner.y.saturating_add(4);
    Some(YoloWarningLayout {
        popup,
        inner,
        confirm: Rect::new(inner.x, footer_y, 13.min(inner.width), 1),
        cancel: Rect::new(
            inner.x.saturating_add(16.min(inner.width)),
            footer_y,
            10.min(inner.width.saturating_sub(16)),
            1,
        ),
    })
}

pub(super) fn render_yolo_warning_popup(frame: &mut Frame, theme: &Theme, overlay: Rect) {
    let Some(layout) = yolo_warning_layout(overlay) else {
        return;
    };
    if !paint_command_palette_panel(frame, theme, layout.popup) {
        return;
    }
    let surface = ui_chrome::command_palette_surface(theme);
    let text = Text::from(vec![
        Line::from(Span::styled(
            "Confirm YOLO mode",
            Style::default()
                .fg(theme.status.warning)
                .bg(surface)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "YOLO marks every menu entry on.",
            Style::default().fg(theme.text.primary).bg(surface),
        )),
        Line::from(Span::styled(
            "Coordinator permissions still apply.",
            Style::default().fg(theme.text.secondary).bg(surface),
        )),
        Line::default(),
        Line::from(Span::styled(
            "Enter confirm   Esc cancel",
            Style::default().fg(theme.text.secondary).bg(surface),
        )),
    ]);
    frame.render_widget(
        Paragraph::new(text)
            .style(Style::default().bg(surface))
            .wrap(Wrap { trim: true }),
        layout.inner,
    );
}
