// allow: SIZE_OK — TUI rendering (indivisible view model)
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Clear, Paragraph, Wrap},
    Frame,
};

use crate::app::permissions::{
    PermissionConfirmSelection, PermissionModalSelection, PermissionModalStage,
};
use crate::app::{ActivePermissionView, AppState, Focus};
use crate::layout::{
    permission_detail_lines, permission_dock_geometry, permission_dock_measure,
    question_dock_geometry, question_dock_measure,
};
use crate::theme::Theme;

use super::super::ui_overlays::{
    permission_modal_actions_text, permission_modal_draft_line, permission_modal_guidance,
    permission_modal_metadata_line, permission_modal_subject_line, permission_modal_summary_line,
    permission_modal_title, question_permission_actions_text, question_permission_body_text,
};
use super::display_width;
use super::ui_transcript_style::blend_color;

const QUESTION_UNFOCUSED_BLEND: f32 = 0.66;

pub(super) fn render_inline_permission_dock(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    permission: &ActivePermissionView,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let submission_pending = app.permission_submission_pending(&permission.permission_id);
    let is_question = permission.question_prompts.is_some();
    if is_question {
        render_question_permission_dock(frame, app, area, theme, permission, submission_pending);
        return;
    }

    let always_confirm = app.permission_modal_stage(&permission.permission_id)
        == PermissionModalStage::AlwaysConfirm;
    let dock_surface = theme.question_prompt.surface;
    let shell_surface = dock_surface;
    let tray_surface = dock_surface;
    let measure = permission_dock_measure(app, area.width, frame.area().height, permission);
    let geometry = permission_dock_geometry(area, measure);
    let body_area = Rect::new(
        geometry.rail.right(),
        area.y,
        area.width.saturating_sub(geometry.rail.width),
        area.height,
    );
    let tray_body_area = Rect::new(
        geometry.content.x,
        geometry.options.y,
        geometry.content.width,
        geometry
            .options
            .height
            .saturating_add(geometry.footer.height),
    );

    frame.render_widget(
        Block::default().style(Style::default().bg(dock_surface)),
        body_area,
    );

    render_permission_rail(frame, geometry.rail, theme, dock_surface);
    fn render_permission_rail(frame: &mut Frame, rail: Rect, theme: &Theme, dock_surface: Color) {
        if rail.width > 0 && rail.height > 0 {
            let rail_style = Style::default()
                .fg(question_prompt_accent(theme))
                .bg(dock_surface);
            let rail_lines = (0..usize::from(rail.height))
                .map(|_| {
                    Line::from(Span::styled(
                        theme.live_shell.transcript_glyphs.rail,
                        rail_style,
                    ))
                })
                .collect::<Vec<_>>();
            frame.render_widget(
                Paragraph::new(Text::from(rail_lines)).style(Style::default().bg(dock_surface)),
                rail,
            );
        }
    }

    if geometry.content.width == 0 || geometry.content.height == 0 {
        return;
    }

    if geometry.title.height > 0 {
        let title = if always_confirm {
            "Always allow".to_owned()
        } else {
            permission_modal_title(permission)
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                title,
                Style::default()
                    .fg(theme.text.primary)
                    .bg(shell_surface)
                    .add_modifier(Modifier::BOLD),
            ))),
            geometry.title,
        );
    }

    if geometry.detail.height > 0 {
        let indicator =
            measure.detail_truncated && geometry.detail.height >= measure.visible_detail_rows;
        let content_rows = if indicator {
            geometry.detail.height.saturating_sub(1)
        } else {
            geometry.detail.height
        };
        let lines = permission_detail_lines(permission, measure.content_width, content_rows)
            .into_iter()
            .map(|line| {
                let mut lines = super::super::ui_syntax_highlight::render_highlighted_code_block(
                    Some("json"),
                    &line,
                    &line,
                    "",
                    theme.terminal_colors.prompt_accent,
                    theme,
                );
                let mut line = lines.pop().unwrap_or_default();
                for span in &mut line.spans {
                    span.style.bg = Some(shell_surface);
                }
                line
            })
            .collect::<Vec<_>>();
        let rendered_rows = u16::try_from(lines.len()).unwrap_or(u16::MAX);
        let last_width = lines.last().map_or(0, Line::width);
        frame.render_widget(
            Paragraph::new(Text::from(lines)).style(Style::default().bg(shell_surface)),
            Rect::new(
                geometry.detail.x,
                geometry.detail.y,
                geometry.detail.width,
                rendered_rows.min(geometry.detail.height),
            ),
        );
        if indicator && rendered_rows < geometry.detail.height {
            let style = Style::default().fg(theme.text.secondary).bg(shell_surface);
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("... ", style),
                    Span::styled("Ctrl-F", style.fg(question_prompt_accent(theme))),
                    Span::styled(" to expand", style),
                ])),
                Rect::new(
                    geometry.detail.x,
                    geometry.detail.y.saturating_add(rendered_rows),
                    geometry.detail.width,
                    1,
                ),
            );
        } else if measure.detail_rows > rendered_rows && rendered_rows > 0 {
            let offset = u16::try_from(last_width)
                .unwrap_or(u16::MAX)
                .min(geometry.detail.width.saturating_sub(2));
            frame.render_widget(
                Paragraph::new(Span::styled(
                    " …",
                    Style::default().fg(theme.text.secondary),
                )),
                Rect::new(
                    geometry.detail.x + offset,
                    geometry.detail.y + rendered_rows - 1,
                    2,
                    1,
                ),
            );
        }
    }

    if tray_body_area.width == 0 || tray_body_area.height == 0 {
        return;
    }

    let tray_inner = tray_body_area;

    if submission_pending {
        frame.render_widget(
            Paragraph::new(permission_modal_actions_text(
                app,
                theme,
                tray_surface,
                submission_pending,
                permission.question_prompts.is_some(),
            ))
            .style(Style::default().bg(tray_surface))
            .wrap(Wrap { trim: true }),
            tray_inner,
        );
        return;
    }

    if always_confirm {
        let expansion_label = (measure.detail_rows > 5).then_some(if measure.expanded {
            "Ctrl-F to collapse"
        } else {
            "Ctrl-F to expand"
        });
        let action_line = permission_prompt_action_line(
            theme,
            tray_surface,
            &[
                (
                    "Confirm",
                    app.permission_modal_confirm_selection(&permission.permission_id)
                        == PermissionConfirmSelection::Confirm,
                ),
                (
                    "Cancel",
                    app.permission_modal_confirm_selection(&permission.permission_id)
                        == PermissionConfirmSelection::Cancel,
                ),
            ],
        );
        let confirm_selection = app.permission_modal_confirm_selection(&permission.permission_id);
        let confirm_index = match confirm_selection {
            PermissionConfirmSelection::Confirm => 1usize,
            PermissionConfirmSelection::Cancel => 2usize,
        };
        let hint_line = permission_prompt_hint_line(
            app,
            theme,
            tray_surface,
            confirm_index,
            2,
            tray_inner.width,
            expansion_label,
        );
        if tray_inner.height > 1 {
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Length(1)])
                .split(tray_inner);
            frame.render_widget(
                Paragraph::new(action_line).style(Style::default().bg(tray_surface)),
                rows[0],
            );
            frame.render_widget(
                Paragraph::new(hint_line).style(Style::default().bg(tray_surface)),
                rows[1],
            );
            return;
        }
        frame.render_widget(
            Paragraph::new(action_line).style(Style::default().bg(tray_surface)),
            tray_inner,
        );
        return;
    }

    let selection = app.permission_modal_selection(&permission.permission_id);
    let options = [
        (
            "Yes, and don't ask again for anything (always-approve mode)",
            selection == PermissionModalSelection::AllowAlways,
        ),
        (
            "Yes, allow all edits during this session",
            selection == PermissionModalSelection::AllowSession,
        ),
        ("Yes", selection == PermissionModalSelection::AllowOnce),
        (
            "No, reject (type to add feedback)",
            selection == PermissionModalSelection::Reject,
        ),
    ];
    let action_text = permission_prompt_numbered_options(theme, tray_surface, &options);
    render_permission_numbered_options(
        frame,
        geometry.options,
        action_text,
        theme,
        tray_surface,
        &options,
    );
    render_permission_feedback(frame, area, geometry.options, app, permission, theme);
    if app.focus != Focus::Prompt {
        dim_question_permission_dock(frame, area, theme);
    }
}

