use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use super::common::{
    horizontal_inset, left_inset, render_empty, render_option_row, visible_window,
};
use crate::app::auth_dialog::{
    is_popular_connect_provider, ConnectDialogState, ConnectProviderMenuItem,
};
use crate::theme::Theme;

#[derive(Debug, Clone, Copy)]
pub(super) enum ProviderRow {
    Category(&'static str),
    Item(ConnectProviderMenuItem),
}

pub(super) fn provider_rows(dialog: &ConnectDialogState) -> Vec<ProviderRow> {
    let mut rows = Vec::new();
    let items = dialog.provider_menu_items();
    push_provider_group(dialog, &items, true, "Popular", &mut rows);
    push_provider_group(dialog, &items, false, "Providers", &mut rows);
    rows
}

pub(super) fn render_provider_rows(
    frame: &mut Frame,
    theme: &Theme,
    dialog: &ConnectDialogState,
    rows: &[ProviderRow],
    area: Rect,
) {
    if rows.is_empty() {
        render_empty(frame, theme, area);
        return;
    }
    let selectable_positions = provider_selectable_positions(rows);
    let selected_row = selectable_positions
        .get(dialog.selected)
        .copied()
        .unwrap_or(0);
    for (offset, row_index) in visible_window(rows.len(), selected_row, area.height).enumerate() {
        let row_area = horizontal_inset(
            Rect::new(area.x, area.y.saturating_add(offset as u16), area.width, 1),
            1,
        );
        match rows[row_index] {
            ProviderRow::Category("") => {}
            ProviderRow::Category(label) => render_category(frame, theme, row_area, label),
            ProviderRow::Item(item) => {
                render_provider_option(
                    frame,
                    theme,
                    dialog,
                    row_area,
                    item,
                    row_index == selected_row,
                );
            }
        }
    }
}

fn push_provider_group(
    dialog: &ConnectDialogState,
    items: &[ConnectProviderMenuItem],
    popular: bool,
    label: &'static str,
    rows: &mut Vec<ProviderRow>,
) {
    let start_len = rows.len();
    for item in items {
        match item {
            ConnectProviderMenuItem::Provider(index) => {
                let Some(provider) = dialog.providers.get(*index) else {
                    continue;
                };
                if is_popular_connect_provider(provider) == popular {
                    rows.push(ProviderRow::Item(*item));
                }
            }
            ConnectProviderMenuItem::Custom => {
                if !popular {
                    rows.push(ProviderRow::Item(*item));
                }
            }
        }
    }
    if rows.len() > start_len {
        rows.insert(start_len, ProviderRow::Category(label));
        if start_len > 0 {
            rows.insert(start_len, ProviderRow::Category(""));
        }
    }
}

fn render_provider_option(
    frame: &mut Frame,
    theme: &Theme,
    dialog: &ConnectDialogState,
    area: Rect,
    item: ConnectProviderMenuItem,
    selected: bool,
) {
    let (title, description) = match item {
        ConnectProviderMenuItem::Provider(index) => {
            let Some(provider) = dialog.providers.get(index) else {
                return;
            };
            (
                provider.label.as_str(),
                (!provider.description.is_empty()).then_some(provider.description.as_str()),
            )
        }
        ConnectProviderMenuItem::Custom => ("Other", Some("Custom provider")),
    };
    render_option_row(frame, theme, area, selected, title, description);
}

fn render_category(frame: &mut Frame, theme: &Theme, area: Rect, label: &str) {
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            label,
            Style::default()
                .fg(theme.text.accent)
                .add_modifier(Modifier::BOLD),
        ))),
        left_inset(area, 3),
    );
}

fn provider_selectable_positions(rows: &[ProviderRow]) -> Vec<usize> {
    rows.iter()
        .enumerate()
        .filter_map(|(index, row)| matches!(row, ProviderRow::Item(_)).then_some(index))
        .collect()
}
