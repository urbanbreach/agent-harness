use super::*;

pub(super) fn render(frame: &mut Frame, app: &AppState, theme: &Theme, root: Rect) {
    let Some(model) = modal_surface_model(app, root) else {
        return;
    };
    if !paint_modal_panel(
        frame,
        app,
        theme,
        model.popup,
        model.key,
        app.product_info.title,
    ) {
        return;
    }
    let inner = inset_rect(model.popup, 1, 1);
    if inner.height == 0 || inner.width == 0 {
        return;
    }
    let muted = Style::default().fg(theme.text.secondary);
    frame.render_widget(
        Paragraph::new(format!("Search: {}", app.product_info.query)).style(muted),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    let entries = app.product_info.matches();
    for region in &model.regions {
        if let ModalTarget::Row(index) = region.target {
            if let Some((label, _)) = entries.get(index) {
                let row = modal_list_row(
                    theme,
                    ModalListRowSpec {
                        area: region.area,
                        state: ModalListRowState {
                            selected: index == app.product_info.selected,
                            hovered: app.modal_target_hovered(model.key, region.target),
                            dimmed: false,
                        },
                        max_scroll: model.max_scroll,
                    },
                );
                frame.render_widget(
                    Paragraph::new(crate::ui::safe_product_text(label)).style(row.style),
                    row.layout.content,
                );
            }
        }
    }
    let preview = preview_rows(model.popup);
    if let Some((_, text)) = entries
        .get(app.product_info.selected)
        .filter(|_| preview > 0)
    {
        let area = Rect::new(
            inner.x,
            inner.bottom().saturating_sub(preview + 1),
            inner.width,
            preview,
        );
        frame.render_widget(
            Paragraph::new(crate::ui::safe_product_text(text))
                .style(Style::default().fg(theme.text.primary))
                .wrap(Wrap { trim: false }),
            area,
        );
    }
    if inner.height > 1 {
        frame.render_widget(
            Paragraph::new("↑↓ browse · Ctrl+C copy · Esc return").style(muted),
            Rect::new(inner.x, inner.bottom() - 1, inner.width, 1),
        );
    }
}

pub(super) fn preview_rows(popup: Rect) -> u16 {
    popup.height.saturating_sub(6).min(7)
}
