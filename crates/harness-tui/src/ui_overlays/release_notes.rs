use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReleaseNoteRow {
    Blank,
    Heading(String),
    Body {
        text: String,
        bold_range: Option<std::ops::Range<usize>>,
    },
}

pub(super) fn render_release_notes_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    root: Rect,
) {
    let popup = crate::layout::release_notes_modal_area(root);
    render_overlay_dim_backdrop(frame, root);
    let key = ModalSurfaceKey::Overlay {
        kind: OverlayKind::ReleaseNotes,
        view: ModalViewKey::Primary,
    };
    if !paint_modal_panel(frame, app, theme, popup, key, "Release Notes") {
        return;
    }

    let compact = root.height <= 20;
    let horizontal_padding = if compact { 1 } else { 2 };
    let content = Rect::new(
        popup.x.saturating_add(1 + horizontal_padding),
        popup.y.saturating_add(if compact { 1 } else { 2 }),
        popup.width.saturating_sub(2 + 2 * horizontal_padding),
        popup.height.saturating_sub(if compact { 3 } else { 5 }),
    );
    if content.width == 0 || content.height == 0 {
        return;
    }

    let rows = release_note_rows(usize::from(content.width));
    let max_scroll = rows.len().saturating_sub(usize::from(content.height));
    let offset = app.modal_visual_offset(key, app.release_notes_scroll, max_scroll);
    let surface = ui_chrome::command_palette_surface(theme);
    let body_style = Style::default().fg(theme.text.primary).bg(surface);
    let heading_style = body_style.add_modifier(Modifier::BOLD);
    for (visual_row, row) in rows
        .iter()
        .skip(offset)
        .take(usize::from(content.height))
        .enumerate()
    {
        let area = Rect::new(
            content.x,
            content
                .y
                .saturating_add(u16::try_from(visual_row).unwrap_or(u16::MAX)),
            content.width,
            1,
        );
        match row {
            ReleaseNoteRow::Blank => {
                frame.render_widget(Paragraph::new(Span::styled("", body_style)), area);
            }
            ReleaseNoteRow::Heading(text) => {
                frame.render_widget(
                    Paragraph::new(Span::styled(text.clone(), heading_style)),
                    area,
                );
            }
            ReleaseNoteRow::Body { text, bold_range } => {
                let line = if let Some(range) = bold_range {
                    Line::from(vec![
                        Span::styled(text[..range.start].to_string(), body_style),
                        Span::styled(text[range.clone()].to_string(), heading_style),
                        Span::styled(text[range.end..].to_string(), body_style),
                    ])
                } else {
                    Line::from(Span::styled(text.clone(), body_style))
                };
                frame.render_widget(Paragraph::new(line), area);
            }
        }
    }

    let footer = Rect::new(
        popup.x,
        popup.bottom().saturating_sub(2),
        popup.width.saturating_sub(2),
        1,
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("↑/↓", heading_style),
            Span::styled(" scroll  |  ", body_style),
            Span::styled("Esc", heading_style),
            Span::styled(" back", body_style),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        footer,
    );
}

pub(crate) fn release_notes_max_scroll(popup: Rect) -> usize {
    let compact = popup.height <= 20;
    let horizontal_padding = if compact { 1 } else { 2 };
    let width = popup.width.saturating_sub(2 + 2 * horizontal_padding);
    let height = popup.height.saturating_sub(if compact { 3 } else { 5 });
    release_note_rows(usize::from(width))
        .len()
        .saturating_sub(usize::from(height))
}

