use super::*;

pub(super) fn render_foreign_import_picker_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    root: Rect,
) {
    if root.width == 0 || root.height == 0 {
        return;
    }

    render_overlay_dim_backdrop(frame, root);

    let overlay_width = root.width.clamp(44, 96);
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
    let ok_style = Style::default().fg(theme.status.success).bg(surface);

    if !paint_modal_panel(
        frame,
        app,
        theme,
        overlay,
        ModalSurfaceKey::Overlay {
            kind: OverlayKind::ForeignImportPicker,
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

    let title = "Import foreign session";
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

    let width = usize::from(inner.width);
    let list_y = inner.y.saturating_add(1);
    let list_height = inner.height.saturating_sub(1);

    // Show last import summary as confirmation if present.
    if let Some(summary) = app.foreign_import_picker.last_import_summary.as_deref() {
        if list_height > 0 {
            let area = Rect::new(inner.x, list_y, inner.width, 1);
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    truncate_plain_text(summary, width),
                    ok_style,
                ))),
                area,
            );
        }
        return;
    }

    if let Some(error) = app.foreign_import_picker.error.as_deref() {
        if list_height > 0 {
            let area = Rect::new(inner.x, list_y, inner.width, 1);
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    truncate_plain_text(error, width),
                    muted_style,
                ))),
                area,
            );
        }
        return;
    }

    let candidates = &app.foreign_import_picker.candidates;
    if candidates.is_empty() {
        if list_height > 0 {
            let area = Rect::new(inner.x, list_y, inner.width, 1);
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "No foreign session candidates found",
                    muted_style,
                ))),
                area,
            );
        }
        return;
    }

    let visible_rows = usize::from(list_height);
    let default_scroll = app
        .foreign_import_picker
        .selected
        .saturating_sub(visible_rows.saturating_sub(1));
    let scroll = app.modal_visual_offset(
        ModalSurfaceKey::Overlay {
            kind: OverlayKind::ForeignImportPicker,
            view: ModalViewKey::Primary,
        },
        default_scroll,
        candidates.len().saturating_sub(visible_rows),
    );
    let max_scroll = candidates.len().saturating_sub(visible_rows);
    let list_area = Rect::new(inner.x, list_y, inner.width, list_height);
    for (visible_index, row_index) in (scroll..candidates.len()).take(visible_rows).enumerate() {
        let Some(candidate) = candidates.get(row_index) else {
            break;
        };
        let y = list_y.saturating_add(u16::try_from(visible_index).unwrap_or(u16::MAX));
        let area = Rect::new(inner.x, y, inner.width, 1);
        let is_selected = row_index == app.foreign_import_picker.selected;
        let key = ModalSurfaceKey::Overlay {
            kind: OverlayKind::ForeignImportPicker,
            view: ModalViewKey::Primary,
        };
        let presentation = modal_list_row(
            theme,
            ModalListRowSpec {
                area,
                state: ModalListRowState {
                    selected: is_selected,
                    hovered: app.modal_target_hovered(key, ModalTarget::Row(row_index)),
                    dimmed: !candidate.is_importable(),
                },
                max_scroll,
            },
        );

        let label = foreign_candidate_label(candidate);
        let status = if candidate.is_importable() {
            " [importable]"
        } else if candidate.is_corrupt() {
            " [corrupt]"
        } else {
            " [rejected]"
        };
        let row_width = usize::from(presentation.layout.content.width);
        let truncated = truncate_plain_text(
            &label,
            row_width
                .saturating_sub(status.chars().count())
                .saturating_sub(1),
        );

        let status_style = if candidate.is_importable() {
            Style::default()
                .fg(theme.status.success)
                .bg(presentation.style.bg.unwrap_or(surface))
        } else {
            Style::default()
                .fg(theme.text.tertiary)
                .bg(presentation.style.bg.unwrap_or(surface))
        };
        frame.render_widget(
            Block::default().style(presentation.style),
            presentation.layout.content,
        );
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(truncated, presentation.style),
                Span::styled(status, status_style),
                Span::styled(" ", presentation.style),
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

fn foreign_candidate_label(
    candidate: &harness_core::foreign_session::ForeignSessionCandidate,
) -> String {
    use harness_core::foreign_session::ForeignSessionCandidate;
    match candidate {
        ForeignSessionCandidate::Discoverable { kind, path, .. } => {
            format!("{} ({})", path.display(), kind.as_str())
        }
        ForeignSessionCandidate::Corrupt { kind, path, .. } => {
            format!("{} ({}, corrupt)", path.display(), kind.as_str())
        }
        ForeignSessionCandidate::Rejected { path, .. } => {
            format!("{} (rejected)", path.display())
        }
    }
}