fn render_question_permission_dock(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    permission: &ActivePermissionView,
    submission_pending: bool,
) {
    let surface = theme.question_prompt.surface;
    let focused = app.focus == Focus::Prompt;
    let rail_color = question_prompt_accent(theme);
    let measure = question_dock_measure(app, area.width, frame.area(), permission);
    let geometry = question_dock_geometry(area, &measure);
    let rail_area = geometry.rail;
    let body_area = Rect::new(
        rail_area.right(),
        area.y,
        area.width.saturating_sub(rail_area.width),
        area.height,
    );

    if body_area.width == 0 || body_area.height == 0 {
        return;
    }

    frame.render_widget(
        Block::default().style(Style::default().bg(surface)),
        body_area,
    );

    if rail_area.width > 0 && rail_area.height > 0 {
        let rail_style = Style::default().fg(rail_color).bg(surface);
        let rail_lines = (0..usize::from(rail_area.height))
            .map(|_| {
                Line::from(Span::styled(
                    theme.live_shell.transcript_glyphs.rail,
                    rail_style,
                ))
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(Text::from(rail_lines)).style(Style::default().bg(surface)),
            rail_area,
        );
    }

    if geometry.content.width == 0 || geometry.content.height == 0 {
        return;
    }

    let prompts = permission.question_prompts.as_deref().unwrap_or(&[]);
    let body = question_permission_body_text(app, permission, prompts, theme, surface, &measure);
    let source_chrome_end = usize::from(measure.source_chrome_rows).min(body.lines.len());
    let sticky_start = body
        .lines
        .len()
        .saturating_sub(usize::from(measure.sticky_rows));
    let option_end = source_chrome_end
        .saturating_add(usize::from(measure.option_rows))
        .min(sticky_start);
    let mut chrome_lines = body.lines[..source_chrome_end].to_vec();
    if chrome_lines.len() > usize::from(measure.chrome_rows) {
        chrome_lines.truncate(usize::from(measure.chrome_rows));
        if let Some(last) = chrome_lines.last_mut() {
            *last = Line::from(Span::styled(
                "... Ctrl-F to expand",
                Style::default()
                    .fg(theme.question_prompt.secondary)
                    .bg(surface),
            ));
        }
    }
    let chrome_body = Text::from(chrome_lines);
    let scroll_body = Text::from(body.lines[source_chrome_end..option_end].to_vec());
    let sticky_body = Text::from(body.lines[sticky_start..].to_vec());

    if geometry.chrome.height > 0 {
        frame.render_widget(Paragraph::new(chrome_body), geometry.chrome);
    }

    if geometry.options.height > 0 && geometry.options.width > 0 {
        if focused && !submission_pending {
            if let Some(selected_row) = question_selected_visual_area(
                geometry.options,
                measure.scroll_offset,
                measure.selected_range,
            ) {
                frame.render_widget(
                    Block::default().style(Style::default().bg(theme.question_prompt.selected)),
                    selected_row,
                );
            }
        }
        let hovered_visual_range = app
            .question_prompt_hovered(&permission.permission_id)
            .and_then(|hovered| {
                measure
                    .option_ranges
                    .iter()
                    .find(|range| range.index == hovered)
                    .map(|range| (range.start, range.end))
            });
        if let Some(hovered_row) = question_selected_visual_area(
            geometry.options,
            measure.scroll_offset,
            hovered_visual_range,
        ) {
            frame.render_widget(
                Block::default().style(Style::default().bg(theme.surface.hover)),
                hovered_row,
            );
        }
        frame.render_widget(
            Paragraph::new(scroll_body).scroll((measure.scroll_offset, 0)),
            geometry.options,
        );
        if let Some((scrollbar, thumb)) = geometry.scrollbar {
            render_question_scrollbar(frame, scrollbar, thumb, theme, surface);
        }
    }

    if geometry.sticky.height > 0 {
        render_question_sticky_background(frame, app, area, permission, theme, &geometry, &measure);
        frame.render_widget(Paragraph::new(sticky_body), geometry.sticky);
        if focused && !submission_pending {
            if let Some((row, column)) = measure.editor_cursor {
                if row < geometry.sticky.height {
                    frame.set_cursor_position((
                        geometry.sticky.x + 8 + column,
                        geometry.sticky.y + row,
                    ));
                }
            }
        }
    }

    if !focused {
        for content in [geometry.chrome, geometry.options, geometry.sticky] {
            dim_question_permission_dock(frame, content, theme);
        }
        if let Some((scrollbar, thumb)) = geometry.scrollbar {
            render_question_scrollbar(frame, scrollbar, thumb, theme, surface);
        }
    }
    if geometry.footer.width == 0 || geometry.footer.height == 0 {
        return;
    }
    let footer = if submission_pending {
        permission_modal_actions_text(app, theme, surface, true, true)
    } else {
        question_permission_actions_text(
            app,
            permission,
            prompts,
            theme,
            surface,
            geometry.footer.width,
        )
    };
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().bg(surface)),
        geometry.footer,
    );
}

