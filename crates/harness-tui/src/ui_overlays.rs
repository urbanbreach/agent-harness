use super::*;

pub(super) fn render_overlays(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    plan: &FrameLayoutPlan,
) {
    for overlay in &app.overlay_stack() {
        match overlay {
            OverlayKind::DetailsDrawer => {}
            OverlayKind::SlashCommands => {
                render_slash_commands_overlay(frame, app, theme, plan.slash_overlay)
            }
            OverlayKind::CommandPalette => {
                render_command_palette_overlay(frame, app, theme, plan.root, plan.palette_overlay)
            }
            OverlayKind::PermissionModal => {}
        }
    }
}

fn render_command_palette_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    root: Rect,
    overlay: Option<Rect>,
) {
    let Some(overlay) = overlay else {
        return;
    };

    render_overlay_dim_backdrop(frame, root);

    let title = if app.session_history_visible {
        session_history_overlay_title(app)
    } else if app.model_switcher_visible {
        model_switcher_overlay_title(app)
    } else {
        "Commands".to_string()
    };

    if app.session_history_visible {
        let Some(inner) = render_command_palette_surface(frame, theme, overlay) else {
            return;
        };
        render_session_history_overlay(frame, app, theme, inner, &title);
    } else if app.model_switcher_visible {
        if !paint_model_select_panel(frame, theme, overlay) {
            return;
        }
        render_model_switcher_overlay(frame, app, theme, overlay, &title);
    } else {
        if !paint_command_palette_panel(frame, theme, overlay) {
            return;
        }
        let Some((header, input, list)) = command_palette_dialog_layout(overlay) else {
            return;
        };
        render_command_palette_header(frame, theme, header, &title);
        render_command_palette_input(frame, app, theme, input);
        render_command_palette_list(frame, app, theme, list);
    }
}

fn render_slash_commands_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    overlay: Option<Rect>,
) {
    let Some(overlay) = overlay else {
        return;
    };
    if overlay.width <= 2 || overlay.height == 0 {
        return;
    }

    frame.render_widget(Clear, overlay);
    let inner = crate::layout::slash_command_overlay_content_area(overlay);
    render_slash_commands_list(frame, app, theme, inner);
}

fn render_slash_commands_list(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = ui_chrome::slash_command_surface(theme);
    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);

    if app.slash_filtered.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ", Style::default().bg(surface)),
                Span::styled(
                    "No matching items",
                    Style::default()
                        .fg(ui_chrome::command_palette_muted(theme))
                        .bg(surface),
                ),
                Span::styled(" ", Style::default().bg(surface)),
            ])),
            area,
        );
        return;
    }

    let visible_rows = usize::from(area.height);
    let selected = app
        .slash_selected
        .min(app.slash_filtered.len().saturating_sub(1));
    let scroll = selected.saturating_sub(visible_rows.saturating_sub(1));
    let command_column_width = app.slash_command_column_width();
    for (row, command) in app
        .slash_filtered
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_rows)
    {
        let row_y = area
            .y
            .saturating_add(u16::try_from(row - scroll).unwrap_or(u16::MAX));
        let row_area = Rect::new(area.x, row_y, area.width, 1);
        let is_selected = row == selected;
        frame.render_widget(
            Block::default().style(ui_chrome::slash_command_row_style(theme, is_selected)),
            row_area,
        );

        frame.render_widget(
            Paragraph::new(slash_command_row(
                command,
                crate::app::slash_command_description(command),
                is_selected,
                theme,
                row_area.width,
                command_column_width,
            )),
            row_area,
        );
    }
}

fn slash_command_row(
    command: &str,
    description: &str,
    is_selected: bool,
    theme: &Theme,
    width: u16,
    command_column_width: usize,
) -> Line<'static> {
    let row_width = usize::from(width);
    let row_style = ui_chrome::slash_command_row_style(theme, is_selected);
    let label_style = if is_selected {
        row_style.fg(ui_chrome::slash_command_selection_fg(theme))
    } else {
        row_style.fg(ui_chrome::command_palette_title(theme))
    };
    let description_style = if is_selected {
        row_style.fg(ui_chrome::slash_command_selection_fg(theme))
    } else {
        row_style.fg(ui_chrome::command_palette_muted(theme))
    };

    let label = slash_command_display(command);
    let side_padding = usize::from(row_width > 0);
    let available_width = row_width.saturating_sub(side_padding.saturating_mul(2));
    let label_width = label.chars().count();
    let label_column_width = command_column_width.max(label_width).min(available_width);
    let label = truncate_plain_text(&label, label_column_width);
    let label_used = label.chars().count();
    let label_padding = label_column_width.saturating_sub(label_used);
    let description_width = available_width.saturating_sub(label_column_width);
    let description = truncate_plain_text(description, description_width);
    let consumed = side_padding
        .saturating_add(label_used)
        .saturating_add(label_padding)
        .saturating_add(description.chars().count());
    let trailing = row_width.saturating_sub(consumed);

    let mut spans = Vec::new();
    if side_padding > 0 {
        spans.push(Span::styled(" ".repeat(side_padding), row_style));
    }
    if !label.is_empty() {
        spans.push(Span::styled(label, label_style));
    }
    if label_padding > 0 {
        spans.push(Span::styled(" ".repeat(label_padding), row_style));
    }
    if !description.is_empty() && description_width > 0 {
        spans.push(Span::styled(description, description_style));
    }
    if trailing > 0 {
        spans.push(Span::styled(" ".repeat(trailing), row_style));
    }

    Line::from(spans)
}

fn slash_command_display(command: &str) -> String {
    format!("/{command}")
}