fn release_note_rows(width: usize) -> Vec<ReleaseNoteRow> {
    let width = width.max(1);
    let mut rows = Vec::new();
    rows.push(ReleaseNoteRow::Heading(format!(
        "{} — current   ",
        env!("CARGO_PKG_VERSION")
    )));
    rows.push(ReleaseNoteRow::Blank);
    rows.push(ReleaseNoteRow::Heading("Features".to_string()));
    rows.push(ReleaseNoteRow::Blank);
    push_wrapped_bullet(
        &mut rows,
        "/session-info",
        "now lets you click any row to copy its value, with hover highlights and a copy-all shortcut.",
        width,
    );
    rows.push(ReleaseNoteRow::Blank);
    rows.push(ReleaseNoteRow::Heading("Performance".to_string()));
    rows.push(ReleaseNoteRow::Blank);
    push_wrapped_bullet(
        &mut rows,
        "Subagent spawning",
        "is dramatically faster when you have many sessions in ~/.harness",
        width,
    );
    push_wrapped_bullet(
        &mut rows,
        "TUI rendering",
        "now automatically matches high-refresh displays (120 Hz+) for smoother scrolling and painting.",
        width,
    );
    rows.push(ReleaseNoteRow::Blank);
    rows.push(ReleaseNoteRow::Blank);
    rows.push(ReleaseNoteRow::Heading("Earlier changes   ".to_string()));
    rows.push(ReleaseNoteRow::Blank);
    rows.push(ReleaseNoteRow::Blank);
    push_wrapped(
        &mut rows,
        "• Reliability and terminal interaction improvements.",
        width,
    );
    push_wrapped(
        &mut rows,
        "• Session replay remains side-effect free and follows contiguous event order.",
        width,
    );
    push_wrapped(
        &mut rows,
        "• Permission prompts precede native tool execution.",
        width,
    );
    push_wrapped(
        &mut rows,
        "• Terminal layouts adapt to compact and wide viewports.",
        width,
    );
    rows.push(ReleaseNoteRow::Blank);
    rows.push(ReleaseNoteRow::Heading("Tooling".to_string()));
    rows.push(ReleaseNoteRow::Blank);
    push_wrapped(
        &mut rows,
        "• Native tools use stable typed identifiers across runtime surfaces.",
        width,
    );
    push_wrapped(
        &mut rows,
        "• Configuration validation reports invalid settings before startup.",
        width,
    );
    push_wrapped(
        &mut rows,
        "• Session inspection reads replay-derived state without executing providers or tools.",
        width,
    );
    rows.push(ReleaseNoteRow::Blank);
    rows.push(ReleaseNoteRow::Heading("Safety".to_string()));
    rows.push(ReleaseNoteRow::Blank);
    push_wrapped(
        &mut rows,
        "• Provider metadata is redacted before it reaches persisted events and artifacts.",
        width,
    );
    push_wrapped(
        &mut rows,
        "• Anchored workspace edits reject overlaps and write atomically.",
        width,
    );
    push_wrapped(
        &mut rows,
        "• Permission checks complete before native tool execution begins.",
        width,
    );
    rows
}

