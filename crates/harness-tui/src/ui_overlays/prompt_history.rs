use super::*;

pub(super) fn render(frame: &mut Frame, app: &AppState, theme: &Theme, root: Rect) {
    let Some(model) = modal_surface_model(app, root) else {
        return;
    };
    if !paint_modal_panel(frame, app, theme, model.popup, model.key, "Prompt history") {
        return;
    }
    let inner = inset_rect(model.popup, 1, 1);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new(format!("Search: {}", app.prompt_history_picker.query))
            .style(Style::default().fg(theme.text.primary)),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    let entries = app.prompt_history_matches();
    for region in &model.regions {
        if let ModalTarget::Row(index) = region.target {
            let Some(text) = entries.get(index) else {
                continue;
            };
            let presentation = modal_list_row(
                theme,
                ModalListRowSpec {
                    area: region.area,
                    state: ModalListRowState {
                        selected: app.prompt_history_picker.selected == index,
                        hovered: app.modal_target_hovered(model.key, region.target),
                        dimmed: false,
                    },
                    max_scroll: model.max_scroll,
                },
            );
            frame.render_widget(
                Paragraph::new(text.replace('\n', " ↵ ")).style(presentation.style),
                presentation.layout.content,
            );
        }
    }
    if entries.is_empty() && inner.height > 1 {
        frame.render_widget(
            Paragraph::new("No matching prompts").style(Style::default().fg(theme.text.secondary)),
            Rect::new(inner.x, inner.y + 1, inner.width, 1),
        );
    }
    if inner.height > 2 {
        frame.render_widget(
            Paragraph::new("↑↓ select · Enter use · Esc return")
                .style(Style::default().fg(theme.text.secondary)),
            Rect::new(inner.x, inner.bottom() - 1, inner.width, 1),
        );
    }
}
