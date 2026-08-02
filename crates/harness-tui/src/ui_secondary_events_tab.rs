use super::*;

use ratatui::widgets::{Block, BorderType, Borders, Clear};

pub(super) fn render_help_tab(
    frame: &mut Frame,
    app: &AppState,
    root: Rect,
    _content: Rect,
    composer: Option<Rect>,
    theme: &Theme,
) {
    let Some(overlay) = help_modal_area(root, composer) else {
        return;
    };

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

fn help_modal_area(root: Rect, composer: Option<Rect>) -> Option<Rect> {
    let tokens = crate::layout::HELP_MODAL_LAYOUT;
    let max_width = root.width.saturating_sub(4).min(tokens.max_width);
    let preferred_width = u16::try_from(
        u32::from(root.width).saturating_mul(tokens.width_numerator) / tokens.width_denominator,
    )
    .unwrap_or(u16::MAX);
    let width = preferred_width
        .min(max_width)
        .max(tokens.min_width)
        .min(root.width);
    let default_height = root
        .height
        .saturating_sub(tokens.vertical_margin.saturating_mul(2));
    if width < 20 || default_height < 6 {
        return None;
    }
    let x = root.x.saturating_add(root.width.saturating_sub(width) / 2);
    let y = root
        .y
        .saturating_add(root.height.saturating_sub(default_height) / 2);
    let default_bottom = y.saturating_add(default_height.saturating_sub(1));
    let root_bottom = root.y.saturating_add(root.height.saturating_sub(1));
    let bottom = composer
        .map(|area| area.y.saturating_add(1).min(root_bottom))
        .filter(|bottom| *bottom >= y)
        .map_or(default_bottom, |bottom| bottom.min(default_bottom));
    let height = bottom.saturating_sub(y).saturating_add(1);
    Some(Rect::new(x, y, width, height))
}

fn paint_help_modal_panel(frame: &mut Frame, theme: &Theme, overlay: Rect) -> bool {
    if overlay.width == 0 || overlay.height == 0 {
        return false;
    }

    let colors = theme.reference_terminal;
    let panel_style = Style::default().fg(colors.primary).bg(colors.canvas);
    let border_style = panel_style.fg(colors.muted);
    let title_style = panel_style.add_modifier(Modifier::BOLD);
    let close_style = border_style;
    frame.render_widget(Clear, overlay);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(border_style)
        .style(panel_style)
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
    let colors = theme.reference_terminal;
    let chrome = Style::default().fg(colors.muted).bg(colors.canvas);
    let cursor = Style::default().fg(colors.primary).bg(colors.canvas);
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
    let colors = theme.reference_terminal;
    let panel_style = Style::default().fg(colors.primary).bg(colors.canvas);
    frame.render_widget(Block::default().style(panel_style), area);

    let rows = help_shortcut_rows(app, theme, usize::from(area.width));
    let visible = usize::from(area.height);
    for (index, row) in rows.into_iter().take(visible).enumerate() {
        let row_y = area
            .y
            .saturating_add(u16::try_from(index).unwrap_or(u16::MAX));
        let row_area = Rect::new(area.x, row_y, area.width, 1);
        frame.render_widget(Paragraph::new(row).style(panel_style), row_area);
    }
}

fn render_help_footer(frame: &mut Frame, theme: &Theme, primary: Rect, secondary: Rect) {
    let colors = theme.reference_terminal;
    let muted = Style::default().fg(colors.muted).bg(colors.canvas);
    let secondary_text = Style::default().fg(colors.secondary).bg(colors.canvas);
    let key = Style::default()
        .fg(colors.primary)
        .bg(colors.canvas)
        .add_modifier(Modifier::BOLD);
    if primary.width > 0 && primary.height > 0 {
        let spans = vec![
            Span::styled("↑/↓".to_string(), key),
            Span::styled(" nav".to_string(), secondary_text),
            Span::styled("  |  ".to_string(), muted),
            Span::styled("f".to_string(), key),
            Span::styled(" filter".to_string(), secondary_text),
            Span::styled("  |  ".to_string(), muted),
            Span::styled("e/Space/→".to_string(), key),
            Span::styled(" expand".to_string(), secondary_text),
            Span::styled("  |  ".to_string(), muted),
            Span::styled("←".to_string(), key),
            Span::styled(" collapse".to_string(), secondary_text),
            Span::styled("  |  ".to_string(), muted),
            Span::styled("Enter".to_string(), key),
            Span::styled(" details".to_string(), secondary_text),
        ];
        frame.render_widget(
            Paragraph::new(Line::from(spans)).alignment(Alignment::Center),
            primary,
        );
    }
    if secondary.width > 0 && secondary.height > 0 {
        let spans = vec![
            Span::styled("/".to_string(), key),
            Span::styled(" search".to_string(), secondary_text),
            Span::styled("  |  ".to_string(), muted),
            Span::styled("Esc".to_string(), key),
            Span::styled(" close".to_string(), secondary_text),
        ];
        frame.render_widget(
            Paragraph::new(Line::from(spans)).alignment(Alignment::Center),
            secondary,
        );
    }
}

fn help_shortcut_rows(app: &AppState, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let mut rows = vec![
        section_row("Essentials", theme),
        labeled_shortcut_row(
            app,
            theme,
            Action::SubmitPrompt,
            "Send",
            "Enter",
            width,
            true,
        ),
        labeled_shortcut_row(
            app,
            theme,
            Action::FocusNext,
            "Focus scrollback",
            "Tab",
            width,
            false,
        ),
        labeled_shortcut_row(
            app,
            theme,
            Action::DismissModal,
            "Cancel turn",
            "Ctrl+c",
            width,
            false,
        ),
        labeled_shortcut_row(
            app,
            theme,
            Action::VariantCycle,
            "Cycle mode (Normal / Plan / Always-approve)",
            "Shift+Tab",
            width,
            false,
        ),
        labeled_shortcut_row(
            app,
            theme,
            Action::Quit,
            "Quit",
            "Ctrl+q / Ctrl+d",
            width,
            false,
        ),
        labeled_shortcut_row(
            app,
            theme,
            Action::Palette,
            "Command palette",
            "Ctrl+p / ?",
            width,
            false,
        ),
        labeled_shortcut_row(
            app,
            theme,
            Action::Help,
            "Keyboard shortcuts",
            "Ctrl+x / Ctrl+.",
            width,
            false,
        ),
        labeled_shortcut_row(
            app,
            theme,
            Action::OpenStatusDialog,
            "Open the settings modal",
            "F2 / Ctrl+, / Super+,",
            width,
            false,
        ),
        collapsed_section_row("Input", 5, theme),
        collapsed_section_row("Conversation Navigation", 10, theme),
        collapsed_section_row("Conversation Actions", 4, theme),
        collapsed_section_row("Panels", 6, theme),
        collapsed_section_row("Session", 3, theme),
        collapsed_section_row("Dashboard", 17, theme),
    ];
    if app.replay_mode {
        rows.insert(
            0,
            Line::from(Span::raw("  Read-only transcript and shortcuts.")),
        );
    }
    rows
}

fn section_row(title: &str, theme: &Theme) -> Line<'static> {
    let colors = theme.reference_terminal;
    Line::from(vec![
        Span::styled("  ".to_string(), Style::default().fg(colors.primary)),
        Span::styled("◆ ".to_string(), Style::default().fg(colors.muted)),
        Span::styled(title.to_string(), Style::default().fg(colors.primary)),
    ])
}

