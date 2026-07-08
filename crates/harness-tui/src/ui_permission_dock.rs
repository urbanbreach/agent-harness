// allow: SIZE_OK — TUI rendering (indivisible view model)
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style},
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
    permission_modal_icon, permission_modal_metadata_line, permission_modal_subject_line,
    permission_modal_summary_line, permission_modal_title, question_permission_actions_text,
    question_permission_body_text,
};
use super::{display_width, COMPOSER_RAIL_GLYPH};

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
    let shell_surface = theme.surface.panel;
    let tray_surface = theme.surface.panel_elevated;
    let tray_height = dock_layout.tray_height;
    let sections = if area.height > tray_height {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(tray_height)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(0)])
            .split(area)
    };
    let shell_area = sections[0];
    let tray_area = sections[1];

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(dock_layout.rail_width),
            Constraint::Min(0),
        ])
        .split(area);
    let rail_area = columns[0];
    let body_area = columns[1];
    let body_sections = if body_area.height > tray_height {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(tray_height)])
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
        Block::default().style(Style::default().bg(shell_surface)),
        shell_body_area,
    );
    if tray_body_area.height > 0 {
        frame.render_widget(
            Block::default().style(Style::default().bg(tray_surface)),
            tray_body_area,
        );
    }

    if rail_area.width > 0 && rail_area.height > 0 {
        let rail_color = theme.status.warning;
        let shell_rows = usize::from(shell_area.height);
        let tray_rows = usize::from(tray_area.height);
        let mut lines = Vec::with_capacity(shell_rows.saturating_add(tray_rows).max(1));
        lines.extend(
            std::iter::repeat_with(|| {
                Line::from(Span::styled(
                    COMPOSER_RAIL_GLYPH,
                    Style::default().fg(rail_color).bg(shell_surface),
                ))
            })
            .take(shell_rows),
        );
        lines.extend(
            std::iter::repeat_with(|| {
                Line::from(Span::styled(
                    COMPOSER_RAIL_GLYPH,
                    Style::default().fg(rail_color).bg(tray_surface),
                ))
            })
            .take(tray_rows),
        );
        if lines.is_empty() {
            lines.push(Line::from(Span::styled(
                COMPOSER_RAIL_GLYPH,
                Style::default().fg(rail_color).bg(shell_surface),
            )));
        }
        frame.render_widget(Paragraph::new(lines), rail_area);
    }

    let shell_inner = pad_rect(shell_body_area, dock_layout.shell_padding);
    if shell_inner.width == 0 || shell_inner.height == 0 {
        return;
    }

    let header = if always_confirm {
        Text::from(vec![Line::from(vec![
            Span::styled(
                "△",
                Style::default().fg(theme.status.warning).bg(shell_surface),
            ),
            Span::styled(" ", Style::default().bg(shell_surface)),
            Span::styled(
                "Always allow",
                Style::default().fg(theme.text.primary).bg(shell_surface),
            ),
        ])])
    } else {
        let icon = permission_modal_icon(permission);
        let subject = permission_modal_subject_line(permission);
        Text::from(vec![
            Line::from(vec![
                Span::styled(
                    "△",
                    Style::default().fg(theme.status.warning).bg(shell_surface),
                ),
                Span::styled(" ", Style::default().bg(shell_surface)),
                Span::styled(
                    permission_modal_title(permission),
                    Style::default().fg(theme.text.primary).bg(shell_surface),
                ),
            ]),
            Line::from(vec![
                Span::styled("  ", Style::default().bg(shell_surface)),
                Span::styled(
                    icon,
                    Style::default().fg(theme.text.secondary).bg(shell_surface),
                ),
                Span::styled(" ", Style::default().bg(shell_surface)),
                Span::styled(
                    subject,
                    Style::default().fg(theme.text.primary).bg(shell_surface),
                ),
            ]),
        ])
    };
    let header_height = u16::try_from(header.lines.len())
        .unwrap_or(u16::MAX)
        .min(shell_inner.height);
    let body_gap = dock_layout
        .header_gap
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
            let mut lines = vec![
                Line::from(vec![Span::styled(
                    "This will allow this exact request until the harness is restarted.",
                    metadata_style,
                )]),
                Line::from(vec![Span::styled(
                    permission_modal_metadata_line(permission),
                    summary_style,
                )]),
            ];
            lines.push(Line::default());
            lines.push(Line::from(vec![Span::styled(
                "Selector listing:",
                metadata_style,
            )]));
            lines.push(Line::from(vec![
                Span::styled("  kind: ", metadata_style),
                Span::styled(permission.kind.as_str(), summary_style),
            ]));
            if let Some(tool_label) = permission.tool_label.as_deref() {
                lines.push(Line::from(vec![
                    Span::styled("  tool: ", metadata_style),
                    Span::styled(tool_label, summary_style),
                ]));
            }
            if let Some(tool_call_id) = permission.tool_call_id.as_deref() {
                lines.push(Line::from(vec![
                    Span::styled("  call: ", metadata_style),
                    Span::styled(tool_call_id, summary_style),
                ]));
            }
            lines.push(Line::from(vec![
                Span::styled("  summary: ", metadata_style),
                Span::styled(
                    truncate_permission_summary(&permission.summary, 60),
                    summary_style,
                ),
            ]));
            Text::from(lines)
        } else {
            let mut lines = vec![Line::from(vec![Span::styled(
                permission_modal_summary_line(permission, false),
                summary_style,
            )])];
            lines.push(Line::from(vec![Span::styled(
                permission_modal_metadata_line(permission),
                metadata_style,
            )]));
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
            ))
            .style(Style::default().bg(tray_surface))
            .wrap(Wrap { trim: true }),
            tray_inner,
        );
        return;
    }

    let action_line = if always_confirm {
        permission_prompt_action_line(
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
        )
    } else {
        let selection = app.permission_modal_selection(&permission.permission_id);
        permission_prompt_action_line(
            theme,
            tray_surface,
            &[
                (
                    "Allow once",
                    selection == PermissionModalSelection::AllowOnce,
                ),
                (
                    "Allow always",
                    selection == PermissionModalSelection::AllowAlways,
                ),
                ("Reject", selection == PermissionModalSelection::Reject),
            ],
        )
    };
    let hint_line = permission_prompt_hint_line(theme, tray_surface);
    let hint_width = u16::try_from(display_width("⇆ select  enter confirm")).unwrap_or(u16::MAX);
    let narrow = area.width < dock_layout.stacked_hint_min_width
        || tray_inner.width <= hint_width.saturating_add(dock_layout.stacked_hint_min_action_width);

    if narrow && tray_inner.height > 1 {
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

    let footer_columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(hint_width.min(tray_inner.width)),
        ])
        .split(tray_inner);
    frame.render_widget(
        Paragraph::new(action_line).style(Style::default().bg(tray_surface)),
        footer_columns[0],
    );
    if footer_columns[1].width > 0 {
        frame.render_widget(
            Paragraph::new(hint_line)
                .style(Style::default().bg(tray_surface))
                .alignment(Alignment::Right),
            footer_columns[1],
        );
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
    let surface = theme.surface.panel;
    let rail_color = question_prompt_accent(theme);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
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
        let lines = std::iter::repeat_with(|| {
            Line::from(Span::styled(
                COMPOSER_RAIL_GLYPH,
                Style::default().fg(rail_color).bg(surface),
            ))
        })
        .take(usize::from(rail_area.height))
        .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(lines), rail_area);
    }

    let inner = pad_rect(body_area, crate::layout::EdgeInsets::new(1, 3, 1, 1));
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let content_height = inner.height.saturating_sub(2);
    let content_width = inner.width.saturating_sub(1);
    if content_height > 0 && content_width > 0 {
        let content_area = Rect::new(
            inner.x.saturating_add(1),
            inner.y,
            content_width,
            content_height,
        );
        let prompts = permission.question_prompts.as_deref().unwrap_or(&[]);
        let body = question_permission_body_text(app, permission, prompts, theme, surface);
        frame.render_widget(
            Paragraph::new(body)
                .style(Style::default().bg(surface))
                .wrap(Wrap { trim: false }),
            content_area,
        );
    }

    let footer_width = inner.width.saturating_sub(1);
    if footer_width == 0 || inner.height == 0 {
        return;
    }

    let footer_area = Rect::new(
        inner.x.saturating_add(1),
        inner.y.saturating_add(inner.height.saturating_sub(1)),
        footer_width,
        1,
    );
    let footer = if submission_pending {
        permission_modal_actions_text(app, theme, surface, true, true)
    } else {
        question_permission_actions_text(
            app,
            permission,
            permission.question_prompts.as_deref().unwrap_or(&[]),
            theme,
            surface,
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
    let selected_style = Style::default()
        .fg(theme.text.inverse)
        .bg(theme.status.warning);
    let unselected_style = Style::default().fg(theme.text.secondary).bg(surface);
    let mut spans = Vec::new();
    for (index, (label, selected)) in options.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(" ", Style::default().bg(surface)));
        }
        spans.push(Span::styled(
            format!(" {label} "),
            if *selected {
                selected_style
            } else {
                unselected_style
            },
        ));
    }
    Line::from(spans)
}

fn permission_prompt_hint_line(theme: &Theme, surface: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled("⇆", Style::default().fg(theme.text.primary).bg(surface)),
        Span::styled(
            " select  ",
            Style::default().fg(theme.text.secondary).bg(surface),
        ),
        Span::styled("enter", Style::default().fg(theme.text.primary).bg(surface)),
        Span::styled(
            " confirm",
            Style::default().fg(theme.text.secondary).bg(surface),
        ),
    ])
}

fn truncate_permission_summary(summary: &str, max_chars: usize) -> String {
    let trimmed = summary.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let truncated: String = trimmed.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{truncated}…")
}
