// allow: SIZE_OK — TUI overlay rendering (indivisible view model)
use super::*;

use crate::text::has_trimmed_content;

pub(super) fn render_model_switcher_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    overlay: Rect,
    title: &str,
) {
    if overlay.width <= 8 || overlay.height <= 5 {
        return;
    }

    let header = Rect::new(
        overlay.x.saturating_add(4),
        overlay.y.saturating_add(1),
        overlay.width.saturating_sub(8),
        1,
    );
    let input = Rect::new(
        overlay.x.saturating_add(4),
        overlay.y.saturating_add(3),
        overlay.width.saturating_sub(8),
        1,
    );
    let list = Rect::new(
        overlay.x.saturating_add(1),
        overlay.y.saturating_add(5),
        overlay.width.saturating_sub(2),
        overlay.height.saturating_sub(6),
    );
    let visible_model_rows = u16::try_from(app.model_switcher_visual_row_count())
        .unwrap_or(u16::MAX)
        .min(list.height.saturating_sub(1));
    let status = Rect::new(
        overlay.x.saturating_add(4),
        list.y.saturating_add(visible_model_rows).saturating_add(1),
        overlay.width.saturating_sub(8),
        1,
    );

    render_command_palette_header(frame, theme, header, title);
    render_command_palette_input(frame, app, theme, input);
    render_model_switcher_list(frame, app, theme, list);
    render_model_switcher_status(frame, app, theme, status);
}

fn render_model_switcher_status(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    frame.render_widget(Paragraph::new(model_switcher_status_line(app, theme)), area);
}

fn model_switcher_status_line(app: &AppState, theme: &Theme) -> Line<'static> {
    let surface = ui_chrome::command_palette_surface(theme);
    let muted = Style::default()
        .fg(ui_chrome::command_palette_muted(theme))
        .bg(surface);
    let text = if app.launch_metadata().available_models().is_empty()
        && app.launch_metadata().model().is_none()
    {
        "Connect a provider with /connect or /auth to list models"
    } else {
        "No automatic model fallback; provider errors stay visible"
    };
    Line::from(Span::styled(text, muted))
}

fn render_model_switcher_list(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = ui_chrome::command_palette_surface(theme);
    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);

    let rows = model_switcher_rows(app);
    if rows.is_empty() {
        let empty_area = Rect::new(
            area.x.saturating_add(3),
            area.y,
            area.width.saturating_sub(3),
            1,
        );
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                if app.launch_metadata().available_models().is_empty()
                    && app.launch_metadata().model().is_none()
                {
                    "Connect a provider to list models"
                } else {
                    "No results found"
                },
                Style::default()
                    .fg(ui_chrome::command_palette_muted(theme))
                    .bg(surface),
            ))),
            empty_area,
        );
        return;
    }

    let visible_rows = usize::from(area.height).max(1);
    let selected = app
        .model_selected
        .min(app.model_filtered.len().saturating_sub(1));
    let selected_row = rows
        .iter()
        .position(|row| matches!(row, ModelSwitcherRow::Option { filtered_index, .. } if *filtered_index == selected))
        .unwrap_or(0);
    let default_scroll = selected_row.saturating_sub(visible_rows.saturating_sub(1));
    let scroll = app.modal_visual_offset(
        ModalSurfaceKey::Overlay {
            kind: OverlayKind::CommandPalette,
            view: ModalViewKey::ModelSwitcher,
        },
        default_scroll,
        rows.len().saturating_sub(visible_rows),
    );

    for (row_index, row) in rows.iter().enumerate().skip(scroll).take(visible_rows) {
        let row_y = area
            .y
            .saturating_add(u16::try_from(row_index - scroll).unwrap_or(u16::MAX));
        let row_area = Rect::new(area.x, row_y, area.width, 1);
        match row {
            ModelSwitcherRow::Spacer => {
                frame.render_widget(
                    Block::default().style(Style::default().bg(surface)),
                    row_area,
                );
            }
            ModelSwitcherRow::Category(category) => {
                frame.render_widget(
                    Paragraph::new(command_palette_section_row(
                        category,
                        theme,
                        row_area.width,
                        false,
                    )),
                    row_area,
                );
            }
            ModelSwitcherRow::Option {
                filtered_index,
                option_index,
            } => {
                let Some(option) = app.model_options.get(*option_index) else {
                    continue;
                };
                let is_selected = *filtered_index == selected;
                if is_selected {
                    frame.render_widget(
                        Block::default().style(ui_chrome::overlay_focus_row_style(theme)),
                        row_area,
                    );
                }
                frame.render_widget(
                    Paragraph::new(model_switcher_row(
                        option,
                        app,
                        is_selected,
                        has_trimmed_content(&app.palette_input),
                        theme,
                        row_area.width,
                    )),
                    row_area,
                );
            }
        }
    }
}

