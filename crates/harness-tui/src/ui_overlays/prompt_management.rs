use super::*;

pub(super) fn render_prompt_management_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    root: Rect,
    overlay: Option<Rect>,
    kind: OverlayKind,
) {
    let Some(overlay) = overlay else {
        return;
    };

    render_overlay_dim_backdrop(frame, root);
    if !paint_command_palette_panel(frame, theme, overlay) {
        return;
    }

    let Some((header, input, list)) = command_palette_dialog_layout(overlay) else {
        return;
    };
    let title = match kind {
        OverlayKind::PromptStash => "Prompt stash",
        OverlayKind::QueuedPrompts => "Queued prompts",
        _ => return,
    };
    render_command_palette_header(frame, theme, header, title);
    render_prompt_management_hint(frame, theme, input, kind);
    render_prompt_management_list(frame, app, theme, list, kind);
}

fn render_prompt_management_hint(frame: &mut Frame, theme: &Theme, area: Rect, kind: OverlayKind) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = ui_chrome::command_palette_surface(theme);
    let hint = match kind {
        OverlayKind::PromptStash => "Enter restores · Del removes",
        OverlayKind::QueuedPrompts => "Queued for next turn",
        _ => "",
    };
    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);
    frame.render_widget(
        Paragraph::new(truncate_plain_text(hint, usize::from(area.width))).style(
            Style::default()
                .fg(ui_chrome::command_palette_muted(theme))
                .bg(surface),
        ),
        area,
    );
}

fn render_prompt_management_list(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    area: Rect,
    kind: OverlayKind,
) {
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

    match kind {
        OverlayKind::PromptStash => render_stash_rows(frame, app, theme, list_area),
        OverlayKind::QueuedPrompts => render_queued_rows(frame, app, theme, list_area),
        _ => {}
    }
}

fn render_stash_rows(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    let entries = app.composer.prompt_stash_entries();
    if entries.is_empty() {
        render_palette_empty_message(frame, theme, area, "No stashed prompts");
        return;
    }

    let selected = app
        .composer
        .prompt_stash_selected()
        .min(entries.len().saturating_sub(1));
    render_prompt_rows(
        frame,
        theme,
        area,
        entries.iter().map(|entry| entry.text()),
        selected,
    );
}

fn render_queued_rows(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    let entries = app.composer.queued_prompt_entries();
    if entries.is_empty() {
        render_palette_empty_message(frame, theme, area, "No queued prompts");
        return;
    }

    let selected = app
        .composer
        .queued_prompt_selected()
        .min(entries.len().saturating_sub(1));
    render_prompt_rows(
        frame,
        theme,
        area,
        entries.iter().map(|entry| entry.text()),
        selected,
    );
}

fn render_prompt_rows<'a>(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    rows: impl Iterator<Item = &'a str>,
    selected: usize,
) {
    let rows = rows.collect::<Vec<_>>();
    let visible_rows = usize::from(area.height);
    let scroll = selected.saturating_sub(visible_rows.saturating_sub(1));

    for (row, text) in rows.iter().enumerate().skip(scroll).take(visible_rows) {
        let row_area = Rect::new(
            area.x,
            area.y
                .saturating_add(u16::try_from(row - scroll).unwrap_or(u16::MAX)),
            area.width,
            1,
        );
        let is_selected = row == selected;
        if is_selected {
            frame.render_widget(
                Block::default().style(ui_chrome::overlay_focus_row_style(theme)),
                row_area,
            );
        }
        frame.render_widget(
            Paragraph::new(prompt_management_row(
                text,
                is_selected,
                theme,
                row_area.width,
            )),
            row_area,
        );
    }
}

fn prompt_management_row(
    text: &str,
    is_selected: bool,
    theme: &Theme,
    width: u16,
) -> Line<'static> {
    let surface = ui_chrome::command_palette_surface(theme);
    let row_style = if is_selected {
        ui_chrome::overlay_focus_row_style(theme)
    } else {
        Style::default().bg(surface)
    };
    let text_style = if is_selected {
        row_style.add_modifier(Modifier::BOLD)
    } else {
        row_style.fg(ui_chrome::command_palette_title(theme))
    };
    let row_width = usize::from(width);
    let prefix = "   ";
    let preview = crate::text::collapse_inline_whitespace(text);
    let preview = truncate_plain_text(&preview, row_width.saturating_sub(prefix.chars().count()));
    let used = prefix
        .chars()
        .count()
        .saturating_add(preview.chars().count());
    let trailing = row_width.saturating_sub(used);
    Line::from(vec![
        Span::styled(prefix, row_style),
        Span::styled(preview, text_style),
        Span::styled(" ".repeat(trailing), row_style),
    ])
}