fn push_wrapped(rows: &mut Vec<ReleaseNoteRow>, text: &str, width: usize) {
    let mut line = String::new();
    for word in text.split_whitespace() {
        let separator = usize::from(!line.is_empty());
        if !line.is_empty() && line.chars().count() + separator + word.chars().count() > width {
            rows.push(ReleaseNoteRow::Body {
                text: std::mem::take(&mut line),
                bold_range: None,
            });
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        rows.push(ReleaseNoteRow::Body {
            text: line,
            bold_range: None,
        });
    }
}

fn push_wrapped_bullet(rows: &mut Vec<ReleaseNoteRow>, title: &str, detail: &str, width: usize) {
    let text = format!("• {title} {detail}");
    let start = "• ".len();
    let end = start + title.len();
    let first_row = rows.len();
    push_wrapped(rows, &text, width);
    if let Some(ReleaseNoteRow::Body { bold_range, .. }) = rows.get_mut(first_row) {
        *bold_range = Some(start..end);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;

    #[test]
    fn release_notes_geometry_matches_responsive_contract() {
        // arrange
        let viewports = [
            (Rect::new(0, 0, 60, 20), Rect::new(6, 0, 48, 20)),
            (Rect::new(0, 0, 100, 30), Rect::new(10, 4, 80, 22)),
            (Rect::new(0, 0, 140, 40), Rect::new(14, 4, 112, 32)),
        ];

        // act
        let actual =
            viewports.map(|(viewport, _)| crate::layout::release_notes_modal_area(viewport));

        // assert
        assert_eq!(actual, viewports.map(|(_, expected)| expected));
    }

    #[test]
    fn release_notes_content_is_product_neutral_and_truthful() {
        // arrange
        let width = 76;

        // act
        let text = release_note_rows(width)
            .into_iter()
            .filter_map(|row| match row {
                ReleaseNoteRow::Blank => None,
                ReleaseNoteRow::Heading(text) | ReleaseNoteRow::Body { text, .. } => Some(text),
            })
            .collect::<Vec<_>>()
            .join("\n");

        // assert
        assert!(text.contains(env!("CARGO_PKG_VERSION")));
        assert!(text.contains("~/.harness"));
        assert!(!text.to_ascii_lowercase().contains("grok"));
    }

    #[test]
    fn release_notes_renderer_paints_shared_chrome_content_and_footer() {
        // arrange
        let mut app = AppState::new_startup(Vec::new(), None);
        app.release_notes_visible = true;
        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut terminal = ratatui::Terminal::new(backend).unwrap_or_abort();

        // act
        terminal
            .draw(|frame| {
                render_release_notes_overlay(frame, &app, app.theme(), Rect::new(0, 0, 100, 30));
            })
            .unwrap_or_abort();
        let debug = format!("{:?}", terminal.backend().buffer());

        // assert
        assert!(debug.contains("Release Notes"));
        assert!(debug.contains(env!("CARGO_PKG_VERSION")));
        assert!(debug.contains("↑/↓"));
        assert!(debug.contains("Esc"));
    }

    #[test]
    fn release_notes_current_marker_paints_the_complete_date_field_bold() {
        // arrange
        let mut app = AppState::new_startup(Vec::new(), None);
        app.release_notes_visible = true;
        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut terminal = ratatui::Terminal::new(backend).unwrap_or_abort();

        // act
        terminal
            .draw(|frame| {
                render_release_notes_overlay(frame, &app, app.theme(), Rect::new(0, 0, 100, 30));
            })
            .unwrap_or_abort();

        // assert
        let buffer = terminal.backend().buffer();
        let rendered = (21..31)
            .map(|column| buffer[(column, 6)].symbol())
            .collect::<String>();
        assert_eq!(rendered, "current   ");
        assert!((21..31).all(|column| buffer[(column, 6)].modifier.contains(Modifier::BOLD)));
        let history = (13..31)
            .map(|column| buffer[(column, 21)].symbol())
            .collect::<String>();
        assert_eq!(history, "Earlier changes   ");
        assert!((13..31).all(|column| buffer[(column, 21)].modifier.contains(Modifier::BOLD)));
        let footer = (38..61)
            .map(|column| buffer[(column, 24)].symbol())
            .collect::<String>();
        assert_eq!(footer, "↑/↓ scroll  |  Esc back");
    }

    #[test]
    fn release_notes_renderer_consumes_the_keyboard_scroll_offset() {
        // arrange
        let mut app = AppState::new_startup(Vec::new(), None);
        app.release_notes_visible = true;
        app.release_notes_scroll = usize::MAX;
        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut terminal = ratatui::Terminal::new(backend).unwrap_or_abort();

        // act
        terminal
            .draw(|frame| {
                render_release_notes_overlay(frame, &app, app.theme(), Rect::new(0, 0, 100, 30));
            })
            .unwrap_or_abort();
        let debug = format!("{:?}", terminal.backend().buffer());

        // assert
        assert!(debug.contains("Safety"));
        assert!(!debug.contains("0.1.0 — current"));
    }
}