fn render_command_palette_surface(frame: &mut Frame, theme: &Theme, overlay: Rect) -> Option<Rect> {
    if !paint_command_palette_panel(frame, theme, overlay) {
        return None;
    }

    let content = inset_rect(overlay, 3.min(overlay.width.saturating_sub(1)), 1);
    if content.width == 0 || content.height == 0 {
        return None;
    }

    Some(content)
}

fn paint_command_palette_panel(frame: &mut Frame, theme: &Theme, overlay: Rect) -> bool {
    if overlay.width == 0 || overlay.height == 0 {
        return false;
    }

    let surface = ui_chrome::command_palette_surface(theme);
    frame.render_widget(Clear, overlay);
    frame.render_widget(
        Block::default().style(Style::default().bg(surface)),
        overlay,
    );
    true
}

fn command_palette_dialog_layout(overlay: Rect) -> Option<(Rect, Rect, Rect)> {
    if overlay.width <= 8 || overlay.height <= 6 {
        return None;
    }

    let content_x = overlay.x.saturating_add(4);
    let content_width = overlay.width.saturating_sub(8);
    let header = Rect::new(content_x, overlay.y.saturating_add(1), content_width, 1);
    let input = Rect::new(content_x, overlay.y.saturating_add(3), content_width, 1);
    let list = Rect::new(
        overlay.x,
        overlay.y.saturating_add(5),
        overlay.width,
        overlay.height.saturating_sub(6),
    );
    Some((header, input, list))
}

fn render_command_palette_header(frame: &mut Frame, theme: &Theme, area: Rect, title: &str) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = ui_chrome::command_palette_surface(theme);
    let esc = "esc";
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(esc.chars().count() as u16),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(title.to_string()).style(
            Style::default()
                .fg(ui_chrome::command_palette_title(theme))
                .bg(surface)
                .add_modifier(Modifier::BOLD),
        ),
        columns[0],
    );
    frame.render_widget(
        Paragraph::new(esc).alignment(Alignment::Right).style(
            Style::default()
                .fg(ui_chrome::command_palette_muted(theme))
                .bg(surface),
        ),
        columns[1],
    );
}

fn render_session_history_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    area: Rect,
    title: &str,
) {
    let show_banner = app.continue_disabled_banner.is_some();
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(if show_banner { 1 } else { 0 }),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(area);
    let surface = ui_chrome::command_palette_surface(theme);

    if let Some(banner) = app.continue_disabled_banner.as_deref() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                truncate_plain_text(
                    &overlay_continue_banner_text(banner),
                    usize::from(sections[0].width),
                ),
                Style::default()
                    .fg(theme.status.warning)
                    .bg(surface)
                    .add_modifier(Modifier::BOLD),
            ))),
            sections[0],
        );
    }

    render_command_palette_header(frame, theme, sections[1], title);
    render_command_palette_input(frame, app, theme, sections[2]);
    frame.render_widget(
        Paragraph::new(session_history_scope_line(app))
            .style(Style::default().fg(theme.text.secondary).bg(surface)),
        sections[3],
    );
    render_session_history_list(frame, app, theme, sections[4]);
}

fn render_command_palette_input(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = ui_chrome::command_palette_surface(theme);
    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);

    let line = if app.palette_input.is_empty() {
        let placeholder = if app.session_history_visible {
            "Filter saved runs"
        } else if app.model_switcher_visible {
            "Filter models, providers"
        } else {
            "Search"
        };
        Line::from(vec![
            Span::styled(
                "█",
                Style::default()
                    .fg(ui_chrome::command_palette_cursor(theme))
                    .bg(surface),
            ),
            Span::styled(
                format!(" {placeholder}"),
                Style::default()
                    .fg(ui_chrome::command_palette_muted(theme))
                    .bg(surface),
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
                Style::default()
                    .fg(ui_chrome::command_palette_muted(theme))
                    .bg(surface),
            ),
            Span::styled(
                "█",
                Style::default()
                    .fg(ui_chrome::command_palette_cursor(theme))
                    .bg(surface),
            ),
            Span::styled(
                after.to_string(),
                Style::default()
                    .fg(ui_chrome::command_palette_muted(theme))
                    .bg(surface),
            ),
        ])
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn render_model_switcher_overlay(
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

    render_model_select_header(frame, theme, header, title);
    render_model_select_input(frame, app, theme, input);
    render_model_switcher_list(frame, app, theme, list);
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
                "No results found",
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
                        !app.palette_input.trim().is_empty(),
                        theme,
                        row_area.width,
                    )),
                    row_area,
                );
            }
        }
    }
}

