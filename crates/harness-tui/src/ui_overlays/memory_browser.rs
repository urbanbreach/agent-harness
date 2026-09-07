use super::*;

pub(super) fn preview_rows(app: &AppState, popup: Rect) -> u16 {
    if !app.memory_browser.fullscreen && popup.width >= 60 && popup.height >= 16 {
        7
    } else {
        0
    }
}

pub(super) fn render_memory_browser_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    root: Rect,
) {
    let Some(model) = modal_surface_model(app, root) else {
        return;
    };
    if !paint_modal_panel(frame, app, theme, model.popup, model.key, "Memory") {
        return;
    }
    let inner = inset_rect(model.popup, 1, 1);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let muted = Style::default().fg(theme.text.secondary);
    frame.render_widget(
        Paragraph::new(format!(
            "{} {}",
            if app.memory_browser.filtering {
                "Filter:"
            } else {
                "Search /"
            },
            app.memory_browser.filter_input
        ))
        .style(muted),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    let entries = app.memory_browser.filtered_entries();
    for region in &model.regions {
        if let ModalTarget::Row(index) = region.target {
            let Some(entry) = entries.get(index) else {
                continue;
            };
            let row = modal_list_row(
                theme,
                ModalListRowSpec {
                    area: region.area,
                    state: ModalListRowState {
                        selected: index == app.memory_browser.selected,
                        hovered: app.modal_target_hovered(model.key, region.target),
                        dimmed: false,
                    },
                    max_scroll: model.max_scroll,
                },
            );
            frame.render_widget(
                Paragraph::new(format!(
                    "{}  {}",
                    entry.id,
                    entry.label.lines().next().unwrap_or_default()
                ))
                .style(row.style),
                row.layout.content,
            );
        }
    }
    let preview_height = if app.memory_browser.fullscreen {
        inner.height.saturating_sub(2)
    } else {
        preview_rows(app, model.popup)
    };
    if preview_height > 0 {
        let area = Rect::new(
            inner.x,
            inner.bottom().saturating_sub(1 + preview_height),
            inner.width,
            preview_height,
        );
        if let Some(entry) = app.memory_browser.selected_entry() {
            let mut lines = Vec::new();
            ui_markdown::append_rich_text_block(
                &mut lines,
                &entry.label,
                theme.text.primary,
                "",
                theme,
                area.width,
            );
            let max_scroll = lines.len().saturating_sub(usize::from(area.height));
            let scroll = usize::from(app.memory_browser.preview_scroll).min(max_scroll);
            frame.render_widget(
                Paragraph::new(lines.into_iter().skip(scroll).collect::<Vec<_>>())
                    .style(Style::default().fg(theme.text.primary)),
                area,
            );
        }
    }
    if entries.is_empty() && inner.height > 2 {
        let message = if app.replay_mode {
            "Workspace memory is unavailable during replay"
        } else {
            "No matching memory entries"
        };
        frame.render_widget(
            Paragraph::new(message).style(muted),
            Rect::new(inner.x, inner.y + 1, inner.width, 1),
        );
    }
    if inner.height > 2 {
        frame.render_widget(
            Paragraph::new("/ filter · Ctrl+F full view · Ctrl+C copy value · Esc return")
                .style(muted),
            Rect::new(inner.x, inner.bottom() - 1, inner.width, 1),
        );
    }
}