fn collapsed_section_row(title: &str, count: usize, theme: &Theme) -> Line<'static> {
    let colors = theme.reference_terminal;
    Line::from(vec![
        Span::styled("  ".to_string(), Style::default().fg(colors.primary)),
        Span::styled("› ".to_string(), Style::default().fg(colors.muted)),
        Span::styled(
            format!("{title} ({count})"),
            Style::default().fg(colors.primary),
        ),
    ])
}

fn labeled_shortcut_row(
    _app: &AppState,
    theme: &Theme,
    _action: Action,
    label: &str,
    freeze_binding: &str,
    width: usize,
    is_selected: bool,
) -> Line<'static> {
    shortcut_row_with_binding(theme, label, freeze_binding, width, is_selected)
}

fn shortcut_row_with_binding(
    theme: &Theme,
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
    let colors = theme.reference_terminal;
    let primary = Style::default().fg(colors.primary);
    let muted = Style::default().fg(colors.muted);
    let secondary = Style::default().fg(colors.secondary);
    let bold = primary.add_modifier(Modifier::BOLD);
    let mut spans = if is_selected {
        vec![
            Span::styled("  ".to_string(), primary),
            Span::styled("  ".to_string(), bold),
            Span::styled("◆ ".to_string(), muted),
            Span::styled(label.to_string(), bold),
        ]
    } else {
        vec![
            Span::styled("    ".to_string(), primary),
            Span::styled("◆ ".to_string(), muted),
            Span::styled(label.to_string(), primary),
        ]
    };
    spans.push(Span::styled(" ".repeat(gap.max(1)), primary));
    spans.push(Span::styled(binding.to_string(), secondary));
    spans.push(Span::styled("   ".to_string(), primary));
    let style = if is_selected {
        Style::default()
            .fg(theme.text.primary)
            .bg(theme.surface.card)
    } else {
        Style::default()
    };
    Line::from(spans).style(style)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;
    use ratatui::{backend::TestBackend, style::Color, Terminal};

    #[test]
    fn help_modal_matches_grok_sizing_at_reference_viewports() {
        assert_eq!(
            help_modal_area(Rect::new(0, 0, 120, 40), None),
            Some(Rect::new(20, 4, 80, 32))
        );
        assert_eq!(
            help_modal_area(Rect::new(0, 0, 60, 20), None),
            Some(Rect::new(8, 4, 44, 12))
        );
    }

    #[test]
    fn selected_help_row_uses_full_width_grok_selection_surface() {
        let theme = Theme::harness_chat();
        let line = shortcut_row_with_binding(&theme, "Send", "Enter", 72, true);

        assert_eq!(line.width(), 72);
        assert_eq!(line.style.fg, Some(theme.text.primary));
        assert_eq!(line.style.bg, Some(theme.surface.card));
    }

    #[test]
    fn help_panel_uses_muted_terminal_chrome() {
        let theme = Theme::harness_chat();
        let backend = TestBackend::new(80, 32);
        let mut terminal = Terminal::new(backend).unwrap_or_abort();

        terminal
            .draw(|frame| {
                assert!(paint_help_modal_panel(
                    frame,
                    &theme,
                    Rect::new(0, 0, 80, 32)
                ));
            })
            .unwrap_or_abort();

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 0)].fg, Color::Indexed(8));
        assert_eq!(buffer[(0, 0)].bg, Color::Indexed(0));
        assert_eq!(buffer[(3, 0)].fg, Color::Indexed(15));
        assert_eq!(buffer[(0, 1)].fg, Color::Indexed(8));
        assert_eq!(buffer[(1, 1)].bg, Color::Indexed(0));
    }

    #[test]
    fn help_panel_respects_the_reference_height_cap() {
        let theme = Theme::harness_chat();
        let app = AppState::new_live(None, false, None);
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap_or_abort();
        let composer = Rect::new(2, 37, 116, 3);

        terminal
            .draw(|frame| {
                render_help_tab(
                    frame,
                    &app,
                    Rect::new(0, 0, 120, 40),
                    Rect::default(),
                    Some(composer),
                    &theme,
                );
            })
            .unwrap_or_abort();

        assert_eq!(terminal.backend().buffer()[(20, 35)].symbol(), "└");
    }
}