fn model_switcher_overlay_title(app: &AppState) -> String {
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
    if app.palette_input.trim().is_empty() {
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

fn paint_model_select_panel(frame: &mut Frame, theme: &Theme, overlay: Rect) -> bool {
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
            Constraint::Length(esc.chars().count() as u16),
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

fn render_command_palette_list(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let list_area = inset_rect(area, 1.min(area.width.saturating_sub(1)), 0);
    if list_area.width == 0 || list_area.height == 0 {
        return;
    }

    frame.render_widget(
        Block::default().style(Style::default().bg(ui_chrome::command_palette_surface(theme))),
        list_area,
    );

    if app.palette_filtered.is_empty() {
        let empty_area = Rect::new(
            list_area.x.saturating_add(3),
            list_area.y,
            list_area.width.saturating_sub(3),
            1,
        );
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No results found",
                Style::default()
                    .fg(ui_chrome::command_palette_muted(theme))
                    .bg(ui_chrome::command_palette_surface(theme)),
            ))),
            empty_area,
        );
        return;
    }

    let visible_rows = usize::from(list_area.height);
    let selected = app
        .palette_selected
        .min(app.palette_filtered.len().saturating_sub(1));
    let rows = palette_overlay_rows(app);
    let selected_row = rows
        .iter()
        .position(|row| matches!(row, PaletteOverlayRow::Command { command, .. } if *command == app.palette_filtered[selected]))
        .unwrap_or(0);
    let scroll = selected_row.saturating_sub(visible_rows.saturating_sub(1));

    for (row, palette_row) in rows.iter().enumerate().skip(scroll).take(visible_rows) {
        let row_y = list_area
            .y
            .saturating_add(u16::try_from(row - scroll).unwrap_or(u16::MAX));
        let row_area = Rect::new(list_area.x, row_y, list_area.width, 1);
        match palette_row {
            PaletteOverlayRow::Spacer => {
                frame.render_widget(
                    Block::default()
                        .style(Style::default().bg(ui_chrome::command_palette_surface(theme))),
                    row_area,
                );
            }
            PaletteOverlayRow::Section(section) => {
                frame.render_widget(
                    Paragraph::new(command_palette_section_row(
                        section.label(),
                        theme,
                        row_area.width,
                    )),
                    row_area,
                );
            }
            PaletteOverlayRow::Command {
                command,
                selected_index,
            } => {
                let is_selected = *selected_index == selected;
                if is_selected {
                    frame.render_widget(
                        Block::default().style(ui_chrome::overlay_focus_row_style(theme)),
                        row_area,
                    );
                }

                frame.render_widget(
                    Paragraph::new(command_palette_row(
                        Action::palette_command_label(command),
                        palette_command_description(command),
                        Action::palette_command_shortcut(command),
                        is_selected,
                        theme,
                        row_area.width,
                    )),
                    row_area,
                );
            }
        }
    }
}

enum PaletteOverlayRow<'a> {
    Spacer,
    Section(crate::keybindings::PaletteCommandSection),
    Command {
        command: &'a str,
        selected_index: usize,
    },
}

fn palette_overlay_rows(app: &AppState) -> Vec<PaletteOverlayRow<'_>> {
    let mut rows = Vec::new();
    let mut last_section = None;

    for (selected_index, command) in app.palette_filtered.iter().enumerate() {
        let section = Action::palette_command_section(command.as_str());
        if section != last_section {
            if let Some(section) = section {
                if last_section.is_some() {
                    rows.push(PaletteOverlayRow::Spacer);
                }
                rows.push(PaletteOverlayRow::Section(section));
            }
            last_section = section;
        }
        rows.push(PaletteOverlayRow::Command {
            command,
            selected_index,
        });
    }

    rows
}

fn command_palette_row(
    label: &str,
    description: &str,
    shortcut: &str,
    is_selected: bool,
    theme: &Theme,
    width: u16,
) -> Line<'static> {
    let row_width = usize::from(width);
    let surface = ui_chrome::command_palette_surface(theme);
    let row_style = if is_selected {
        ui_chrome::overlay_focus_row_style(theme)
    } else {
        Style::default().bg(surface)
    };
    let label_style = if is_selected {
        row_style.add_modifier(Modifier::BOLD)
    } else {
        row_style.fg(ui_chrome::command_palette_title(theme))
    };
    let description_style = if is_selected {
        row_style
    } else {
        row_style.fg(ui_chrome::command_palette_muted(theme))
    };
    let shortcut_style = if is_selected {
        row_style.add_modifier(Modifier::BOLD)
    } else {
        row_style.fg(ui_chrome::command_palette_muted(theme))
    };

    let content_width = row_width.saturating_sub(3);
    let reserved_shortcut = if shortcut.is_empty() {
        0
    } else {
        shortcut.chars().count().saturating_add(2)
    };
    let body_width = content_width.saturating_sub(reserved_shortcut);
    let prefix = "      ";
    let mut spans = vec![Span::styled(prefix.to_string(), row_style)];
    let mut used_width = prefix.chars().count();

    let label = truncate_plain_text(label, 61usize.min(body_width.saturating_sub(used_width)));
    used_width = used_width.saturating_add(label.chars().count());
    spans.push(Span::styled(label, label_style));

    let gap_width = 1;
    let available_description = body_width.saturating_sub(used_width.saturating_add(gap_width));
    let description = truncate_plain_text(description, available_description);
    if !description.is_empty() {
        spans.push(Span::styled(" ", row_style));
        used_width = used_width.saturating_add(gap_width);
        used_width = used_width.saturating_add(description.chars().count());
        spans.push(Span::styled(description, description_style));
    }

    if used_width < body_width {
        spans.push(Span::styled(" ".repeat(body_width - used_width), row_style));
    }

    if !shortcut.is_empty() {
        spans.push(Span::styled("  ", row_style));
        spans.push(Span::styled(shortcut.to_string(), shortcut_style));
    }

    if content_width < row_width {
        spans.push(Span::styled(
            " ".repeat(row_width - content_width),
            row_style,
        ));
    }

    Line::from(spans)
}

fn command_palette_section_row(label: &str, theme: &Theme, width: u16) -> Line<'static> {
    let row_width = usize::from(width);
    let surface = ui_chrome::command_palette_surface(theme);
    let prefix = "   ";
    let mut spans = vec![Span::styled(prefix, Style::default().bg(surface))];
    let label = truncate_plain_text(label, row_width.saturating_sub(prefix.chars().count()));
    let label_width = label.chars().count();
    spans.push(Span::styled(
        label,
        Style::default()
            .fg(ui_chrome::command_palette_section())
            .bg(surface)
            .add_modifier(Modifier::BOLD),
    ));
    let used_width = prefix.chars().count().saturating_add(label_width);
    if used_width < row_width {
        spans.push(Span::styled(
            " ".repeat(row_width - used_width),
            Style::default().bg(surface),
        ));
    }
    Line::from(spans)
}

