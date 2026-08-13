// allow: SIZE_OK — TUI rendering (indivisible view model)
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Paragraph, Wrap},
    Frame,
};

use crate::app::permissions::{
    PermissionConfirmSelection, PermissionModalSelection, PermissionModalStage,
};
use crate::app::{ActivePermissionView, AppState};
use crate::layout::{pad_rect, permission_dock_layout};
use crate::theme::Theme;

use super::super::ui_overlays::{
    permission_modal_actions_text, permission_modal_draft_line, permission_modal_guidance,
    permission_modal_metadata_line, permission_modal_subject_line, permission_modal_summary_line,
    permission_modal_title, question_permission_actions_text, question_permission_body_text,
};
use super::display_width;

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

    let dock_layout = permission_dock_layout(area, is_question);
    let always_confirm = !is_question
        && app.permission_modal_stage(&permission.permission_id)
            == PermissionModalStage::AlwaysConfirm;
    let dock_surface = theme.surface.panel_elevated;
    let shell_surface = dock_surface;
    let tray_surface = dock_surface;
    let tray_height = dock_layout.tray_height;
    // Freeze PERM packs shell as title + gap + body (3 rows). A non-empty draft
    // replaces the freeze blank (gap→0) so draft stays visible without +1 dock height.
    let has_draft = !app.composer.prompt_buffer.trim().is_empty();
    let shell_content_height = 3u16.min(area.height.saturating_sub(tray_height).max(1));
    let used_height = shell_content_height
        .saturating_add(tray_height)
        .min(area.height);
    let dock_top = area
        .y
        .saturating_add(area.height.saturating_sub(used_height));
    let dock_area = Rect {
        x: area.x,
        y: dock_top,
        width: area.width,
        height: used_height,
    };

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(dock_layout.rail_width),
            Constraint::Min(0),
        ])
        .split(dock_area);
    let rail_area = columns[0];
    let body_area = columns[1];
    let body_sections = if body_area.height > tray_height {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(
                    shell_content_height.min(body_area.height.saturating_sub(tray_height)),
                ),
                Constraint::Length(tray_height),
            ])
            .split(body_area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(0)])
            .split(body_area)
    };
    let shell_body_area = body_sections[0];
    let tray_body_area = body_sections[1];

    frame.render_widget(
        Block::default().style(Style::default().bg(dock_surface)),
        body_area,
    );

    if rail_area.width > 0 && rail_area.height > 0 {
        let rail_style = Style::default().fg(theme.text.accent).bg(dock_surface);
        let option_rows: u16 = if always_confirm { 2 } else { 4 };
        let post_option_blank: u16 = 1;
        let rail_paint_height = shell_body_area
            .height
            .saturating_add(option_rows)
            .saturating_add(post_option_blank)
            .min(rail_area.height.saturating_sub(1).max(1));
        let rail_paint = Rect {
            x: rail_area.x,
            y: rail_area.y,
            width: rail_area.width,
            height: rail_paint_height,
        };
        let rail_lines = (0..usize::from(rail_paint.height))
            .map(|_| {
                Line::from(Span::styled(
                    theme.live_shell.transcript_glyphs.rail,
                    rail_style,
                ))
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(Text::from(rail_lines)).style(Style::default().bg(dock_surface)),
            rail_paint,
        );
    }

    let shell_inner = pad_rect(shell_body_area, dock_layout.shell_padding);
    if shell_inner.width == 0 || shell_inner.height == 0 {
        return;
    }

    let header = if always_confirm {
        Text::from(vec![Line::from(vec![Span::styled(
            "Always allow",
            Style::default().fg(theme.text.primary).bg(shell_surface),
        )])])
    } else {
        Text::from(vec![Line::from(vec![Span::styled(
            permission_modal_title(permission),
            Style::default()
                .fg(theme.text.primary)
                .bg(shell_surface)
                .add_modifier(Modifier::BOLD),
        )])])
    };
    let header_height = u16::try_from(header.lines.len())
        .unwrap_or(u16::MAX)
        .min(shell_inner.height);
    // Empty draft: freeze blank gap between title and options. Non-empty draft: gap
    // collapses so the body slot holds the draft line inside height 3.
    let body_gap = if has_draft { 0 } else { dock_layout.header_gap }
        .min(shell_inner.height.saturating_sub(header_height));
    let shell_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Length(body_gap),
            Constraint::Min(0),
        ])
        .split(shell_inner);
    frame.render_widget(Paragraph::new(header), shell_rows[0]);

    if shell_rows[2].width > 0 && shell_rows[2].height > 0 {
        let metadata_style = Style::default().fg(theme.text.secondary).bg(shell_surface);
        let summary_style = Style::default().fg(theme.text.primary).bg(shell_surface);
        let guidance_style = Style::default().fg(theme.text.secondary).bg(shell_surface);
        let body = if submission_pending {
            Text::from(vec![
                Line::from(vec![Span::styled(
                    permission_modal_summary_line(permission, submission_pending),
                    summary_style,
                )]),
                Line::from(vec![Span::styled(
                    permission_modal_guidance(permission, submission_pending),
                    metadata_style,
                )]),
            ])
        } else if always_confirm {
            let mut lines = vec![Line::from(vec![Span::styled(
                "This will allow this exact request until the harness is restarted.",
                metadata_style,
            )])];
            let metadata = permission_modal_metadata_line(permission);
            if !metadata.is_empty() {
                lines.push(Line::from(vec![Span::styled(metadata, summary_style)]));
            }
            Text::from(lines)
        } else {
            let mut lines = Vec::new();
            let subject = permission_modal_subject_line(permission);
            if !subject.is_empty() {
                lines.push(Line::from(vec![Span::styled(subject, summary_style)]));
            }
            let metadata = permission_modal_metadata_line(permission);
            if !metadata.is_empty() {
                lines.push(Line::from(vec![Span::styled(metadata, metadata_style)]));
            }
            let draft = permission_modal_draft_line(app.composer.prompt_buffer.as_str());
            if !draft.is_empty() {
                lines.push(Line::from(vec![Span::styled(draft, guidance_style)]));
            }
            Text::from(lines)
        };

        frame.render_widget(
            Paragraph::new(body)
                .style(Style::default().bg(shell_surface))
                .wrap(Wrap { trim: true }),
            pad_rect(shell_rows[2], dock_layout.body_padding),
        );
    }

    if tray_body_area.width == 0 || tray_body_area.height == 0 {
        return;
    }

    let tray_inner = pad_rect(tray_body_area, dock_layout.tray_padding);
    if tray_inner.width == 0 || tray_inner.height == 0 {
        return;
    }

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

    if is_question {
        frame.render_widget(
            Paragraph::new(question_permission_actions_text(
                app,
                permission,
                permission.question_prompts.as_deref().unwrap_or(&[]),
                theme,
                tray_surface,
                tray_inner.width,
            ))
            .style(Style::default().bg(tray_surface))
            .wrap(Wrap { trim: false }),
            tray_inner,
        );
        return;
    }

    if always_confirm {
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
    let options = if tray_inner.width <= 60 {
        [
            (
                "Yes, always approve",
                selection == PermissionModalSelection::AllowAlways,
            ),
            (
                "Yes, allow edits this session",
                selection == PermissionModalSelection::AllowSession,
            ),
            (
                "Yes, once",
                selection == PermissionModalSelection::AllowOnce,
            ),
            (
                "No, reject and add feedback",
                selection == PermissionModalSelection::Reject,
            ),
        ]
    } else {
        [
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
        ]
    };
    let selected_index = match selection {
        PermissionModalSelection::AllowAlways => 1usize,
        PermissionModalSelection::AllowSession => 2usize,
        PermissionModalSelection::AllowOnce => 3usize,
        PermissionModalSelection::Reject => 4usize,
    };
    let action_text = permission_prompt_numbered_options(theme, tray_surface, &options);
    let hint_line = permission_prompt_hint_line(
        app,
        theme,
        tray_surface,
        selected_index,
        options.len(),
        tray_inner.width,
    );
    let option_rows = u16::try_from(options.len()).unwrap_or(u16::MAX);
    // Freeze tray: options, post blank, empty, hints, trailing blank (height 8).
    if tray_inner.height >= option_rows.saturating_add(4) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(option_rows),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(tray_inner);
        render_permission_numbered_options(
            frame,
            rows[0],
            action_text,
            theme,
            tray_surface,
            &options,
        );
        let hint_area = permission_hint_area(rows[3]);
        if hint_area.width > 0 && hint_area.height > 0 {
            frame.render_widget(
                Paragraph::new(hint_line).style(Style::default().bg(tray_surface)),
                hint_area,
            );
        }
        return;
    }
    if tray_inner.height >= option_rows.saturating_add(2) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(option_rows),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(tray_inner);
        render_permission_numbered_options(
            frame,
            rows[0],
            action_text,
            theme,
            tray_surface,
            &options,
        );
        let hint_area = permission_hint_area(rows[2]);
        if hint_area.width > 0 && hint_area.height > 0 {
            frame.render_widget(
                Paragraph::new(hint_line).style(Style::default().bg(tray_surface)),
                hint_area,
            );
        }
        return;
    }
    if tray_inner.height > option_rows {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(option_rows), Constraint::Length(1)])
            .split(tray_inner);
        render_permission_numbered_options(
            frame,
            rows[0],
            action_text,
            theme,
            tray_surface,
            &options,
        );
        let hint_area = permission_hint_area(rows[1]);
        if hint_area.width > 0 && hint_area.height > 0 {
            frame.render_widget(
                Paragraph::new(hint_line).style(Style::default().bg(tray_surface)),
                hint_area,
            );
        }
        return;
    }

    render_permission_numbered_options(
        frame,
        tray_inner,
        action_text,
        theme,
        tray_surface,
        &options,
    );
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
    let rail_color = question_prompt_accent(theme);
    let dock_layout = permission_dock_layout(area, true);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(dock_layout.rail_width.max(1)),
            Constraint::Min(0),
        ])
        .split(area);
    let rail_area = columns[0];
    let body_area = columns[1];

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

    // Reference question state: two spaces after ┃ ("┃  Which color?") + bottom blank rail.
    let inner = pad_rect(body_area, dock_layout.question_content_padding);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let prompts = permission.question_prompts.as_deref().unwrap_or(&[]);
    let body = question_permission_body_text(app, permission, prompts, theme, surface, inner.width);
    let sticky_custom_index = body.lines.iter().position(|line| {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
            .starts_with("z (")
    });
    let selected_visual_range = body
        .lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.style.bg == Some(theme.question_prompt.selected))
        .map(|(index, _)| u16::try_from(index).unwrap_or(u16::MAX))
        .fold(None, |range, row| match range {
            Some((start, _)) => Some((start, row.saturating_add(1))),
            None => Some((row, row.saturating_add(1))),
        });
    let sticky_custom_line = sticky_custom_index.and_then(|index| body.lines.get(index).cloned());
    let scroll_body = if let Some(sticky_index) = sticky_custom_index {
        Text::from(
            body.lines
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != sticky_index)
                .map(|(_, line)| line.clone())
                .collect::<Vec<_>>(),
        )
    } else {
        body
    };
    let sticky_height = u16::from(sticky_custom_line.is_some());
    let total_body_lines = u16::try_from(scroll_body.lines.len()).unwrap_or(u16::MAX);
    let gap_after_body = dock_layout.question_chrome_gap;
    let footer_height = dock_layout.question_footer_height;
    let body_capacity = inner
        .height
        .saturating_sub(sticky_height)
        .saturating_sub(gap_after_body)
        .saturating_sub(footer_height);
    let body_lines = total_body_lines.min(body_capacity);
    let used_height = body_lines
        .saturating_add(sticky_height)
        .saturating_add(gap_after_body)
        .saturating_add(footer_height)
        .min(inner.height);
    let body_height = body_lines.min(
        used_height
            .saturating_sub(sticky_height)
            .saturating_sub(gap_after_body)
            .saturating_sub(footer_height),
    );

    if body_height > 0 && inner.width > 0 {
        let content_area = Rect::new(inner.x, inner.y, inner.width, body_height);
        let scroll_y = question_scroll_offset(
            selected_visual_range,
            total_body_lines,
            body_height,
            question_custom_row_selected(app, permission, prompts),
        );
        if !submission_pending {
            if let Some(selected_row) =
                question_selected_visual_area(content_area, scroll_y, selected_visual_range)
            {
                frame.render_widget(
                    Block::default().style(Style::default().bg(theme.question_prompt.selected)),
                    selected_row,
                );
            }
        }
        frame.render_widget(
            Paragraph::new(scroll_body).scroll((scroll_y, 0)),
            content_area,
        );
        render_question_scrollbar(
            frame,
            body_area,
            content_area,
            total_body_lines,
            scroll_y,
            theme,
            surface,
        );
    }

    if let Some(sticky_line) = sticky_custom_line {
        let sticky_area = Rect::new(
            inner.x,
            inner.y.saturating_add(body_height),
            inner.width,
            sticky_height,
        );
        if question_custom_row_selected(app, permission, prompts) {
            frame.render_widget(
                Block::default().style(Style::default().bg(theme.question_prompt.selected)),
                sticky_area,
            );
        }
        frame.render_widget(Paragraph::new(sticky_line), sticky_area);
    }

    let footer_y = inner
        .y
        .saturating_add(body_height)
        .saturating_add(sticky_height)
        .saturating_add(gap_after_body.min(inner.height.saturating_sub(body_height)));
    let footer_area = Rect::new(
        inner.x,
        footer_y.min(inner.y.saturating_add(inner.height.saturating_sub(1))),
        inner.width,
        footer_height.min(inner.height.saturating_sub(body_height).max(1)),
    );
    if footer_area.width == 0 || footer_area.height == 0 {
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
            footer_area.width,
        )
    };
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().bg(surface)),
        footer_area,
    );
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