fn render_question_sticky_background(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    permission: &ActivePermissionView,
    theme: &Theme,
    geometry: &crate::layout::QuestionDockGeometry,
    measure: &crate::layout::QuestionDockMeasure,
) {
    let prompts = permission.question_prompts.as_deref().unwrap_or(&[]);
    let editing = !measure.editor_lines.is_empty();
    let custom_height = if editing {
        u16::try_from(measure.editor_lines.len()).unwrap_or(u16::MAX)
    } else {
        1
    }
    .min(geometry.sticky.height);
    if editing {
        frame.render_widget(
            Block::default().style(Style::default().bg(theme.question_prompt.selected)),
            Rect::new(
                area.x.saturating_add(1),
                geometry.sticky.y,
                area.width.saturating_sub(1),
                custom_height,
            ),
        );
    }
    if app.focus == Focus::Prompt && question_custom_row_selected(app, permission, prompts) {
        frame.render_widget(
            Block::default().style(Style::default().bg(theme.question_prompt.selected)),
            Rect::new(
                geometry.sticky.x,
                geometry.sticky.y,
                geometry.sticky.width,
                1,
            ),
        );
    }
    if question_custom_row_hovered(app, permission, prompts) {
        frame.render_widget(
            Block::default().style(Style::default().bg(theme.surface.hover)),
            Rect::new(
                geometry.sticky.x,
                geometry.sticky.y,
                geometry.sticky.width,
                1,
            ),
        );
    }
}