fn render_session_history_list(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    if app.session_history_filtered.is_empty() {
        let empty = if app.session_history_entries.is_empty() {
            "No saved runs yet — launch one and it will appear here."
        } else {
            "No saved runs match this filter."
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                empty,
                Style::default().fg(theme.text.secondary),
            ))),
            area,
        );
        return;
    }

    let row_height = 1usize;
    let visible_rows = (usize::from(area.height) / row_height).max(1);
    let selected = app
        .session_history_selected
        .min(app.session_history_filtered.len().saturating_sub(1));
    let scroll = selected.saturating_sub(visible_rows.saturating_sub(1));

    for (visible_index, entry_index) in app
        .session_history_filtered
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_rows)
    {
        let entry = &app.session_history_entries[*entry_index];
        let row_offset = (visible_index - scroll) * row_height;
        let row_y = area
            .y
            .saturating_add(u16::try_from(row_offset).unwrap_or(u16::MAX));
        if row_y >= area.y.saturating_add(area.height) {
            break;
        }

        let remaining_height = area
            .height
            .saturating_sub(u16::try_from(row_offset).unwrap_or(u16::MAX));
        let row_area = Rect::new(area.x, row_y, area.width, remaining_height.min(1));
        let is_selected = visible_index == selected;
        if is_selected {
            frame.render_widget(
                Block::default().style(ui_chrome::overlay_focus_row_style(theme)),
                row_area,
            );
        }

        frame.render_widget(
            Paragraph::new(session_history_row(
                entry,
                app,
                is_selected,
                theme,
                row_area.width,
            )),
            row_area,
        );
    }
}

fn session_history_overlay_title(app: &AppState) -> String {
    let total = app.session_history_filtered.len();
    let matches_label = format!("{total} match{}", if total == 1 { "" } else { "es" });
    match app.startup_launcher_action {
        StartupLauncherAction::ReplaySession => format!("Replay session · {matches_label}"),
        StartupLauncherAction::ContinueSession => {
            let blocked = app
                .session_history_filtered
                .iter()
                .filter(|entry_index| {
                    !app.session_history_entries[**entry_index]
                        .catalog
                        .is_resumable
                })
                .count();
            if blocked > 0 {
                format!("Continue session · {matches_label} · {blocked} blocked")
            } else {
                format!("Continue session · {matches_label}")
            }
        }
        StartupLauncherAction::NewSession => format!("Session history · {matches_label}"),
    }
}

fn session_history_scope_line(app: &AppState) -> String {
    match app.startup_launcher_action {
        StartupLauncherAction::ContinueSession => {
            let ready = app
                .session_history_filtered
                .iter()
                .filter(|entry_index| {
                    app.session_history_entries[**entry_index]
                        .catalog
                        .is_resumable
                })
                .count();
            let blocked = app.session_history_filtered.len().saturating_sub(ready);
            if app.session_history_filtered.is_empty() {
                "Interactive histories only · blocked rows stay visible when they match".to_string()
            } else if blocked == 0 {
                format!(
                    "Interactive histories · {ready} ready · filter by run/profile/model/lineage"
                )
            } else {
                format!(
                    "Interactive histories · {ready} ready · {blocked} blocked · filter by run/profile/model/lineage"
                )
            }
        }
        StartupLauncherAction::ReplaySession => {
            let prompt_only = app
                .session_history_filtered
                .iter()
                .filter(|entry_index| {
                    matches!(
                        app.session_history_entries[**entry_index]
                            .catalog
                            .mode_source,
                        harness_core::proj::SessionModeSource::Prompt
                    )
                })
                .count();
            if prompt_only > 0 {
                format!(
                    "Read-only replays · {} matching · {prompt_only} prompt-only still visible",
                    app.session_history_filtered.len()
                )
            } else {
                format!(
                    "Read-only replays · {} matching · interactive and prompt runs stay available",
                    app.session_history_filtered.len()
                )
            }
        }
        StartupLauncherAction::NewSession => {
            "Browse saved runs without losing the draft in the launcher".to_string()
        }
    }
}

