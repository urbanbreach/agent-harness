use super::*;

use ratatui::widgets::{Block, Borders, BorderType, Clear};

pub(super) fn render_help_tab(
    frame: &mut Frame,
    app: &AppState,
    root: Rect,
    content: Rect,
    composer: Option<Rect>,
    theme: &Theme,
) {
    let Some(overlay) = help_modal_area(root) else {
        return;
    };

    paint_help_solid_backdrop(frame, content, composer);
    if !paint_help_modal_panel(frame, theme, overlay) {
        return;
    }

    let Some((input, list, primary_footer, secondary_footer)) = help_modal_layout(overlay) else {
        return;
    };

    render_help_search_row(frame, theme, input);
    render_help_list(frame, app, theme, list);
    render_help_footer(frame, theme, primary_footer, secondary_footer);
}

fn help_modal_area(root: Rect) -> Option<Rect> {
    const WIDTH: u16 = 80;
    const HEIGHT: u16 = 24;
    let width = WIDTH.min(root.width.saturating_sub(4)).max(40.min(root.width));
    let height = HEIGHT
        .min(root.height.saturating_sub(4))
        .max(12.min(root.height));
    if width < 32 || height < 10 {
        return None;
    }
    let x = root
        .x
        .saturating_add(root.width.saturating_sub(width) / 2);
    let max_y = root
        .y
        .saturating_add(root.height.saturating_sub(height.max(1)));
    let y = 4u16.clamp(root.y, max_y.max(root.y));
    Some(Rect::new(x, y, width, height))
}

fn paint_help_solid_backdrop(frame: &mut Frame, area: Rect, preserve: Option<Rect>) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let buffer = frame.buffer_mut();
    let max_x = area.x.saturating_add(area.width);
    let max_y = area.y.saturating_add(area.height);
    let preserve = preserve.filter(|rect| rect.width > 0 && rect.height > 0);
    for y in area.y..max_y {
        for x in area.x..max_x {
            if preserve.is_some_and(|rect| {
                x >= rect.x
                    && y >= rect.y
                    && x < rect.x.saturating_add(rect.width)
                    && y < rect.y.saturating_add(rect.height)
            }) {
                continue;
            }
            let cell = &mut buffer[(x, y)];
            cell.set_symbol(" ");
            cell.set_fg(Color::Reset);
            cell.set_bg(Color::Reset);
        }
    }
}

fn paint_help_modal_panel(frame: &mut Frame, theme: &Theme, overlay: Rect) -> bool {
    if overlay.width == 0 || overlay.height == 0 {
        return false;
    }

    let surface = ui_chrome::command_palette_surface(theme);
    let border_style = Style::default().bg(surface);
    let title_style = Style::default()
        .bg(surface)
        .add_modifier(Modifier::BOLD);
    let close_style = border_style;
    frame.render_widget(Clear, overlay);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(border_style)
        .style(Style::default().bg(surface))
        .title(Line::from(vec![
            Span::styled("─ ", border_style),
            Span::styled("Keyboard Shortcuts", title_style),
            Span::styled(" ", border_style),
        ]))
        .title(
            Line::from(vec![
                Span::styled(" [", close_style),
                Span::styled("✗", close_style),
                Span::styled("] ─", close_style),
            ])
            .right_aligned(),
        );
    frame.render_widget(block, overlay);
    true
}

fn help_modal_layout(overlay: Rect) -> Option<(Rect, Rect, Rect, Rect)> {
    if overlay.width <= 8 || overlay.height <= 6 {
        return None;
    }
    let inner = Rect::new(
        overlay.x.saturating_add(1),
        overlay.y.saturating_add(1),
        overlay.width.saturating_sub(2),
        overlay.height.saturating_sub(2),
    );
    if inner.width <= 4 || inner.height <= 5 {
        return None;
    }
    let content_x = inner.x.saturating_add(2);
    let content_width = inner.width.saturating_sub(4);
    let input = Rect::new(content_x, inner.y.saturating_add(1), content_width, 1);
    let secondary_footer = Rect::new(
        content_x,
        inner.y.saturating_add(inner.height.saturating_sub(1)),
        content_width,
        1,
    );
    let primary_footer = Rect::new(
        content_x,
        secondary_footer.y.saturating_sub(1),
        content_width,
        1,
    );
    let list_top = inner.y.saturating_add(3);
    let list_bottom = primary_footer.y.saturating_sub(1);
    let list_height = list_bottom.saturating_sub(list_top);
    if list_height == 0 {
        return None;
    }
    let list = Rect::new(inner.x, list_top, inner.width, list_height);
    Some((input, list, primary_footer, secondary_footer))
}

fn render_help_search_row(frame: &mut Frame, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let surface = ui_chrome::command_palette_surface(theme);
    let chrome = Style::default().bg(surface);
    let cursor = Style::default()
        .fg(ui_chrome::command_palette_cursor(theme))
        .bg(surface);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" / to search", chrome),
            Span::styled(" ", cursor),
        ])),
        area,
    );

    let rule_y = area.y.saturating_add(1);
    let rule_area = Rect::new(
        area.x.saturating_sub(2),
        rule_y,
        area.width.saturating_add(4),
        1,
    );
    if rule_area.width > 0 {
        let rule = "─".repeat(usize::from(rule_area.width));
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(rule, chrome))),
            rule_area,
        );
    }
}