pub(super) fn model_switcher_overlay_title(app: &AppState) -> String {
    let _ = app;
    "Select model".to_string()
}

fn model_switcher_row(
    option: &crate::app::ModelOption,
    app: &AppState,
    is_selected: bool,
    flatten: bool,
    theme: &Theme,
    width: u16,
) -> Line<'static> {
    let surface = ui_chrome::command_palette_surface(theme);
    let row_style = if is_selected {
        ui_chrome::overlay_focus_row_style(theme)
    } else {
        Style::default().bg(surface)
    };
    let selected_fg = ui_chrome::command_palette_selection_fg(theme);
    let title_style = if is_selected {
        row_style.fg(selected_fg).add_modifier(Modifier::BOLD)
    } else if app.is_current_model_option(option) {
        row_style.fg(ui_chrome::command_palette_selection_bg(theme))
    } else {
        row_style.fg(ui_chrome::command_palette_title(theme))
    };
    let meta_style = if is_selected {
        row_style.fg(selected_fg)
    } else {
        row_style.fg(ui_chrome::command_palette_muted(theme))
    };

    let row_width = usize::from(width);
    let mut spans = Vec::new();
    let is_current = app.is_current_model_option(option);

    let prefix = "  ";
    spans.push(Span::styled(prefix, row_style));
    let mut used_width = 2usize;

    // Current-model marker: ● for current, spaces for non-current (keeps titles aligned).
    if is_current && used_width < row_width {
        let marker_style = if is_selected {
            row_style.fg(selected_fg)
        } else {
            row_style.fg(ui_chrome::command_palette_selection_bg(theme))
        };
        spans.push(Span::styled("●", marker_style));
        spans.push(Span::styled(" ", row_style));
        used_width = used_width.saturating_add(2);
    } else if used_width < row_width {
        spans.push(Span::styled("  ", row_style));
        used_width = used_width.saturating_add(2);
    }

    let title_padding = 1usize.min(row_width.saturating_sub(used_width));
    if title_padding > 0 {
        spans.push(Span::styled(" ".repeat(title_padding), row_style));
        used_width = used_width.saturating_add(title_padding);
    }

    let footer = flatten.then(|| option.selector_category());
    let footer_width = footer.map(str::chars).map(Iterator::count).unwrap_or(0);
    let title_budget = row_width
        .saturating_sub(used_width)
        .saturating_sub(footer_width)
        .saturating_sub(usize::from(footer_width > 0))
        .min(61);
    let title = truncate_plain_text(option.selector_title(), title_budget);
    used_width = used_width.saturating_add(title.chars().count());
    spans.push(Span::styled(title, title_style));

    if let Some(footer) = footer {
        let gap = row_width
            .saturating_sub(used_width)
            .saturating_sub(footer_width);
        if gap > 0 {
            spans.push(Span::styled(" ".repeat(gap), row_style));
            used_width = used_width.saturating_add(gap);
        }
        if used_width < row_width {
            spans.push(Span::styled(
                truncate_plain_text(footer, row_width.saturating_sub(used_width)),
                meta_style,
            ));
            used_width = row_width;
        }
    }

    if used_width < row_width {
        spans.push(Span::styled(" ".repeat(row_width - used_width), row_style));
    }

    Line::from(spans)
}

pub(super) enum ModelSwitcherRow {
    Spacer,
    Category(String),
    Option {
        filtered_index: usize,
        option_index: usize,
    },
}

pub(super) fn model_switcher_rows(app: &AppState) -> Vec<ModelSwitcherRow> {
    if !has_trimmed_content(&app.palette_input) {
        let mut rows = Vec::new();
        let mut previous_category: Option<String> = None;
        for (filtered_index, option_index) in app.model_filtered.iter().copied().enumerate() {
            let Some(option) = app.model_options.get(option_index) else {
                continue;
            };
            let category = option.selector_category().to_string();
            if previous_category.as_deref() != Some(category.as_str()) {
                if previous_category.is_some() {
                    rows.push(ModelSwitcherRow::Spacer);
                }
                rows.push(ModelSwitcherRow::Category(category.clone()));
                previous_category = Some(category);
            }
            rows.push(ModelSwitcherRow::Option {
                filtered_index,
                option_index,
            });
        }
        return rows;
    }

    app.model_filtered
        .iter()
        .copied()
        .enumerate()
        .map(|(filtered_index, option_index)| ModelSwitcherRow::Option {
            filtered_index,
            option_index,
        })
        .collect()
}