fn session_history_row(
    entry: &crate::app::SessionHistoryEntry,
    app: &AppState,
    is_selected: bool,
    theme: &Theme,
    width: u16,
) -> Line<'static> {
    let row_style = if is_selected {
        ui_chrome::overlay_focus_row_style(theme)
    } else {
        Style::default()
    };
    let title_style = if is_selected {
        row_style.add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme.text.primary)
            .add_modifier(Modifier::BOLD)
    };
    let meta_style = if is_selected {
        row_style
    } else {
        Style::default().fg(theme.text.secondary)
    };
    let action_style = if is_selected {
        row_style.add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(session_history_action_color(app, entry, theme))
            .add_modifier(Modifier::BOLD)
    };
    let status_style = if is_selected {
        row_style
    } else {
        Style::default().fg(session_history_status_color(entry, theme))
    };
    let capability_style = if is_selected {
        row_style
    } else {
        Style::default().fg(match app.startup_launcher_action {
            StartupLauncherAction::ContinueSession | StartupLauncherAction::NewSession => {
                if entry.catalog.is_resumable {
                    theme.status.success
                } else {
                    theme.status.warning
                }
            }
            StartupLauncherAction::ReplaySession => theme.status.info,
        })
    };

    let capability = overlay_session_history_capability_label(entry, app.startup_launcher_action);
    let artifact_label = session_history_artifact_label(entry);
    let lineage_label = session_history_lineage_label(entry);
    let source = format!(
        "{}/{}",
        session_history_profile_label(entry),
        session_history_provider_model_label(entry)
    );

    let row_width = usize::from(width);
    let prefix = session_history_action_prefix(app, entry);
    let reserved_capability_width = usize::from(row_width > 32) * 18;
    let title_budget = row_width
        .saturating_sub(
            prefix
                .chars()
                .count()
                .saturating_add(reserved_capability_width),
        )
        .max(12)
        .min(row_width.saturating_sub(prefix.chars().count()));
    let title = truncate_plain_text(session_history_run_name(entry), title_budget);

    let mut spans = vec![
        Span::styled(prefix.clone(), action_style),
        Span::styled(title.clone(), title_style),
    ];
    let mut used_width = prefix.chars().count().saturating_add(title.chars().count());

    append_session_history_segment(
        &mut spans,
        &mut used_width,
        row_width,
        &capability,
        meta_style,
        capability_style,
        8,
    );

    if row_width >= 58 {
        append_session_history_segment(
            &mut spans,
            &mut used_width,
            row_width,
            &artifact_label,
            meta_style,
            meta_style,
            8,
        );
    }

    if row_width >= 76 {
        append_session_history_segment(
            &mut spans,
            &mut used_width,
            row_width,
            &lineage_label,
            meta_style,
            meta_style,
            10,
        );
    }

    if row_width >= 92 {
        append_session_history_segment(
            &mut spans,
            &mut used_width,
            row_width,
            session_history_status_label(entry),
            meta_style,
            status_style,
            6,
        );
    }

    if row_width >= 112 {
        append_session_history_segment(
            &mut spans,
            &mut used_width,
            row_width,
            &source,
            meta_style,
            meta_style,
            10,
        );
    }

    if is_selected && used_width < row_width {
        spans.push(Span::styled(" ".repeat(row_width - used_width), row_style));
    }

    Line::from(spans)
}

fn append_session_history_segment(
    spans: &mut Vec<Span<'static>>,
    used_width: &mut usize,
    row_width: usize,
    text: &str,
    separator_style: Style,
    text_style: Style,
    min_text_width: usize,
) {
    const SEPARATOR: &str = " · ";

    let separator_width = SEPARATOR.chars().count();
    let remaining = row_width.saturating_sub(*used_width);
    if remaining <= separator_width.saturating_add(min_text_width) {
        return;
    }

    let text = truncate_plain_text(text, remaining.saturating_sub(separator_width));
    if text.is_empty() {
        return;
    }

    spans.push(Span::styled(SEPARATOR.to_string(), separator_style));
    spans.push(Span::styled(text.clone(), text_style));
    *used_width = used_width
        .saturating_add(separator_width)
        .saturating_add(text.chars().count());
}

fn overlay_continue_banner_text(banner: &str) -> String {
    banner
        .strip_prefix("continue unavailable: ")
        .map(|reason| format!("blocked · {reason}"))
        .unwrap_or_else(|| banner.to_string())
}

fn overlay_session_history_capability_label(
    entry: &crate::app::SessionHistoryEntry,
    action: StartupLauncherAction,
) -> String {
    match action {
        StartupLauncherAction::ContinueSession | StartupLauncherAction::NewSession => {
            if entry.catalog.is_resumable {
                "continue ready".to_string()
            } else {
                entry
                    .catalog
                    .resume_disabled_reason
                    .as_deref()
                    .map(|reason| format!("blocked · {reason}"))
                    .unwrap_or_else(|| "blocked".to_string())
            }
        }
        StartupLauncherAction::ReplaySession => session_history_capability_label(entry, action),
    }
}

fn session_history_capability_label(
    entry: &crate::app::SessionHistoryEntry,
    action: StartupLauncherAction,
) -> String {
    match action {
        StartupLauncherAction::ContinueSession | StartupLauncherAction::NewSession => {
            session_history_resumability_label(entry)
        }
        StartupLauncherAction::ReplaySession => match entry.catalog.mode_source {
            harness_core::proj::SessionModeSource::Prompt => "prompt-only replay ready".to_string(),
            harness_core::proj::SessionModeSource::InteractiveLive
            | harness_core::proj::SessionModeSource::InteractiveMock => {
                if entry.catalog.is_resumable {
                    "replay ready · continue ready".to_string()
                } else {
                    entry
                        .catalog
                        .resume_disabled_reason
                        .as_deref()
                        .map(|reason| format!("replay ready · blocked: {reason}"))
                        .unwrap_or_else(|| "replay ready".to_string())
                }
            }
            harness_core::proj::SessionModeSource::ScenarioFixture => {
                "fixture replay ready".to_string()
            }
            harness_core::proj::SessionModeSource::ReplayOnly => {
                "replay artifact ready".to_string()
            }
            harness_core::proj::SessionModeSource::Unknown => "saved replay ready".to_string(),
        },
    }
}

fn session_history_action_prefix(
    app: &AppState,
    entry: &crate::app::SessionHistoryEntry,
) -> String {
    match app.startup_launcher_action {
        StartupLauncherAction::ReplaySession => "↺ replay ".to_string(),
        StartupLauncherAction::ContinueSession | StartupLauncherAction::NewSession => {
            if entry.catalog.is_resumable {
                "▶ continue ".to_string()
            } else {
                "! blocked ".to_string()
            }
        }
    }
}

fn session_history_action_color(
    app: &AppState,
    entry: &crate::app::SessionHistoryEntry,
    theme: &Theme,
) -> Color {
    match app.startup_launcher_action {
        StartupLauncherAction::ReplaySession => theme.status.info,
        StartupLauncherAction::ContinueSession | StartupLauncherAction::NewSession => {
            if entry.catalog.is_resumable {
                theme.status.success
            } else {
                theme.status.warning
            }
        }
    }
}

