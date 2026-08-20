use super::*;

pub(super) fn render_plan_view_overlay(
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
    let muted_style = Style::default().fg(theme.text.secondary).bg(surface);
    let text_style = Style::default().fg(theme.text.primary).bg(surface);

    if !paint_modal_panel(
        frame,
        app,
        theme,
        overlay,
        ModalSurfaceKey::Overlay {
            kind: OverlayKind::PlanView,
            view: if app.plan_view_preview().is_some() {
                ModalViewKey::PlanPreview
            } else {
                ModalViewKey::Primary
            },
        },
        "Commands",
    ) {
        return;
    }
    let inner = inset_rect(overlay, 1.min(overlay.width.saturating_sub(1)), 1);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let preview = app.plan_view_preview();
    let summary = app.plan_view_summary();
    let title = if preview.is_some() {
        "Plan preview"
    } else {
        "Plans"
    };
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

    let summary_line = summary.overlay_line();
    let summary_area = Rect::new(inner.x, inner.y.saturating_add(1), inner.width, 1);
    if inner.height >= 2 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(summary_line, muted_style))),
            summary_area,
        );
    }

    let list_y = inner.y.saturating_add(2);
    let list_width = usize::from(inner.width);
    let list_height = inner.height.saturating_sub(2);

    if let Some(body) = preview {
        let preview_area = Rect::new(inner.x, list_y, inner.width, list_height);
        if preview_area.width == 0 || preview_area.height == 0 {
            return;
        }
        let max_lines = usize::from(preview_area.height);
        let rendered: Vec<Line> = body
            .lines()
            .take(max_lines)
            .map(|line| {
                let truncated = if line.chars().count() > list_width {
                    let mut out: String = line.chars().take(list_width.saturating_sub(1)).collect();
                    out.push('…');
                    out
                } else {
                    line.to_string()
                };
                Line::from(Span::styled(truncated, text_style))
            })
            .collect();
        frame.render_widget(Paragraph::new(rendered), preview_area);
        return;
    }

    let rows = app.plan_view_rows();

    if rows.is_empty() {
        if list_height == 0 {
            return;
        }
        let empty_area = Rect::new(inner.x + 1, list_y, inner.width.saturating_sub(2), 1);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled("No plan files yet", muted_style))),
            empty_area,
        );
        return;
    }

    let visible_rows = usize::from(list_height);
    let selected = app
        .plan_view_selected_index()
        .min(rows.len().saturating_sub(1));
    let default_scroll = selected.saturating_sub(visible_rows.saturating_sub(1));
    let scroll = app.modal_visual_offset(
        ModalSurfaceKey::Overlay {
            kind: OverlayKind::PlanView,
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
        let key = ModalSurfaceKey::Overlay {
            kind: OverlayKind::PlanView,
            view: ModalViewKey::Primary,
        };
        let presentation = modal_list_row(
            theme,
            ModalListRowSpec {
                area: row_area,
                state: ModalListRowState {
                    selected: is_selected,
                    hovered: app.modal_target_hovered(key, ModalTarget::Row(row)),
                    dimmed: !entry.exists,
                },
                max_scroll,
            },
        );
        let style = presentation.style;
        frame.render_widget(Block::default().style(style), presentation.layout.content);

        let meta = match (entry.is_active, entry.exists) {
            (true, true) => "active",
            (true, false) => "active · missing",
            (false, true) => "saved",
            (false, false) => "missing",
        };
        let row_width = usize::from(presentation.layout.content.width);
        let id_budget = row_width.saturating_sub(meta.chars().count() + 3);
        let id_text = if entry.path.chars().count() > id_budget {
            let mut out = entry
                .path
                .chars()
                .take(id_budget.saturating_sub(1))
                .collect::<String>();
            out.push('…');
            out
        } else {
            entry.path.clone()
        };

        let id_style = modal_list_row_text_style(style, theme.text.primary);
        let meta_style = modal_list_row_text_style(style, theme.text.tertiary);

        let pad = row_width
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