fn dim_question_permission_dock(frame: &mut Frame, area: Rect, theme: &Theme) {
    let buffer = frame.buffer_mut();
    for position in area.positions() {
        let cell = &mut buffer[position];
        if cell.bg == theme.question_prompt.selected {
            cell.bg = theme.question_prompt.surface;
        }
        if cell.fg != Color::Reset {
            cell.fg = blend_color(
                cell.fg,
                theme.question_prompt.surface,
                1.0 - QUESTION_UNFOCUSED_BLEND,
            );
        }
    }
}

fn question_custom_row_selected(
    app: &AppState,
    permission: &ActivePermissionView,
    prompts: &[crate::app::QuestionPromptView],
) -> bool {
    let tab = app
        .question_prompt_tab(&permission.permission_id)
        .min(prompts.len());
    prompts.get(tab).is_some_and(|prompt| {
        prompt.custom
            && app.question_prompt_selection(&permission.permission_id) == prompt.options.len()
    })
}

fn question_custom_row_hovered(
    app: &AppState,
    permission: &ActivePermissionView,
    prompts: &[crate::app::QuestionPromptView],
) -> bool {
    let tab = app
        .question_prompt_tab(&permission.permission_id)
        .min(prompts.len());
    prompts.get(tab).is_some_and(|prompt| {
        prompt.custom
            && app.question_prompt_hovered(&permission.permission_id) == Some(prompt.options.len())
    })
}

fn render_question_scrollbar(
    frame: &mut Frame,
    scrollbar_area: Rect,
    thumb: Rect,
    theme: &Theme,
    surface: Color,
) {
    let lines = (scrollbar_area.y..scrollbar_area.bottom())
        .map(|row| {
            let symbol = if row >= thumb.y && row < thumb.bottom() {
                "█"
            } else {
                " "
            };
            let color = if symbol == "█" {
                theme.question_prompt.secondary
            } else {
                theme.terminal_colors.muted
            };
            let background = if symbol == "█" { color } else { surface };
            Line::from(Span::styled(
                symbol,
                Style::default().fg(color).bg(background),
            ))
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(Text::from(lines)), scrollbar_area);
}

fn question_selected_visual_area(
    content_area: Rect,
    scroll_y: u16,
    selected_visual_range: Option<(u16, u16)>,
) -> Option<Rect> {
    let (selected_top, selected_bottom) = selected_visual_range?;
    let visible_top = selected_top.saturating_sub(scroll_y);
    let visible_bottom = selected_bottom
        .saturating_sub(scroll_y)
        .min(content_area.height);
    if visible_top >= content_area.height || visible_bottom <= visible_top {
        return None;
    }
    Some(Rect::new(
        content_area.x,
        content_area.y.saturating_add(visible_top),
        content_area.width,
        visible_bottom.saturating_sub(visible_top),
    ))
}

pub(in crate::ui) const fn question_prompt_accent(theme: &Theme) -> Color {
    theme.question_prompt.accent
}

pub(in crate::ui) const fn question_prompt_secondary(theme: &Theme) -> Color {
    theme.question_prompt.secondary
}

fn permission_prompt_action_line(
    theme: &Theme,
    surface: Color,
    options: &[(&str, bool)],
) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, (label, selected)) in options.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(" ", Style::default().bg(surface)));
        }
        let marker = if *selected {
            theme.live_shell.transcript_glyphs.choice_selected
        } else {
            theme.live_shell.transcript_glyphs.choice_unselected
        };
        spans.push(Span::styled(
            format!(" {marker} {label} "),
            permission_prompt_option_style(theme, surface, *selected),
        ));
    }
    Line::from(spans)
}

