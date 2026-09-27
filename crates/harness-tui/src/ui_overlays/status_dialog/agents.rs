use super::*;
use crate::ui::{dashboard_preview_frame, ui_overlays::permission_modal};

pub(super) fn render_dashboard_roster(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    area: Rect,
    dashboard: &crate::dashboard_integration::DashboardIntegration,
) {
    use crate::dashboard_roster::RosterItem;
    let layout = dashboard.roster_layout();
    let parents = dashboard
        .dashboard()
        .rows
        .iter()
        .filter(|row| !row.relationship.is_child);
    let (mut total, mut working, mut awaiting) = (0, 0, 0);
    for row in parents {
        total += 1;
        if row.status == crate::dashboard::DashboardStatus::AwaitingInput {
            awaiting += 1;
            continue;
        }
        working += usize::from(matches!(
            row.status,
            crate::dashboard::DashboardStatus::Running
                | crate::dashboard::DashboardStatus::Streaming
        ));
    }
    let header = Rect::new(
        area.x.saturating_add(2),
        area.y.saturating_sub(2),
        area.width.saturating_sub(4),
        1,
    );
    let pending_count = if awaiting > 0 {
        format!(" · {awaiting} needs input")
    } else {
        String::new()
    };
    frame.render_widget(
        Paragraph::new(format!(
            "{total} {} · {working} working{pending_count}",
            if total == 1 { "agent" } else { "agents" }
        ))
        .style(Style::default().fg(theme.text.secondary)),
        header,
    );
    for item in layout.items {
        match item {
            RosterItem::Group(group) => frame.render_widget(
                Paragraph::new(group.label).style(Style::default().fg(theme.text.secondary)),
                group.rect,
            ),
            RosterItem::Overflow(overflow) => frame.render_widget(
                Paragraph::new(overflow.label).style(Style::default().fg(theme.text.secondary)),
                overflow.rect,
            ),
            RosterItem::Row(row) => {
                let data = dashboard.dashboard().row(row.selection_key.as_str());
                let pending = app.run_id() == Some(row.selection_key.as_str())
                    && app.active_permission_view().is_some();
                let status = if pending {
                    "needs input"
                } else {
                    data.map_or("unavailable", |entry| dashboard_status_label(entry.status))
                };
                let color = if pending {
                    theme.status.warning
                } else {
                    match row.marker.status {
                        crate::dashboard::DashboardStatus::Failed => theme.status.error,
                        crate::dashboard::DashboardStatus::Completed => theme.status.success,
                        _ => theme.text.secondary,
                    }
                };
                let marker = if !row.selected {
                    " "
                } else if theme.glyph_mode() == crate::theme::GlyphMode::Ascii {
                    "|"
                } else {
                    "│"
                };
                let indent = " ".repeat(usize::from(row.indent));
                let subtitle = if pending {
                    app.active_permission_view()
                        .map(dashboard_permission_subject)
                        .unwrap_or_default()
                } else {
                    format!(
                        "{}{}",
                        if row.pinned { "pinned · " } else { "" },
                        row.selection_key.as_str()
                    )
                };
                let title = Line::from(vec![
                    Span::styled(
                        format!("{marker} {indent}"),
                        Style::default().fg(theme.text.accent),
                    ),
                    Span::styled(
                        format!(
                            "{} ",
                            if theme.glyph_mode() == crate::theme::GlyphMode::Ascii {
                                row.marker.ascii
                            } else {
                                row.marker.preferred
                            }
                        ),
                        Style::default().fg(color),
                    ),
                    Span::styled(
                        row.label,
                        Style::default()
                            .fg(theme.text.primary)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!("  [{status}]"), Style::default().fg(color)),
                ]);
                let secondary = Line::from(vec![
                    Span::styled(
                        format!("{marker} {indent}  "),
                        Style::default().fg(theme.text.accent),
                    ),
                    Span::styled(
                        truncate_plain_text(
                            &subtitle,
                            usize::from(row.rect.width.saturating_sub(row.indent + 4)),
                        ),
                        Style::default().fg(color),
                    ),
                ]);
                frame.render_widget(Paragraph::new(vec![title, secondary]), row.rect);
            }
        }
    }
}

