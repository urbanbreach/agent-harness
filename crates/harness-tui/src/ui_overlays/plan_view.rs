use super::*;

pub(super) fn render_plan_view_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    root: Rect,
) {
    let popup = modal_chrome::centered_popup(root, 48, 88, 10, 28);
    let preview = app.plan_view_preview();
    let key = ModalSurfaceKey::Overlay {
        kind: OverlayKind::PlanView,
        view: if preview.is_some() {
            ModalViewKey::PlanPreview
        } else {
            ModalViewKey::Primary
        },
    };
    if !paint_modal_panel(frame, app, theme, popup, key, "Commands") {
        return;
    }
    let inner = inset_rect(popup, 1.min(popup.width.saturating_sub(1)), 1);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let surface = ui_chrome::command_palette_surface(theme);
    let text = Style::default().fg(theme.text.primary).bg(surface);
    let muted = text.fg(theme.text.secondary);
    let title = if preview.is_some() {
        "Plan preview"
    } else {
        "Plans"
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(title, text.add_modifier(Modifier::BOLD)),
            Span::styled(
                " ".repeat(usize::from(inner.width).saturating_sub(display_width(title) + 3)),
                Style::default().bg(surface),
            ),
            Span::styled("esc", muted),
        ])),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    if inner.height >= 2 {
        frame.render_widget(
            Paragraph::new(Span::styled(
                app.prepared_plan_summary().overlay_line(),
                muted,
            )),
            Rect::new(inner.x, inner.y.saturating_add(1), inner.width, 1),
        );
    }
    let body = Rect::new(
        inner.x,
        inner.y.saturating_add(2),
        inner.width,
        inner.height.saturating_sub(2),
    );
    if body.height == 0 {
        return;
    }
    if let Some(preview) = preview {
        let lines: Vec<Line> = preview
            .lines()
            .take(usize::from(body.height))
            .map(|line| {
                Line::from(Span::styled(
                    truncate_plain_text(line, usize::from(body.width)),
                    text,
                ))
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), body);
        return;
    }
    let entries = &app.plan_entries;
    if entries.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled("No plan files yet", muted)),
            Rect::new(body.x + 1, body.y, body.width.saturating_sub(2), 1),
        );
        return;
    }
    let visible = usize::from(body.height);
    let selected = app.plan_view_selected_index().min(entries.len() - 1);
    let max_scroll = entries.len().saturating_sub(visible);
    let offset = app.modal_visual_offset(key, selected.saturating_sub(visible - 1), max_scroll);
    for (index, entry) in entries.iter().enumerate().skip(offset).take(visible) {
        let row = Rect::new(
            body.x,
            body.y
                .saturating_add(u16::try_from(index - offset).unwrap_or(u16::MAX)),
            body.width,
            1,
        );
        let presentation = modal_list_row(
            theme,
            ModalListRowSpec {
                area: row,
                state: ModalListRowState {
                    selected: index == selected,
                    hovered: app.modal_target_hovered(key, ModalTarget::Row(index)),
                    dimmed: !entry.exists,
                },
                max_scroll,
            },
        );
        let area = presentation.layout.content;
        let style = presentation.style;
        frame.render_widget(Block::default().style(style), area);
        let meta = match (entry.is_active, entry.exists) {
            (true, true) => "active",
            (true, false) => "active · missing",
            (false, true) => "saved",
            (false, false) => "missing",
        };
        let width = usize::from(area.width);
        let meta_width = display_width(meta);
        let path = truncate_plain_text(&entry.path, width.saturating_sub(meta_width + 3).max(1));
        let padding = width
            .saturating_sub(display_width(&path))
            .saturating_sub(meta_width + 1);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(" {path}"),
                    modal_list_row_text_style(style, theme.text.primary),
                ),
                Span::styled(" ".repeat(padding), style),
                Span::styled(meta, modal_list_row_text_style(style, theme.text.tertiary)),
            ])),
            area,
        );
    }
    render_modal_list_scrollbar(
        frame,
        theme,
        ModalListScrollbarSpec {
            area: body,
            offset,
            max_scroll,
        },
    );
}