fn session_history_status_color(entry: &crate::app::SessionHistoryEntry, theme: &Theme) -> Color {
    match entry.catalog.status {
        Some(harness_core::proj::RunStatus::Running) => theme.status.info,
        Some(harness_core::proj::RunStatus::Finished) => theme.status.success,
        Some(harness_core::proj::RunStatus::Failed) => theme.status.error,
        None => theme.text.secondary,
    }
}

fn palette_command_description(command: &str) -> &'static str {
    Action::palette_commands()
        .iter()
        .find_map(|(candidate, description)| (*candidate == command).then_some(*description))
        .unwrap_or("")
}

pub(super) fn permission_modal_metadata_line(
    permission: &crate::app::ActivePermissionView,
) -> String {
    let subject = permission
        .tool_label
        .as_deref()
        .map(|tool| format!("tool {tool}"))
        .or_else(|| {
            permission
                .tool_call_id
                .as_deref()
                .map(|tool_call_id| format!("call {tool_call_id}"))
        })
        .unwrap_or_else(|| format!("perm {}", permission.permission_id));

    format!(
        "{} · dig {} · timeout {}s",
        subject,
        abbreviated_digest(&permission.request_digest),
        permission.timeout_ms / 1_000,
    )
}

fn abbreviated_digest(digest: &str) -> String {
    let mut short = digest.chars().take(6).collect::<String>();
    if digest.chars().count() > 6 {
        short.push('…');
    }
    short
}

pub(super) fn permission_modal_icon(permission: &crate::app::ActivePermissionView) -> &'static str {
    let kind = permission.kind.as_str();
    if kind.eq_ignore_ascii_case("question")
        || kind.eq_ignore_ascii_case("ask")
        || kind.eq_ignore_ascii_case("ask_user")
    {
        return "?";
    }
    if kind.eq_ignore_ascii_case("edit")
        || kind.eq_ignore_ascii_case("edit_fs")
        || kind.eq_ignore_ascii_case("lsp")
    {
        return "→";
    }
    if kind.eq_ignore_ascii_case("shell") || kind.eq_ignore_ascii_case("bash") {
        return "#";
    }
    if kind.eq_ignore_ascii_case("task") {
        return "#";
    }
    if kind.eq_ignore_ascii_case("webfetch") {
        return "%";
    }
    if kind.eq_ignore_ascii_case("websearch") {
        return "◈";
    }
    if kind.eq_ignore_ascii_case("codesearch") {
        return "◇";
    }
    "⚙"
}

pub(super) fn permission_modal_subject_line(
    permission: &crate::app::ActivePermissionView,
) -> String {
    if permission.kind.eq_ignore_ascii_case("question")
        || permission.kind.eq_ignore_ascii_case("ask")
        || permission.kind.eq_ignore_ascii_case("ask_user")
    {
        return "Answer operator question".to_string();
    }

    let summary = permission.summary.trim();
    if !summary.is_empty() && !summary.starts_with('{') && !summary.starts_with('[') {
        return summary.to_string();
    }

    permission
        .tool_label
        .as_deref()
        .map(|tool| format!("Review {tool}"))
        .unwrap_or_else(|| format!("Review {}", permission.kind.replace('_', " ")))
}

pub(super) fn permission_modal_title(
    permission: &crate::app::ActivePermissionView,
) -> &'static str {
    if permission.kind.eq_ignore_ascii_case("question")
        || permission.kind.eq_ignore_ascii_case("ask")
        || permission.kind.eq_ignore_ascii_case("ask_user")
    {
        "Question required"
    } else {
        "Permission required"
    }
}

pub(super) fn permission_modal_guidance(
    permission: &crate::app::ActivePermissionView,
    submission_pending: bool,
) -> &'static str {
    if submission_pending {
        "Decision recorded. Wait for confirmation before sending another turn."
    } else if permission.kind.eq_ignore_ascii_case("question")
        || permission.kind.eq_ignore_ascii_case("ask")
        || permission.kind.eq_ignore_ascii_case("ask_user")
    {
        "Safest next step: deny. Answer only after review."
    } else {
        "Safest next step: deny. Allow once only after review."
    }
}

pub(super) fn permission_modal_summary_line(
    permission: &crate::app::ActivePermissionView,
    submission_pending: bool,
) -> String {
    if submission_pending {
        return "Decision submitted — awaiting confirmation.".to_string();
    }

    permission
        .tool_label
        .as_deref()
        .map(|tool| format!("Tool {tool} is paused for review."))
        .unwrap_or_else(|| {
            if permission.summary.chars().count() > 48 {
                format!(
                    "{} request is paused for review.",
                    permission.kind.replace('_', " ")
                )
            } else {
                permission.summary.clone()
            }
        })
}

pub(super) fn permission_modal_draft_line(prompt_buffer: &str) -> String {
    let draft = prompt_buffer.trim();
    if draft.is_empty() {
        String::new()
    } else {
        format!("Draft preserved · {draft}")
    }
}

fn render_overlay_dim_backdrop(frame: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let buffer = frame.buffer_mut();
    let max_x = area.x.saturating_add(area.width);
    let max_y = area.y.saturating_add(area.height);
    for y in area.y..max_y {
        for x in area.x..max_x {
            let cell = &mut buffer[(x, y)];
            cell.set_fg(dim_overlay_color(cell.fg));
            cell.set_bg(dim_overlay_color(cell.bg));
        }
    }
}