fn dashboard_permission_subject(permission: crate::app::ActivePermissionView) -> String {
    permission
        .question_prompts
        .as_ref()
        .and_then(|prompts| prompts.first())
        .map(|prompt| prompt.question.clone())
        .unwrap_or_else(|| permission_modal::permission_modal_subject_line(&permission))
}

pub(super) fn render_dashboard_peek(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    area: Rect,
    dashboard: &crate::dashboard_integration::DashboardIntegration,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let title = dashboard
        .peek_view()
        .ok()
        .and_then(|view| dashboard.dashboard().row(view.session_id.as_str()))
        .and_then(|row| row.title.as_deref())
        .unwrap_or("Agent preview");
    let panel = Block::default()
        .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
        .border_style(Style::default().fg(theme.terminal_colors.muted))
        .title(title);
    let inner = panel.inner(area);
    frame.render_widget(panel, area);
    let selected_current = dashboard
        .roster_state()
        .selected_key()
        .map(|key| key.as_str())
        == app.run_id();
    if let Some(permission) = app.active_permission_view().filter(|_| selected_current) {
        let body = if let Some(prompts) = &permission.question_prompts {
            let measure = crate::layout::question_content_measure(
                app,
                inner.width,
                frame.area().height,
                &permission,
            );
            permission_modal::question_permission_body_text(
                app,
                &permission,
                prompts,
                theme,
                theme.surface.canvas,
                &measure,
            )
        } else {
            let mut rows = vec![
                Line::from(permission_modal::permission_modal_title(&permission)),
                Line::from(permission_modal::permission_modal_subject_line(&permission)),
            ];
            if app.permission_submission_pending(&permission.permission_id) {
                rows.push(Line::from("Decision submitted · awaiting confirmation"));
            } else {
                let selected = app
                    .permission_modal_selection(&permission.permission_id)
                    .number();
                for (index, label) in [
                    "Enable always-approve mode",
                    "Remember this approval for this session",
                    "Allow once",
                    "Reject and add feedback",
                ]
                .into_iter()
                .enumerate()
                {
                    rows.push(dashboard_permission_option(
                        theme,
                        index + 1,
                        label,
                        selected,
                    ));
                }
                if let Some(feedback) = app.permission_feedback(&permission.permission_id) {
                    let (before, after) =
                        feedback.visible_parts(usize::from(inner.width).saturating_sub(10));
                    rows.push(Line::from(format!("Feedback: {before}{after}")));
                }
            }
            Text::from(rows)
        };
        frame.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }), inner);
        return;
    }
    let mut lines = dashboard.peek_view().map_or_else(
        |_| Vec::new(),
        |view| {
            dashboard_preview_frame(
                &view.blocks,
                inner.width,
                inner.height,
                peek_scroll_row(view.scroll_top),
                theme,
            )
        },
    );
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "No recorded output yet",
            Style::default().fg(theme.text.secondary),
        )));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn dashboard_permission_option(
    theme: &Theme,
    number: usize,
    label: &str,
    selected: usize,
) -> Line<'static> {
    let marker = if selected == number { "❯" } else { " " };
    let color = if selected == number {
        theme.text.accent
    } else {
        theme.text.primary
    };
    Line::from(Span::styled(
        format!("{marker} {number} {label}"),
        Style::default().fg(color),
    ))
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "scroll rows are finite nonnegative values; the saturating float conversion bounds oversized offsets"
)]
fn peek_scroll_row(value: f64) -> usize {
    if value.is_finite() && value > 0.0 {
        value.floor() as usize
    } else {
        0
    }
}