fn render_question_scrollbar(
    frame: &mut Frame,
    body_area: Rect,
    content_area: Rect,
    total_lines: u16,
    scroll_y: u16,
    theme: &Theme,
    surface: Color,
) {
    let visible_lines = content_area.height;
    if total_lines <= visible_lines || visible_lines == 0 || body_area.width == 0 {
        return;
    }
    let thumb_height = visible_lines
        .saturating_mul(visible_lines)
        .checked_div(total_lines)
        .unwrap_or(1)
        .max(1)
        .min(visible_lines);
    let scroll_range = total_lines.saturating_sub(visible_lines);
    let thumb_range = visible_lines.saturating_sub(thumb_height);
    let thumb_top = scroll_y
        .min(scroll_range)
        .saturating_mul(thumb_range)
        .checked_div(scroll_range.max(1))
        .unwrap_or(0);
    let lines = (0..visible_lines)
        .map(|row| {
            let symbol = if row >= thumb_top && row < thumb_top.saturating_add(thumb_height) {
                "█"
            } else {
                " "
            };
            let color = if symbol == "█" {
                theme.scrollbar.thumb
            } else {
                theme.scrollbar.track
            };
            Line::from(Span::styled(symbol, Style::default().fg(color).bg(surface)))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(Text::from(lines)),
        Rect::new(
            body_area.right().saturating_sub(1),
            content_area.y,
            1,
            visible_lines,
        ),
    );
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

fn question_scroll_offset(
    selected_visual_range: Option<(u16, u16)>,
    total_height: u16,
    visible_height: u16,
    custom_selected: bool,
) -> u16 {
    if visible_height == 0 {
        return 0;
    }
    let max_scroll = total_height.saturating_sub(visible_height);
    if custom_selected {
        return max_scroll;
    }
    selected_visual_range
        .map(|(_, bottom)| bottom.saturating_sub(visible_height).min(max_scroll))
        .unwrap_or(0)
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
            Line::from(vec![Span::styled(
                format!("{} ({marker}) {label}", index + 1),
                style,
            )])
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

fn permission_hint_area(row: Rect) -> Rect {
    Rect::new(
        row.x.saturating_add(2),
        row.y,
        row.width.saturating_sub(2),
        row.height,
    )
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
    Line::from(vec![
        Span::styled(
            format!("{selected}/{count}:select"),
            Style::default().fg(theme.text.primary).bg(surface),
        ),
        Span::styled(
            "  │  ",
            Style::default().fg(theme.text.secondary).bg(surface),
        ),
        Span::styled(
            always_label,
            Style::default().fg(theme.text.primary).bg(surface),
        ),
        Span::styled(
            "  │  ",
            Style::default().fg(theme.text.secondary).bg(surface),
        ),
        Span::styled(
            cancel_label,
            Style::default().fg(theme.text.primary).bg(surface),
        ),
    ])
}

#[cfg(test)]
mod semantic_style_tests {
    use super::*;

    #[test]
    fn selected_permission_choice_uses_question_choice_tokens() {
        // Given: the reference-backed Harness chat theme.
        let theme = Theme::harness_chat();

        // When: a permission choice is selected.
        let style = permission_prompt_option_style(&theme, theme.question_prompt.surface, true);

        // Then: the row uses choice semantics, not slash-command selection colors.
        assert_eq!(style.fg, Some(theme.question_prompt.primary));
        assert_eq!(style.bg, Some(theme.question_prompt.selected));
    }
}
