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

    render_model_select_header(frame, theme, header, title);
    render_model_select_input(frame, app, theme, input);
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
    let surface = model_select_surface(theme);
    let muted = Style::default().fg(model_select_muted(theme)).bg(surface);
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

    let surface = model_select_surface(theme);
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
                Style::default().fg(model_select_muted(theme)).bg(surface),
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
    let scroll = selected_row.saturating_sub(visible_rows.saturating_sub(1));

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
                    Paragraph::new(model_switcher_category_row(category, theme, row_area.width)),
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
                frame.render_widget(
                    Block::default().style(model_switcher_option_row_style(theme, is_selected)),
                    row_area,
                );
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
    let row_style = model_switcher_option_row_style(theme, is_selected);
    let selected_fg = model_select_selected_fg(theme);
    let title_style = if is_selected {
        row_style.fg(selected_fg).add_modifier(Modifier::BOLD)
    } else if app.is_current_model_option(option) {
        row_style.fg(model_select_primary(theme))
    } else {
        row_style.fg(model_select_text(theme))
    };
    let meta_style = if is_selected {
        row_style.fg(selected_fg)
    } else {
        row_style.fg(model_select_muted(theme))
    };

    let row_width = usize::from(width);
    let mut spans = Vec::new();
    let is_current = app.is_current_model_option(option);
    let leading_padding = if is_current { 1 } else { 3 }.min(row_width);
    if leading_padding > 0 {
        spans.push(Span::styled(" ".repeat(leading_padding), row_style));
    }
    let mut used_width = leading_padding;

    if is_current && used_width < row_width {
        let marker_style = if is_selected {
            row_style.fg(selected_fg)
        } else {
            row_style.fg(model_select_primary(theme))
        };
        spans.push(Span::styled("●", marker_style));
        used_width = used_width.saturating_add(1);
    }

    let title_padding = 3.min(row_width.saturating_sub(used_width));
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

enum ModelSwitcherRow {
    Spacer,
    Category(String),
    Option {
        filtered_index: usize,
        option_index: usize,
    },
}

fn model_switcher_rows(app: &AppState) -> Vec<ModelSwitcherRow> {
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

pub(super) fn paint_model_select_panel(frame: &mut Frame, theme: &Theme, overlay: Rect) -> bool {
    if overlay.width == 0 || overlay.height == 0 {
        return false;
    }

    frame.render_widget(Clear, overlay);
    frame.render_widget(
        Block::default().style(Style::default().bg(model_select_surface(theme))),
        overlay,
    );
    true
}

fn render_model_select_header(frame: &mut Frame, theme: &Theme, area: Rect, title: &str) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = model_select_surface(theme);
    let esc = "esc";
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(u16::try_from(esc.chars().count()).unwrap_or(u16::MAX)),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(title.to_string()).style(
            Style::default()
                .fg(model_select_text(theme))
                .bg(surface)
                .add_modifier(Modifier::BOLD),
        ),
        columns[0],
    );
    frame.render_widget(
        Paragraph::new(esc)
            .alignment(Alignment::Right)
            .style(Style::default().fg(model_select_muted(theme)).bg(surface)),
        columns[1],
    );
}

fn render_model_select_input(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = model_select_surface(theme);
    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);

    let line = if app.palette_input.is_empty() {
        Line::from(vec![
            Span::styled(
                "█",
                Style::default().fg(model_select_primary(theme)).bg(surface),
            ),
            Span::styled(
                " Search",
                Style::default().fg(model_select_muted(theme)).bg(surface),
            ),
        ])
    } else {
        let cursor_byte = app
            .palette_input
            .char_indices()
            .nth(app.palette_cursor)
            .map(|(index, _)| index)
            .unwrap_or(app.palette_input.len());
        let before = &app.palette_input[..cursor_byte];
        let after = &app.palette_input[cursor_byte..];
        Line::from(vec![
            Span::styled(
                before.to_string(),
                Style::default().fg(model_select_muted(theme)).bg(surface),
            ),
            Span::styled(
                "█",
                Style::default().fg(model_select_primary(theme)).bg(surface),
            ),
            Span::styled(
                after.to_string(),
                Style::default().fg(model_select_muted(theme)).bg(surface),
            ),
        ])
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn model_switcher_category_row(category: &str, theme: &Theme, width: u16) -> Line<'static> {
    let surface = model_select_surface(theme);
    let row_width = usize::from(width);
    let padding = 3.min(row_width);
    let mut used_width = padding;
    let mut spans = Vec::new();
    if padding > 0 {
        spans.push(Span::styled(
            " ".repeat(padding),
            Style::default().bg(surface),
        ));
    }
    let label = truncate_plain_text(category, row_width.saturating_sub(used_width));
    used_width = used_width.saturating_add(label.chars().count());
    spans.push(Span::styled(
        label,
        Style::default()
            .fg(model_select_primary(theme))
            .bg(surface)
            .add_modifier(Modifier::BOLD),
    ));
    if used_width < row_width {
        spans.push(Span::styled(
            " ".repeat(row_width - used_width),
            Style::default().bg(surface),
        ));
    }
    Line::from(spans)
}

fn model_switcher_option_row_style(theme: &Theme, is_selected: bool) -> Style {
    if is_selected {
        Style::default()
            .fg(model_select_selected_fg(theme))
            .bg(model_select_primary(theme))
    } else {
        Style::default().bg(model_select_surface(theme))
    }
}

const fn model_select_surface(theme: &Theme) -> Color {
    theme.surface.panel_elevated
}

const fn model_select_primary(theme: &Theme) -> Color {
    theme.status.info
}

const fn model_select_text(theme: &Theme) -> Color {
    theme.text.primary
}

const fn model_select_muted(theme: &Theme) -> Color {
    theme.text.secondary
}

const fn model_select_selected_fg(theme: &Theme) -> Color {
    theme.text.inverse
}
