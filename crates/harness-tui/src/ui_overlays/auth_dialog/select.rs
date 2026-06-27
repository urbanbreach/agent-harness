use harness_core::auth::plugin::AuthMethodSpec;
use ratatui::{layout::Rect, Frame};

use super::common::{
    horizontal_inset, render_empty, render_option_row, render_panel, render_select_header,
    visible_select_rows, visible_window, SELECT_HEADER_HEIGHT,
};
use super::provider_rows::{provider_rows, render_provider_rows};
use crate::app::auth_dialog::{auth_method_label, ConnectDialogState};
use crate::app::AppState;
use crate::theme::Theme;

pub(super) fn render_provider_select(frame: &mut Frame, app: &AppState, theme: &Theme, root: Rect) {
    let dialog = &app.connect_dialog;
    let rows = provider_rows(dialog);
    let visible_rows = visible_select_rows(root, rows.len() as u16);
    let area = render_panel(frame, theme, root, SELECT_HEADER_HEIGHT + visible_rows + 1);
    render_select_header(
        frame,
        theme,
        area,
        "Connect a provider",
        &dialog.filter_buffer,
    );

    let body = Rect::new(
        area.x,
        area.y + SELECT_HEADER_HEIGHT,
        area.width,
        visible_rows,
    );
    render_provider_rows(frame, theme, dialog, &rows, body);
}

pub(super) fn render_method_select(frame: &mut Frame, app: &AppState, theme: &Theme, root: Rect) {
    let dialog = &app.connect_dialog;
    let method_indices = dialog.filtered_method_indices();
    let visible_rows = visible_select_rows(root, method_indices.len() as u16);
    let area = render_panel(frame, theme, root, SELECT_HEADER_HEIGHT + visible_rows + 1);
    render_select_header(
        frame,
        theme,
        area,
        "Select auth method",
        &dialog.filter_buffer,
    );

    let methods = dialog
        .selected_provider
        .and_then(|index| dialog.providers.get(index))
        .map(|provider| provider.methods.as_slice())
        .unwrap_or(&[]);
    let body = Rect::new(
        area.x,
        area.y + SELECT_HEADER_HEIGHT,
        area.width,
        visible_rows,
    );
    render_method_rows(
        frame,
        theme,
        methods,
        &method_indices,
        dialog.selected,
        body,
    );
}

pub(super) fn render_model_select(frame: &mut Frame, app: &AppState, theme: &Theme, root: Rect) {
    let dialog = &app.connect_dialog;
    let model_indices = dialog.filtered_model_indices();
    let total_rows = model_indices.len() + usize::from(dialog.skip_model_matches_filter());
    let visible_rows = visible_select_rows(root, total_rows as u16);
    let area = render_panel(frame, theme, root, SELECT_HEADER_HEIGHT + visible_rows + 1);
    let title = dialog
        .selected_provider
        .and_then(|index| dialog.providers.get(index))
        .map(|provider| provider.label.as_str())
        .unwrap_or("Select model");
    render_select_header(frame, theme, area, title, &dialog.filter_buffer);

    let body = Rect::new(
        area.x,
        area.y + SELECT_HEADER_HEIGHT,
        area.width,
        visible_rows,
    );
    render_model_rows(frame, theme, dialog, &model_indices, body);
}

fn render_method_rows(
    frame: &mut Frame,
    theme: &Theme,
    methods: &[AuthMethodSpec],
    method_indices: &[usize],
    selected: usize,
    area: Rect,
) {
    if method_indices.is_empty() {
        render_empty(frame, theme, area);
        return;
    }
    let selected_row = selected.min(method_indices.len().saturating_sub(1));
    for (offset, index) in
        visible_window(method_indices.len(), selected_row, area.height).enumerate()
    {
        let Some(method) = method_indices
            .get(index)
            .and_then(|index| methods.get(*index))
        else {
            continue;
        };
        render_option_row(
            frame,
            theme,
            horizontal_inset(
                Rect::new(area.x, area.y.saturating_add(offset as u16), area.width, 1),
                1,
            ),
            index == selected,
            auth_method_label(method),
            None,
        );
    }
}

fn render_model_rows(
    frame: &mut Frame,
    theme: &Theme,
    dialog: &ConnectDialogState,
    model_indices: &[usize],
    area: Rect,
) {
    let total = model_indices.len() + usize::from(dialog.skip_model_matches_filter());
    if total == 0 {
        render_empty(frame, theme, area);
        return;
    }
    let selected_row = dialog.selected.min(total.saturating_sub(1));
    for (offset, index) in visible_window(total, selected_row, area.height).enumerate() {
        let row_area = horizontal_inset(
            Rect::new(area.x, area.y.saturating_add(offset as u16), area.width, 1),
            1,
        );
        if let Some(model_index) = model_indices.get(index) {
            let Some(model) = dialog.models.get(*model_index) else {
                continue;
            };
            render_option_row(
                frame,
                theme,
                row_area,
                index == dialog.selected,
                model,
                None,
            );
        } else {
            render_option_row(
                frame,
                theme,
                row_area,
                index == dialog.selected,
                "Skip model selection",
                None,
            );
        }
    }
}
