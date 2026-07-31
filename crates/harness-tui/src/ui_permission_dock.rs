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
    let dock_surface = theme.surface.card;
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
        let rail_style = Style::default().fg(theme.status.warning).bg(dock_surface);
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
            .map(|_| Line::from(Span::styled("┃", rail_style)))
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
        let hint_line = permission_prompt_hint_line(app, theme, tray_surface, confirm_index, 2);
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
    let selected_index = match selection {
        PermissionModalSelection::AllowAlways => 1usize,
        PermissionModalSelection::AllowSession => 2usize,
        PermissionModalSelection::AllowOnce => 3usize,
        PermissionModalSelection::Reject => 4usize,
    };
    let action_text = permission_prompt_numbered_options(theme, tray_surface, &options);
    let hint_line =
        permission_prompt_hint_line(app, theme, tray_surface, selected_index, options.len());
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
        frame.render_widget(
            Paragraph::new(action_text).style(Style::default().bg(tray_surface)),
            rows[0],
        );
        let hint_area = Rect {
            x: dock_area.x,
            y: rows[3].y,
            width: dock_area.width,
            height: 1,
        };
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
        frame.render_widget(
            Paragraph::new(action_text).style(Style::default().bg(tray_surface)),
            rows[0],
        );
        let hint_area = Rect {
            x: dock_area.x,
            y: rows[2].y,
            width: dock_area.width,
            height: 1,
        };
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
        frame.render_widget(
            Paragraph::new(action_text).style(Style::default().bg(tray_surface)),
            rows[0],
        );
        let hint_area = Rect {
            x: dock_area.x,
            y: rows[1].y,
            width: dock_area.width,
            height: 1,
        };
        if hint_area.width > 0 && hint_area.height > 0 {
            frame.render_widget(
                Paragraph::new(hint_line).style(Style::default().bg(tray_surface)),
                hint_area,
            );
        }
        return;
    }

    frame.render_widget(
        Paragraph::new(action_text).style(Style::default().bg(tray_surface)),
        tray_inner,
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
    let surface = theme.surface.card;
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
            .map(|_| Line::from(Span::styled("┃", rail_style)))
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(Text::from(rail_lines)).style(Style::default().bg(surface)),
            rail_area,
        );
    }

    // Reference question state: two spaces after ┃ ("┃  Which color?") + bottom blank rail.
    let inner = pad_rect(body_area, crate::layout::EdgeInsets::new(2, 1, 1, 1));
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let prompts = permission.question_prompts.as_deref().unwrap_or(&[]);
    let body = question_permission_body_text(app, permission, prompts, theme, surface);
    let body_lines = u16::try_from(body.lines.len())
        .unwrap_or(u16::MAX)
        .min(inner.height.saturating_sub(1));
    let gap_after_body: u16 = 1;
    let footer_height: u16 = 1;
    let used_height = body_lines
        .saturating_add(gap_after_body)
        .saturating_add(footer_height)
        .min(inner.height);
    let body_height = body_lines.min(used_height.saturating_sub(footer_height));

    if body_height > 0 && inner.width > 0 {
        let content_area = Rect::new(inner.x, inner.y, inner.width, body_height);
        frame.render_widget(
            Paragraph::new(body)
                .style(Style::default().bg(surface))
                .wrap(Wrap { trim: false }),
            content_area,
        );
    }

    let footer_y = inner
        .y
        .saturating_add(body_height)
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
    let selected_style = Style::default().fg(theme.text.primary).bg(surface);
    let unselected_style = Style::default().fg(theme.text.primary).bg(surface);
    let mut spans = Vec::new();
    for (index, (label, selected)) in options.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(" ", Style::default().bg(surface)));
        }
        let marker = if *selected { "●" } else { "○" };
        spans.push(Span::styled(
            format!(" {marker} {label} "),
            if *selected {
                selected_style
            } else {
                unselected_style
            },
        ));
    }
    Line::from(spans)
}

fn permission_prompt_numbered_options(
    theme: &Theme,
    surface: Color,
    options: &[(&str, bool)],
) -> Text<'static> {
    let selected_style = Style::default().fg(theme.text.primary).bg(surface);
    let unselected_style = Style::default().fg(theme.text.primary).bg(surface);
    let lines = options
        .iter()
        .enumerate()
        .map(|(index, (label, selected))| {
            let marker = if *selected { "●" } else { "○" };
            let style = if *selected {
                selected_style
            } else {
                unselected_style
            };
            Line::from(vec![Span::styled(
                format!("{} ({marker}) {label}", index + 1),
                style,
            )])
        })
        .collect::<Vec<_>>();
    Text::from(lines)
}

fn permission_prompt_hint_line(
    app: &AppState,
    theme: &Theme,
    surface: Color,
    selected_index: usize,
    option_count: usize,
) -> Line<'static> {
    use crate::keybindings::Action;

    let count = option_count.max(1);
    let selected = selected_index.clamp(1, count);
    let always = app.keymap.get_binding_str(Action::AlwaysApprovePermission);
    let always_label = if always == "-" {
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
