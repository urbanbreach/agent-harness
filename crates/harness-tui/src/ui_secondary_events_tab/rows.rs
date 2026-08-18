use super::*;

use unicode_width::UnicodeWidthStr;

use crate::app::{HelpRow, ModalSurfaceKey, ModalTarget};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HelpRowLayout {
    pub(crate) areas: Vec<(usize, Rect)>,
    pub(crate) offset: usize,
    pub(crate) max_scroll: usize,
}

pub(super) fn render_browse(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    let rows = app.help_rows();
    let layout = help_row_layout(app, area, &rows);
    let mut visual_y = 0usize;
    for (index, row) in rows.iter().enumerate() {
        let height = row_height(row, area.width);
        let row_end = visual_y.saturating_add(height);
        if row_end <= layout.offset {
            visual_y = row_end;
            continue;
        }
        let visible_start = visual_y.max(layout.offset);
        let screen_y = area.y.saturating_add(
            u16::try_from(visible_start.saturating_sub(layout.offset)).unwrap_or(u16::MAX),
        );
        if screen_y >= area.bottom() {
            break;
        }
        render_row(
            frame,
            app,
            theme,
            area,
            screen_y,
            visible_start.saturating_sub(visual_y),
            index,
            row,
        );
        visual_y = row_end;
    }
    if layout.max_scroll > 0 && area.width > 0 {
        render_scrollbar(frame, theme, area, layout.offset, layout.max_scroll);
    }
}

pub(crate) fn help_row_layout(app: &AppState, area: Rect, rows: &[HelpRow]) -> HelpRowLayout {
    let heights = rows
        .iter()
        .map(|row| row_height(row, area.width))
        .collect::<Vec<_>>();
    let total = heights.iter().sum::<usize>();
    let visible = usize::from(area.height);
    let max_scroll = total.saturating_sub(visible);
    let selected_end = heights
        .iter()
        .take(app.help_browser.selected.saturating_add(1))
        .sum::<usize>();
    let keyboard_offset = selected_end.saturating_sub(visible);
    let offset = if app.help_browser.follow_selection {
        app.help_browser.visual_offset.max(keyboard_offset)
    } else {
        app.help_browser.visual_offset
    }
    .min(max_scroll);
    let mut areas = Vec::new();
    let mut visual_y = 0usize;
    for (index, height) in heights.into_iter().enumerate() {
        let row_end = visual_y.saturating_add(height);
        if row_end > offset && visual_y < offset.saturating_add(visible) {
            let start = visual_y.max(offset);
            let end = row_end.min(offset.saturating_add(visible));
            areas.push((
                index,
                Rect::new(
                    area.x,
                    area.y.saturating_add(
                        u16::try_from(start.saturating_sub(offset)).unwrap_or(u16::MAX),
                    ),
                    area.width,
                    u16::try_from(end.saturating_sub(start)).unwrap_or(u16::MAX),
                ),
            ));
        }
        visual_y = row_end;
    }
    HelpRowLayout {
        areas,
        offset,
        max_scroll,
    }
}

fn row_height(row: &HelpRow, width: u16) -> usize {
    match row {
        HelpRow::Shortcut {
            description,
            expanded: true,
            ..
        } => 1usize.saturating_add(wrapped_lines(description, width.saturating_sub(4)).len()),
        HelpRow::Section { .. } | HelpRow::Shortcut { .. } => 1,
    }
}