fn dim_overlay_color(color: Color) -> Color {
    let Some((red, green, blue)) = color_rgb(color) else {
        return color;
    };
    Color::Rgb(
        scrim_channel(red),
        scrim_channel(green),
        scrim_channel(blue),
    )
}

fn scrim_channel(channel: u8) -> u8 {
    let channel = u16::from(channel);
    u8::try_from(channel.saturating_mul(105) / 255).unwrap_or_default()
}

fn color_rgb(color: Color) -> Option<(u8, u8, u8)> {
    match color {
        Color::Black => Some((0, 0, 0)),
        Color::Red => Some((128, 0, 0)),
        Color::Green => Some((0, 128, 0)),
        Color::Yellow => Some((128, 128, 0)),
        Color::Blue => Some((0, 0, 128)),
        Color::Magenta => Some((128, 0, 128)),
        Color::Cyan => Some((0, 128, 128)),
        Color::Gray => Some((192, 192, 192)),
        Color::DarkGray => Some((128, 128, 128)),
        Color::LightRed => Some((255, 0, 0)),
        Color::LightGreen => Some((0, 255, 0)),
        Color::LightYellow => Some((255, 255, 0)),
        Color::LightBlue => Some((0, 0, 255)),
        Color::LightMagenta => Some((255, 0, 255)),
        Color::LightCyan => Some((0, 255, 255)),
        Color::White => Some((255, 255, 255)),
        Color::Rgb(red, green, blue) => Some((red, green, blue)),
        Color::Indexed(index) => Some((index, index, index)),
        Color::Reset => None,
    }
}

pub(super) fn permission_modal_actions_text(
    app: &AppState,
    theme: &Theme,
    surface: Color,
    submission_pending: bool,
    is_question: bool,
) -> Text<'static> {
    let metadata_style = Style::default().fg(theme.text.secondary).bg(surface);
    let primary_style = Style::default().fg(theme.text.primary).bg(surface);

    if submission_pending {
        return Text::from(vec![
            Line::from(vec![
                status_badge("decision sent", theme.status.info, theme),
                Span::styled("  waiting for confirmation", metadata_style),
            ]),
            Line::from(vec![Span::styled(
                "No new action required until confirmation returns.",
                metadata_style,
            )]),
        ]);
    }

    let deny_label = app.keymap.get_binding_label(Action::DenyPermission, "deny");
    let reject_label = app.keymap.get_binding_label(Action::DismissModal, "reject");
    let allow_label = app
        .keymap
        .get_binding_label(Action::AllowPermission, "allow once");

    Text::from(vec![
        Line::from(vec![
            status_badge(deny_label, theme.status.error, theme),
            Span::styled("  default deny · stays fail-closed", metadata_style),
        ]),
        Line::from(vec![
            Span::styled(
                if is_question {
                    format!("{reject_label} rejects the question")
                } else {
                    format!("{reject_label} rejects")
                },
                metadata_style,
            ),
            Span::styled("  ·  ", metadata_style),
            Span::styled(
                if is_question {
                    format!("{allow_label} sends answers")
                } else {
                    allow_label
                },
                primary_style,
            ),
        ]),
    ])
}

pub(super) fn question_permission_actions_text(
    app: &AppState,
    permission: &crate::app::ActivePermissionView,
    prompts: &[crate::app::QuestionPromptView],
    theme: &Theme,
    surface: Color,
) -> Text<'static> {
    let primary_style = Style::default().fg(theme.text.primary).bg(surface);
    let metadata_style = Style::default().fg(theme.text.secondary).bg(surface);
    let single = prompts.len() == 1 && !prompts[0].multiple;
    let confirm = !single && app.question_prompt_tab(&permission.permission_id) >= prompts.len();
    let submit_label = if confirm {
        "submit"
    } else if prompts
        .get(app.question_prompt_tab(&permission.permission_id))
        .is_some_and(|prompt| prompt.multiple)
    {
        "toggle"
    } else if single {
        "submit"
    } else {
        "confirm"
    };

    let mut spans = Vec::new();
    if !single {
        spans.push(Span::styled("⇆", primary_style));
        spans.push(Span::styled(" tab  ", metadata_style));
    }
    if !confirm {
        spans.push(Span::styled("↑↓", primary_style));
        spans.push(Span::styled(" select  ", metadata_style));
    }
    spans.push(Span::styled("enter", primary_style));
    spans.push(Span::styled(format!(" {submit_label}  "), metadata_style));
    spans.push(Span::styled("esc", primary_style));
    spans.push(Span::styled(" dismiss", metadata_style));
    Text::from(Line::from(spans))
}

