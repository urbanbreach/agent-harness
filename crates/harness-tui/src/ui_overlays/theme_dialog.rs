use super::*;

pub(super) fn render_theme_dialog_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    root: Rect,
) {
    let Some(dialog_area) = theme_dialog_area(root) else {
        return;
    };

    render_overlay_dim_backdrop(frame, root);
    if !super::paint_modal_panel(
        frame,
        app,
        theme,
        dialog_area,
        ModalSurfaceKey::Overlay {
            kind: OverlayKind::ThemeDialog,
            view: ModalViewKey::Primary,
        },
        "Themes",
    ) {
        return;
    }

    let content = inset_rect(dialog_area, 1.min(dialog_area.width.saturating_sub(1)), 1);
    if content.width == 0 || content.height == 0 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(content);
    render_command_palette_header(frame, theme, chunks[0], "Themes");
    render_theme_dialog_body(frame, app, theme, chunks[1]);
}

pub(super) fn theme_dialog_area(root: Rect) -> Option<Rect> {
    let width = 44u16.min(root.width.saturating_sub(4));
    let height = 8u16.min(root.height.saturating_sub(4));
    (width >= 32 && height >= 6).then(|| {
        Rect::new(
            root.x + (root.width.saturating_sub(width)) / 2,
            root.y + (root.height.saturating_sub(height)) / 2,
            width,
            height,
        )
    })
}

pub(super) fn theme_dialog_row_areas(dialog_area: Rect) -> Vec<Rect> {
    let content = inset_rect(dialog_area, 1.min(dialog_area.width.saturating_sub(1)), 1);
    if content.width == 0 || content.height == 0 {
        return Vec::new();
    }
    let body = Rect::new(
        content.x,
        content.y.saturating_add(1),
        content.width,
        content.height.saturating_sub(1),
    );
    Theme::available_theme_names()
        .iter()
        .enumerate()
        .filter_map(|(index, _)| {
            let y = body
                .y
                .saturating_add(1)
                .saturating_add(u16::try_from(index).unwrap_or(u16::MAX));
            (y < body.bottom()).then_some(Rect::new(body.x, y, body.width, 1))
        })
        .collect()
}

fn render_theme_dialog_body(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = ui_chrome::command_palette_surface(theme);
    let names = Theme::available_theme_names();
    let name_count = u16::try_from(names.len()).unwrap_or(u16::MAX);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(name_count),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);

    frame.render_widget(
        Block::default().style(Style::default().bg(surface)),
        chunks[0],
    );

    for (index, name) in names.iter().enumerate() {
        let row_area = Rect::new(
            chunks[1].x,
            chunks[1]
                .y
                .saturating_add(u16::try_from(index).unwrap_or(u16::MAX)),
            chunks[1].width,
            1,
        );
        let is_selected = index == app.theme_dialog_selected;
        let is_current = *name == app.theme_name;
        let key = ModalSurfaceKey::Overlay {
            kind: OverlayKind::ThemeDialog,
            view: ModalViewKey::Primary,
        };
        let presentation = modal_list_row(
            theme,
            ModalListRowSpec {
                area: row_area,
                state: ModalListRowState {
                    selected: is_selected,
                    hovered: app.modal_target_hovered(key, ModalTarget::Row(index)),
                    dimmed: false,
                },
                max_scroll: 0,
            },
        );
        frame.render_widget(
            Block::default().style(presentation.style),
            presentation.layout.content,
        );

        let prefix = "  ";
        let marker = if is_current { "● " } else { "  " };
        let label: &'static str = match *name {
            "default" => "Harness Chat",
            "high-contrast" => "High Contrast",
            _ => name,
        };
        let fg = if is_selected {
            theme.text.primary
        } else {
            ui_chrome::command_palette_title(theme)
        };
        let label_style = modal_list_row_text_style(presentation.style, fg);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(prefix, label_style),
                Span::styled(marker, label_style),
                Span::styled(label, label_style),
            ])),
            presentation.layout.content,
        );
    }

    frame.render_widget(
        Block::default().style(Style::default().bg(surface)),
        chunks[2],
    );

    let muted_style = Style::default()
        .fg(ui_chrome::command_palette_muted(theme))
        .bg(surface);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "enter apply · esc close",
            muted_style,
        ))),
        chunks[3],
    );
}
