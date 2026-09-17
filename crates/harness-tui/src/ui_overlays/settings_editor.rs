use super::*;
use harness_core::config::SettingEditorKind;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(super) fn choices(app: &AppState) -> Option<(&'static [&'static str], usize)> {
    let edit = app.settings_interaction.edit.as_ref()?;
    let SettingEditorKind::Choice(choices) = edit.kind else {
        return None;
    };
    let value = edit.editor.text();
    let selected = choices
        .iter()
        .position(|choice| *choice == value)
        .unwrap_or(0);
    Some((choices, selected))
}

pub(super) fn render_settings_editor_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    root: Rect,
) {
    let Some(model) = modal_surface_model(app, root) else {
        return;
    };
    let popup = model.popup;
    if !paint_modal_panel(frame, app, theme, popup, model.key, "Settings") {
        return;
    }
    let inner = inset_rect(popup, 1, 1);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let surface = ui_chrome::command_palette_surface(theme);
    let muted = Style::default().fg(theme.text.secondary).bg(surface);
    let editing = app.settings_interaction.edit.is_some();
    let footer = match (editing, inner.width) {
        (true, 0..24) => "Enter · Esc",
        (true, _) => "Enter save · Esc cancel",
        (false, 0..24) => "↑↓ · / · Esc",
        (false, 24..58) => "↑↓ · / search · Tab · Esc",
        (false, _) => "↑↓ navigate · / search · Tab switch · Enter edit · Esc close",
    };
    let mut chrome = modal_chrome::settings_chrome(app.settings_editor_tab());
    chrome.footer = footer;
    if editing {
        chrome.breadcrumb = None;
        chrome.tabs = None;
    }
    modal_chrome::render_body(frame, theme, popup, chrome);

    if let Some(edit) = &app.settings_interaction.edit {
        frame.render_widget(
            Paragraph::new(truncate_plain_text(
                &crate::app::settings_label(&edit.id),
                usize::from(inner.width.saturating_sub(2)),
            ))
            .style(
                Style::default()
                    .fg(theme.text.primary)
                    .add_modifier(Modifier::BOLD),
            ),
            Rect::new(inner.x + 1, inner.y, inner.width.saturating_sub(2), 1),
        );
        if let Some((choices, selected)) = choices(app) {
            if inner.height > 2 {
                frame.render_widget(
                    Paragraph::new("Choose a value; Enter saves").style(muted),
                    Rect::new(inner.x + 1, inner.y + 1, inner.width.saturating_sub(2), 1),
                );
            }
            for region in &model.regions {
                if let ModalTarget::Row(index) = region.target {
                    let row = modal_list_row(
                        theme,
                        ModalListRowSpec {
                            area: region.area,
                            state: ModalListRowState {
                                selected: index == selected,
                                hovered: app.modal_target_hovered(model.key, region.target),
                                dimmed: false,
                            },
                            max_scroll: model.max_scroll,
                        },
                    );
                    frame.render_widget(
                        Paragraph::new(format!(
                            "({}) {}",
                            if index == selected {
                                theme.live_shell.transcript_glyphs.choice_selected
                            } else {
                                theme.live_shell.transcript_glyphs.choice_unselected
                            },
                            choices[index]
                        ))
                        .style(row.style),
                        row.layout.content,
                    );
                }
            }
        } else {
            render_value(frame, app, theme, inner);
        }
        return;
    }

    if popup.height > 5 {
        let input = Rect::new(popup.x + 2, popup.y + 3, popup.width.saturating_sub(4), 1);
        let query = &app.settings_interaction.query;
        if query.is_empty() && !app.settings_interaction.filtering {
            frame.render_widget(Paragraph::new("/ to search").style(muted), input);
        } else {
            render_input(
                frame,
                theme,
                input,
                query,
                query.len(),
                app.settings_interaction.filtering,
                false,
            );
        }
    }
    let rows = app.settings_editor_rows();
    let list_area = Rect::new(
        inner.x,
        popup.y + 4,
        inner.width,
        popup.height.saturating_sub(6),
    );
    if rows.is_empty() && list_area.height > 0 {
        frame.render_widget(Paragraph::new("No matches").style(muted), list_area);
    }
    for region in &model.regions {
        let ModalTarget::Row(index) = region.target else {
            continue;
        };
        let entry = &rows[index];
        let row = modal_list_row(
            theme,
            ModalListRowSpec {
                area: region.area,
                state: ModalListRowState {
                    selected: entry.selected,
                    hovered: app.modal_target_hovered(model.key, region.target),
                    dimmed: !entry.editable,
                },
                max_scroll: model.max_scroll,
            },
        );
        let width = usize::from(row.layout.content.width);
        let meta = if width < 32 {
            ""
        } else if entry.sensitivity == "secret" {
            "secret"
        } else if entry.editable {
            "Enter edit"
        } else {
            "read only"
        };
        let label = crate::app::settings_label(&entry.setting_id);
        let label = match entry.effective_value.as_ref() {
            Some(value) => format!("{label} = {}", crate::ui::safe_product_text(value)),
            None => label,
        };
        let label = truncate_plain_text(&label, width.saturating_sub(meta.width() + 2));
        let pad = width.saturating_sub(label.width() + meta.width() + 1);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!(" {label}"), row.style),
                Span::styled(" ".repeat(pad), row.style),
                Span::styled(meta, row.style.fg(theme.text.tertiary)),
            ])),
            row.layout.content,
        );
    }
    render_modal_list_scrollbar(
        frame,
        theme,
        ModalListScrollbarSpec {
            area: list_area,
            offset: model.visual_offset,
            max_scroll: model.max_scroll,
        },
    );
}