pub(super) fn question_permission_body_text(
    app: &AppState,
    permission: &crate::app::ActivePermissionView,
    prompts: &[crate::app::QuestionPromptView],
    theme: &Theme,
    surface: Color,
) -> Text<'static> {
    if prompts.is_empty() {
        return Text::default();
    }

    let primary_style = Style::default().fg(theme.text.primary).bg(surface);
    let muted_style = Style::default().fg(theme.text.secondary).bg(surface);
    let accent_style = Style::default()
        .fg(theme.text.inverse)
        .bg(ui_chrome::question_prompt_accent(theme));
    let active_surface = theme.surface.panel_elevated;
    let active_number_style = Style::default()
        .fg(question_prompt_tint(
            theme.text.secondary,
            ui_chrome::question_prompt_secondary(theme),
            0.6,
        ))
        .bg(active_surface);
    let active_label_style = Style::default()
        .fg(ui_chrome::question_prompt_secondary(theme))
        .bg(active_surface);
    let success_style = Style::default().fg(theme.status.success).bg(surface);
    let error_style = Style::default().fg(theme.status.error).bg(surface);
    let single = prompts.len() == 1 && !prompts[0].multiple;
    let tab = app
        .question_prompt_tab(&permission.permission_id)
        .min(prompts.len());
    let confirm = !single && tab >= prompts.len();
    let answers = app.question_prompt_answers(&permission.permission_id);
    let mut lines = Vec::new();

    if !single {
        let mut tabs = Vec::new();
        for (index, prompt) in prompts.iter().enumerate() {
            if index > 0 {
                tabs.push(Span::styled(" ", Style::default().bg(surface)));
            }
            let answered = answers.get(index).is_some_and(|value| !value.is_empty());
            tabs.push(Span::styled(
                format!(" {} ", prompt.header),
                if index == tab {
                    accent_style
                } else if answered {
                    primary_style
                } else {
                    muted_style
                },
            ));
        }
        tabs.push(Span::styled(" ", Style::default().bg(surface)));
        tabs.push(Span::styled(
            " Confirm ",
            if confirm { accent_style } else { muted_style },
        ));
        lines.push(Line::from(tabs));
        lines.push(Line::default());
    }

    if confirm {
        lines.push(Line::from(vec![Span::styled("Review", primary_style)]));
        for (index, prompt) in prompts.iter().enumerate() {
            let value = answers
                .get(index)
                .map(|value| value.join(", "))
                .unwrap_or_default();
            lines.push(Line::from(vec![
                Span::styled(format!("{}: ", prompt.header), muted_style),
                Span::styled(
                    if value.is_empty() {
                        "(not answered)".to_string()
                    } else {
                        value
                    },
                    if answers.get(index).is_some_and(|value| !value.is_empty()) {
                        primary_style
                    } else {
                        error_style
                    },
                ),
            ]));
        }
        return Text::from(lines);
    }

    let prompt = &prompts[tab.min(prompts.len().saturating_sub(1))];
    let selected = app.question_prompt_selection(&permission.permission_id);
    let current_answers = answers.get(tab).cloned().unwrap_or_default();

    lines.push(Line::from(vec![Span::styled(
        if prompt.multiple {
            format!("{} (select all that apply)", prompt.question)
        } else {
            prompt.question.clone()
        },
        primary_style,
    )]));
    lines.push(Line::default());

    for (index, option) in prompt.options.iter().enumerate() {
        let picked = current_answers.iter().any(|value| value == &option.label);
        let active = index == selected;
        let mut row = vec![Span::styled(
            format!("{}.", index + 1),
            if active {
                active_number_style
            } else {
                muted_style
            },
        )];
        row.push(Span::styled(
            " ",
            Style::default().bg(if active { active_surface } else { surface }),
        ));
        row.push(Span::styled(
            if prompt.multiple {
                format!("[{}] {}", if picked { '✓' } else { ' ' }, option.label)
            } else {
                option.label.clone()
            },
            if active {
                active_label_style
            } else if prompt.multiple && picked {
                success_style
            } else {
                primary_style
            },
        ));
        if !prompt.multiple {
            row.push(Span::styled(if picked { "✓" } else { "" }, success_style));
        }
        lines.push(Line::from(row));
        lines.push(Line::from(vec![Span::styled(
            format!("   {}", option.description),
            muted_style,
        )]));
    }

    if prompt.custom {
        let custom_value = app
            .question_prompt_custom(&permission.permission_id, tab)
            .unwrap_or_default();
        let picked =
            !custom_value.is_empty() && current_answers.iter().any(|value| value == custom_value);
        let active = selected == prompt.options.len();
        let mut row = vec![Span::styled(
            format!("{}.", prompt.options.len() + 1),
            if active {
                active_number_style
            } else {
                muted_style
            },
        )];
        row.push(Span::styled(
            " ",
            Style::default().bg(if active { active_surface } else { surface }),
        ));
        row.push(Span::styled(
            if prompt.multiple {
                format!("[{}] Type your own answer", if picked { '✓' } else { ' ' })
            } else {
                "Type your own answer".to_string()
            },
            if active {
                active_label_style
            } else if prompt.multiple && picked {
                success_style
            } else {
                primary_style
            },
        ));
        if !prompt.multiple {
            row.push(Span::styled(if picked { "✓" } else { "" }, success_style));
        }
        lines.push(Line::from(row));

        let editing = app.question_prompt_editing(&permission.permission_id) && active;
        if editing {
            let preview = app.question_answer_preview(&permission.permission_id);
            let (text, style) = if preview == "█" {
                ("Type your own answer".to_string(), muted_style)
            } else {
                (preview, primary_style)
            };
            lines.push(Line::from(vec![Span::styled(format!("   {text}"), style)]));
        } else if !custom_value.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                format!("   {custom_value}"),
                muted_style,
            )]));
        }
    }

    if let Some(error) = app.question_answer_error(&permission.permission_id) {
        lines.push(Line::default());
        lines.push(Line::from(vec![Span::styled(
            error.to_string(),
            error_style,
        )]));
    }

    Text::from(lines)
}

fn question_prompt_tint(base: Color, overlay: Color, alpha: f32) -> Color {
    match (base, overlay) {
        (
            Color::Rgb(base_red, base_green, base_blue),
            Color::Rgb(overlay_red, overlay_green, overlay_blue),
        ) => {
            let blend = |base: u8, overlay: u8| -> u8 {
                let value = (f32::from(base) * (1.0 - alpha)) + (f32::from(overlay) * alpha);
                value.round().clamp(0.0, 255.0) as u8
            };
            Color::Rgb(
                blend(base_red, overlay_red),
                blend(base_green, overlay_green),
                blend(base_blue, overlay_blue),
            )
        }
        _ => overlay,
    }
}