fn render_row(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    list: Rect,
    y: u16,
    skipped_lines: usize,
    index: usize,
    row: &HelpRow,
) {
    let hovered = app.modal_target_hovered(ModalSurfaceKey::Help, ModalTarget::Row(index));
    let selected = app.help_browser.selected == index;
    let background = if selected {
        theme.question_prompt.selected
    } else if hovered {
        theme.surface.card
    } else {
        theme.reference_terminal.canvas
    };
    let foreground = match row {
        HelpRow::Shortcut { dimmed: true, .. } if !selected => theme.reference_terminal.muted,
        HelpRow::Section { .. } | HelpRow::Shortcut { .. } => theme.reference_terminal.primary,
    };
    let style = Style::default()
        .fg(foreground)
        .bg(background)
        .add_modifier(if selected {
            Modifier::BOLD
        } else {
            Modifier::empty()
        });
    let line = match row {
        HelpRow::Section {
            section,
            count,
            collapsed,
        } => {
            let label = if *collapsed {
                format!("{} ({count})", section.label())
            } else {
                section.label().to_owned()
            };
            row_line(
                if *collapsed { "› " } else { "◆ " },
                &label,
                "",
                list.width,
                style,
                style,
            )
        }
        HelpRow::Shortcut {
            label,
            bindings,
            dimmed,
            ..
        } => row_line(
            "  ◆ ",
            label,
            bindings,
            list.width,
            style,
            Style::default()
                .fg(if *dimmed {
                    theme.reference_terminal.muted
                } else {
                    theme.reference_terminal.secondary
                })
                .bg(background),
        ),
    };
    let visible_height = row_height(row, list.width)
        .saturating_sub(skipped_lines)
        .min(usize::from(list.bottom().saturating_sub(y)));
    let row_area = Rect::new(
        list.x,
        y,
        list.width,
        u16::try_from(visible_height).unwrap_or(u16::MAX),
    );
    frame.render_widget(
        Block::default().style(Style::default().bg(background)),
        row_area,
    );
    if skipped_lines == 0 {
        frame.render_widget(Paragraph::new(line), Rect::new(list.x, y, list.width, 1));
    }
    if let HelpRow::Shortcut {
        description,
        expanded: true,
        ..
    } = row
    {
        let description_start = skipped_lines.saturating_sub(1);
        let first_description_y = y.saturating_add(u16::from(skipped_lines == 0));
        for (line_index, description_line) in
            wrapped_lines(description, list.width.saturating_sub(4))
                .into_iter()
                .skip(description_start)
                .enumerate()
        {
            let description_y =
                first_description_y.saturating_add(u16::try_from(line_index).unwrap_or(u16::MAX));
            if description_y >= list.bottom() {
                break;
            }
            frame.render_widget(
                Paragraph::new(format!("    {description_line}")).style(
                    Style::default()
                        .fg(theme.reference_terminal.secondary)
                        .bg(background),
                ),
                Rect::new(list.x, description_y, list.width, 1),
            );
        }
    }
}

fn row_line(
    prefix: &str,
    label: &str,
    right: &str,
    width: u16,
    style: Style,
    right_style: Style,
) -> Line<'static> {
    let used = UnicodeWidthStr::width(prefix)
        .saturating_add(UnicodeWidthStr::width(label))
        .saturating_add(UnicodeWidthStr::width(right));
    let gap = usize::from(width)
        .saturating_sub(used.saturating_add(1))
        .max(1);
    Line::from(vec![
        Span::styled(prefix.to_owned(), style),
        Span::styled(label.to_owned(), style),
        Span::styled(" ".repeat(gap), style),
        Span::styled(right.to_owned(), right_style),
    ])
}

pub(crate) fn wrapped_lines(text: &str, width: u16) -> Vec<String> {
    let width = usize::from(width).max(1);
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let next_width = UnicodeWidthStr::width(current.as_str())
            .saturating_add(usize::from(!current.is_empty()))
            .saturating_add(UnicodeWidthStr::width(word));
        if next_width > width && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

fn render_scrollbar(frame: &mut Frame, theme: &Theme, area: Rect, offset: usize, max: usize) {
    let x = area.right().saturating_sub(1);
    let thumb = offset
        .saturating_mul(usize::from(area.height.saturating_sub(1)))
        .checked_div(max)
        .unwrap_or(0);
    for row in 0..area.height {
        let color = if usize::from(row) == thumb {
            theme.scrollbar.thumb_active
        } else {
            theme.scrollbar.track
        };
        frame.render_widget(
            Paragraph::new(if usize::from(row) == thumb {
                "█"
            } else {
                " "
            })
            .style(
                Style::default()
                    .fg(color)
                    .bg(theme.reference_terminal.canvas),
            ),
            Rect::new(x, area.y.saturating_add(row), 1, 1),
        );
    }
}