fn render_help_list(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let surface = ui_chrome::command_palette_surface(theme);
    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);

    let rows = help_shortcut_rows(app, usize::from(area.width));
    let visible = usize::from(area.height);
    for (index, row) in rows.into_iter().take(visible).enumerate() {
        let row_y = area
            .y
            .saturating_add(u16::try_from(index).unwrap_or(u16::MAX));
        let row_area = Rect::new(area.x, row_y, area.width, 1);
        frame.render_widget(
            Paragraph::new(row).style(Style::default().bg(surface)),
            row_area,
        );
    }
}

fn render_help_footer(
    frame: &mut Frame,
    _theme: &Theme,
    primary: Rect,
    secondary: Rect,
) {
    let muted = Style::default();
    let key = Style::default().add_modifier(Modifier::BOLD);
    if primary.width > 0 && primary.height > 0 {
        let spans = vec![
            Span::styled("↑/↓".to_string(), key),
            Span::styled(" nav  |  ".to_string(), muted),
            Span::styled("f".to_string(), key),
            Span::styled(" filter  |  ".to_string(), muted),
            Span::styled("e/Space/→".to_string(), key),
            Span::styled(" expand  |  ".to_string(), muted),
            Span::styled("←".to_string(), key),
            Span::styled(" collapse  |  ".to_string(), muted),
            Span::styled("Enter".to_string(), key),
            Span::styled(" details".to_string(), muted),
        ];
        frame.render_widget(
            Paragraph::new(Line::from(spans)).alignment(Alignment::Center),
            primary,
        );
    }
    if secondary.width > 0 && secondary.height > 0 {
        let spans = vec![
            Span::styled("/".to_string(), key),
            Span::styled(" search  |  ".to_string(), muted),
            Span::styled("Esc".to_string(), key),
            Span::styled(" close".to_string(), muted),
        ];
        frame.render_widget(
            Paragraph::new(Line::from(spans)).alignment(Alignment::Center),
            secondary,
        );
    }
}

fn help_shortcut_rows(app: &AppState, width: usize) -> Vec<Line<'static>> {
    let mut rows = vec![
        section_row("Essentials"),
        labeled_shortcut_row(app, Action::SubmitPrompt, "Send", "Enter", width, true),
        labeled_shortcut_row(app, Action::FocusNext, "Focus scrollback", "Tab", width, false),
        labeled_shortcut_row(app, Action::DismissModal, "Cancel turn", "Ctrl+c", width, false),
        labeled_shortcut_row(
            app,
            Action::VariantCycle,
            "Cycle mode (Normal / Plan / Always-approve)",
            "Shift+Tab",
            width,
            false,
        ),
        labeled_shortcut_row(app, Action::Quit, "Quit", "Ctrl+q / Ctrl+d", width, false),
        labeled_shortcut_row(app, Action::Palette, "Command palette", "Ctrl+p / ?", width, false),
        labeled_shortcut_row(
            app,
            Action::Help,
            "Keyboard shortcuts",
            "Ctrl+x / Ctrl+.",
            width,
            false,
        ),
        labeled_shortcut_row(
            app,
            Action::OpenStatusDialog,
            "Open the settings modal",
            "F2 / Ctrl+, / Super+,",
            width,
            false,
        ),
        collapsed_section_row("Input", 5),
        collapsed_section_row("Conversation Navigation", 10),
        collapsed_section_row("Conversation Actions", 4),
        collapsed_section_row("Panels", 6),
        collapsed_section_row("Session", 3),
        collapsed_section_row("Dashboard", 17),
    ];
    if app.replay_mode {
        rows.insert(
            0,
            Line::from(Span::raw("  Read-only transcript and shortcuts.")),
        );
    }
    rows
}

fn section_row(title: &str) -> Line<'static> {
    Line::from(Span::raw(format!("  ◆ {title}")))
}

fn collapsed_section_row(title: &str, count: usize) -> Line<'static> {
    Line::from(Span::raw(format!("  › {title} ({count})")))
}

fn labeled_shortcut_row(
    _app: &AppState,
    _action: Action,
    label: &str,
    freeze_binding: &str,
    width: usize,
    is_selected: bool,
) -> Line<'static> {
    shortcut_row_with_binding(label, freeze_binding, width, is_selected)
}

fn shortcut_row_with_binding(
    label: &str,
    binding: &str,
    width: usize,
    is_selected: bool,
) -> Line<'static> {
    let prefix_width = 6usize;
    let label_width = label.chars().count();
    let right_width = binding.chars().count();
    let gap = width.saturating_sub(
        prefix_width
            .saturating_add(label_width)
            .saturating_add(right_width)
            .saturating_add(3),
    );
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let normal = Style::default();
    let mut spans = if is_selected {
        vec![
            Span::raw("  ".to_string()),
            Span::styled("  ".to_string(), bold),
            Span::styled("◆ ".to_string(), normal),
            Span::styled(label.to_string(), bold),
        ]
    } else {
        vec![
            Span::raw("    ◆ ".to_string()),
            Span::raw(label.to_string()),
        ]
    };
    spans.push(Span::raw(format!("{}{binding}   ", " ".repeat(gap.max(1)))));
    Line::from(spans)
}