fn render_permission_feedback(
    frame: &mut Frame,
    panel: Rect,
    options: Rect,
    app: &AppState,
    permission: &ActivePermissionView,
    theme: &Theme,
) {
    let Some(feedback) = app.permission_feedback_for_display(&permission.permission_id) else {
        return;
    };
    let selected = app.permission_modal_selection(&permission.permission_id)
        == PermissionModalSelection::Reject;
    let focused = app.focus == Focus::Prompt;
    let top = options.y.saturating_add(3);
    let height = options.bottom().saturating_sub(top);
    let editing = selected && feedback.editing;
    if height == 0 || (!editing && feedback.preview_text().trim().is_empty()) {
        return;
    }
    let marker = if selected {
        theme.live_shell.transcript_glyphs.choice_selected
    } else {
        theme.live_shell.transcript_glyphs.choice_unselected
    };
    let arrow = if theme.glyph_mode() == crate::theme::GlyphMode::Ascii {
        ">"
    } else {
        "❯"
    };
    let background = if selected {
        theme.question_prompt.selected
    } else {
        theme.question_prompt.surface
    };
    let accent = Style::default()
        .fg(question_prompt_accent(theme))
        .bg(background)
        .remove_modifier(Modifier::BOLD);
    let text = Style::default()
        .fg(theme.text.primary)
        .bg(background)
        .remove_modifier(Modifier::BOLD);
    let (rows, cursor) = if editing {
        feedback.editor_viewport(options.width.saturating_sub(6).max(1), height)
    } else {
        (
            vec![super::truncate_plain_text(feedback.preview_text(), 50)],
            (0, 0),
        )
    };
    for (index, row) in rows.into_iter().enumerate() {
        let offset = u16::try_from(index).unwrap_or(u16::MAX);
        if offset >= height {
            break;
        }
        let y = top.saturating_add(offset);
        let row_area = Rect::new(
            options.x,
            y,
            options.width.saturating_add(2 * u16::from(editing)),
            1,
        );
        // The numbered placeholder was painted first; remove its glyphs and
        // modifiers before overlaying a shorter or wrapped feedback line.
        frame.render_widget(Clear, row_area);
        frame.render_widget(Block::default().style(text), row_area);
        if editing {
            frame.render_widget(
                Block::default().style(Style::default().bg(background)),
                Rect::new(panel.x, y, panel.width, 1),
            );
            frame.render_widget(
                Block::default().style(Style::default().bg(theme.markdown.code_background)),
                Rect::new(panel.right().saturating_sub(1), y, 1, 1),
            );
        }
        let mut spans = if index == 0 {
            vec![
                Span::styled("4 ", accent),
                Span::styled(
                    format!("({marker}) "),
                    if selected {
                        text.add_modifier(Modifier::BOLD)
                    } else {
                        text.fg(theme.text.secondary)
                    },
                ),
                Span::styled(format!("{arrow} "), accent),
            ]
        } else {
            vec![Span::styled("        ", text)]
        };
        spans.push(Span::styled(row, text));
        frame.render_widget(Paragraph::new(Line::from(spans)), row_area);
    }
    if editing && focused && cursor.0 < height {
        frame.set_cursor_position((options.x + 8 + cursor.1, top + cursor.0));
    }
}