fn render_value(frame: &mut Frame, app: &AppState, theme: &Theme, inner: Rect) {
    let Some(edit) = &app.settings_interaction.edit else {
        return;
    };
    if inner.height < 4 || inner.width < 3 {
        return;
    }
    let input = Rect::new(inner.x + 1, inner.y + 2, inner.width.saturating_sub(2), 1);
    let text = edit.editor.text();
    let cursor = text
        .graphemes(true)
        .take(edit.editor.cursor().insertion_index())
        .map(str::len)
        .sum();
    let kind = if edit.kind == SettingEditorKind::Integer {
        "Number"
    } else {
        "Text"
    };
    frame.render_widget(
        Paragraph::new(kind).style(Style::default().fg(theme.text.secondary)),
        Rect::new(input.x, inner.y + 1, input.width, 1),
    );
    render_input(
        frame,
        theme,
        input,
        &text,
        cursor,
        true,
        edit.editor.selection().is_some(),
    );
    if inner.height > 4 {
        let message = edit
            .error
            .as_deref()
            .unwrap_or("Applies next session · Ctrl+A selects all");
        frame.render_widget(
            Paragraph::new(crate::ui::safe_product_text(message))
                .style(Style::default().fg(if edit.error.is_some() {
                    theme.status.error
                } else {
                    theme.text.secondary
                }))
                .wrap(Wrap { trim: false }),
            Rect::new(
                input.x,
                input.y + 1,
                input.width,
                inner.height.saturating_sub(4),
            ),
        );
    }
}

fn render_input(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    text: &str,
    cursor: usize,
    focused: bool,
    selected: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let (visible, column) =
        super::new_worktree_dialog::input_viewport(text, cursor, usize::from(area.width));
    let mut style = Style::default()
        .fg(theme.text.primary)
        .bg(theme.surface.hover);
    if selected {
        style = style.add_modifier(Modifier::REVERSED);
    }
    frame.render_widget(
        Paragraph::new(crate::ui::safe_product_text(&visible)).style(style),
        area,
    );
    if focused {
        let position = (area.x + u16::try_from(column).unwrap_or(0), area.y);
        if let Some(cell) = frame.buffer_mut().cell_mut(position) {
            cell.set_style(
                Style::default()
                    .fg(theme.surface.hover)
                    .bg(theme.text.primary),
            );
        }
        frame.set_cursor_position(position);
    }
}