fn permission_prompt_numbered_options(
    theme: &Theme,
    surface: Color,
    options: &[(&str, bool)],
) -> Text<'static> {
    let lines = options
        .iter()
        .enumerate()
        .map(|(index, (label, selected))| {
            let marker = if *selected {
                theme.live_shell.transcript_glyphs.choice_selected
            } else {
                theme.live_shell.transcript_glyphs.choice_unselected
            };
            let style = permission_prompt_option_style(theme, surface, *selected);
            let marker_style = if *selected {
                style.add_modifier(Modifier::BOLD)
            } else {
                style.fg(theme.text.secondary)
            };
            let label_style = if index == options.len().saturating_sub(1) {
                style.fg(theme.text.secondary)
            } else if *selected {
                style.add_modifier(Modifier::BOLD)
            } else {
                style
            };
            Line::from(vec![
                Span::styled(
                    format!("{} ", index + 1),
                    style.fg(question_prompt_accent(theme)),
                ),
                Span::styled(format!("({marker}) "), marker_style),
                Span::styled((*label).to_owned(), label_style),
            ])
        })
        .collect::<Vec<_>>();
    Text::from(lines)
}

fn render_permission_numbered_options(
    frame: &mut Frame,
    area: Rect,
    options_text: Text<'static>,
    theme: &Theme,
    surface: Color,
    options: &[(&str, bool)],
) {
    for (index, (_, selected)) in options.iter().enumerate() {
        let row_offset = u16::try_from(index).unwrap_or(u16::MAX);
        let row_y = area.y.saturating_add(row_offset);
        if row_y >= area.bottom() {
            break;
        }
        frame.render_widget(
            Block::default().style(permission_prompt_option_style(theme, surface, *selected)),
            Rect::new(area.x, row_y, area.width, 1),
        );
    }

    frame.render_widget(Paragraph::new(options_text), area);
}

fn permission_prompt_option_style(theme: &Theme, surface: Color, selected: bool) -> Style {
    if selected {
        Style::default()
            .fg(theme.question_prompt.primary)
            .bg(theme.question_prompt.selected)
    } else {
        Style::default()
            .fg(theme.question_prompt.primary)
            .bg(surface)
    }
}

fn permission_prompt_hint_line(
    app: &AppState,
    theme: &Theme,
    surface: Color,
    selected_index: usize,
    option_count: usize,
    available_width: u16,
    expansion_label: Option<&str>,
) -> Line<'static> {
    use crate::keybindings::Action;

    let count = option_count.max(1);
    let selected = selected_index.clamp(1, count);
    let always = app.keymap.get_binding_str(Action::AlwaysApprovePermission);
    let always_label = if available_width <= 60 {
        if always == "-" {
            "Ctrl+o:always".to_string()
        } else {
            format!("{always}:always")
        }
    } else if always == "-" {
        "Ctrl+o:always-approve".to_string()
    } else {
        format!("{always}:always-approve")
    };
    let cancel = app.keymap.get_binding_str(Action::DismissModal);
    let cancel_label = if cancel == "-" {
        "Ctrl+c:cancel".to_string()
    } else {
        format!("{cancel}:cancel")
    };
    let primary_style = Style::default().fg(theme.text.primary).bg(surface);
    let separator_style = Style::default().fg(theme.text.secondary).bg(surface);
    let mut spans = vec![
        Span::styled(format!("{selected}/{count}:select"), primary_style),
        Span::styled("  │  ", separator_style),
        Span::styled(always_label, primary_style),
        Span::styled("  │  ", separator_style),
        Span::styled(cancel_label, primary_style),
    ];
    if let Some(label) = expansion_label {
        let expansion_width = Line::from(label).width();
        let base_width = Line::from(spans.clone()).width();
        if expansion_width.saturating_add(5).saturating_add(base_width)
            <= usize::from(available_width)
        {
            let mut expanded_spans = vec![
                Span::styled(label.to_owned(), primary_style),
                Span::styled("  │  ", separator_style),
            ];
            expanded_spans.append(&mut spans);
            spans = expanded_spans;
        }
    }
    Line::from(spans)
}

#[cfg(test)]
mod semantic_style_tests {
    use super::*;

    #[test]
    fn selected_permission_choice_preserves_its_card_tokens() {
        // arrange
        let theme = Theme::harness_dark();

        // When: a permission choice is selected.
        let style = permission_prompt_option_style(&theme, theme.question_prompt.surface, true);

        // act
        // Then: the row uses choice semantics, not slash-command selection colors.
        // assert
        assert_eq!(style.fg, Some(theme.question_prompt.primary));
        assert_eq!(style.bg, Some(theme.question_prompt.selected));
    }
}
