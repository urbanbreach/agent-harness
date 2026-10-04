use ratatui::{
    layout::{Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use unicode_width::UnicodeWidthStr;

use crate::{
    app::{ActivityStatus, AppState},
    layout::{subagent, FrameLayoutPlan},
    theme::Theme,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubagentFrameTarget {
    Close,
}

pub(crate) fn subagent_frame_target_at(
    app: &AppState,
    area: Rect,
    column: u16,
    row: u16,
) -> Option<SubagentFrameTarget> {
    if !app.current_subagent_session_present() || app.review_surface().is_some() {
        return None;
    }
    subagent::close(area)?
        .contains(Position::new(column, row))
        .then_some(SubagentFrameTarget::Close)
}

pub(super) fn render(frame: &mut Frame, app: &AppState, plan: &FrameLayoutPlan, theme: &Theme) {
    frame
        .buffer_mut()
        .set_style(plan.root, Style::default().fg(theme.text.primary));
    let Some(area) = subagent::frame(plan.root) else {
        return;
    };
    let style = Style::default()
        .fg(crate::theme::quantize_color(
            ratatui::style::Color::Rgb(60, 60, 65),
            theme.color_level(),
        ))
        .bg(theme.surface.canvas);
    frame.render_widget(
        Block::default()
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .border_style(style),
        Rect::new(area.x, area.y, area.width, 2),
    );
    let content = Rect::new(area.x, area.y + 2, area.width, area.height - 2);
    frame.render_widget(
        Block::default().borders(Borders::ALL).border_style(style),
        content,
    );
    for (x, glyph) in [(area.x, "├"), (area.right() - 1, "┤")] {
        frame.buffer_mut()[(x, content.y)]
            .set_symbol(glyph)
            .set_style(style);
    }
    let Some(header) = app.subagent_header() else {
        return;
    };
    frame.buffer_mut().set_style(
        plan.shell,
        Style::default().fg(ratatui::style::Color::Reset),
    );
    render_title(frame, app, area, &header, theme);
    let cwd = crate::text::collapse_inline_whitespace(&header.cwd);
    let cwd_width = u16::try_from(cwd.width())
        .unwrap_or(u16::MAX)
        .min(plan.header.width);
    frame.render_widget(
        Paragraph::new(cwd).style(Style::default().fg(theme.markdown.text)),
        Rect {
            width: cwd_width,
            ..plan.header
        },
    );
    render_link_preview(frame, app, plan.header, theme);
    if let Some(transcript) = plan.transcript {
        super::render_transcript_pane(frame, app, transcript, theme);
    }
    if let Some(index) = app.transcript_view.highlighted_link {
        for (_, link) in app
            .transcript_view
            .hyperlinks
            .iter()
            .skip(index)
            .enumerate()
            .take_while(|(offset, link)| *offset == 0 || link.continues_previous)
        {
            frame.buffer_mut().set_style(
                Rect::new(
                    link.start_column,
                    link.row,
                    link.end_column.saturating_sub(link.start_column),
                    1,
                ),
                Style::default()
                    .fg(theme.markdown.link)
                    .add_modifier(Modifier::UNDERLINED | Modifier::BOLD),
            );
        }
    }
    if let Some(status) = plan.status.filter(|_| {
        matches!(
            header.status,
            ActivityStatus::Queued | ActivityStatus::Streaming
        )
    }) {
        render_status(frame, app, status, &header, theme);
    }
    let mut shortcuts = child_shortcuts(app);
    shortcuts.truncate(5);
    shortcuts.push((
        if app.shortcuts_ctrl_dot {
            "Ctrl+."
        } else {
            "Ctrl+x"
        },
        "shortcuts",
    ));
    let mut spans = Vec::new();
    for (index, (key, label)) in shortcuts.into_iter().enumerate() {
        let used: usize = spans.iter().map(Span::width).sum();
        if used + usize::from(index > 0) * 5 + key.width() + 1 > usize::from(plan.footer.width) {
            break;
        }
        if index > 0 {
            spans.push(Span::styled(
                "  │  ",
                Style::default()
                    .fg(theme.text.secondary)
                    .add_modifier(Modifier::DIM),
            ));
        }
        for (part_index, part) in key.split('/').enumerate() {
            if part_index > 0 {
                spans.push(Span::styled("/", Style::default().fg(theme.text.secondary)));
            }
            spans.push(Span::styled(
                part,
                Style::default()
                    .fg(theme.markdown.text)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        let used: usize = spans.iter().map(Span::width).sum();
        let label = if used + label.width() < usize::from(plan.footer.width) {
            label
        } else {
            ""
        };
        spans.push(Span::styled(
            format!(":{label}"),
            Style::default().fg(theme.text.secondary),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().fg(theme.text.secondary)),
        plan.footer,
    );
}

fn render_link_preview(frame: &mut Frame, app: &AppState, header: Rect, theme: &Theme) {
    let Some(link) = app
        .transcript_view
        .highlighted_link
        .and_then(|index| app.transcript_view.hyperlinks.get(index))
    else {
        return;
    };
    let available = header.width.saturating_sub(24);
    if available < 12 {
        return;
    }
    let text = super::ui_chrome::truncate_plain_text(&link.destination, usize::from(available));
    let width = u16::try_from(text.width()).unwrap_or(available);
    frame.render_widget(
        Paragraph::new(text).style(
            Style::default()
                .fg(theme.markdown.link)
                .bg(theme.surface.canvas),
        ),
        Rect::new(header.right().saturating_sub(width), header.y, width, 1),
    );
}

fn render_status(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    header: &crate::app::subagents::SubagentHeader,
    theme: &Theme,
) {
    use crate::app::subagents::activity::{
        pretty_tool_name, retry_label, subject_label, tool_activity,
    };
    use crate::app::LiveTurnPhase;
    let activity = app.activities.back();
    let phase = activity.map(|activity| app.live_turn_phase(activity).0);
    if child_parked(app) && !header.cancelling {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("○ ", Style::default().fg(theme.agents.build)),
                Span::styled(
                    "waiting · send a message to interrupt",
                    Style::default().fg(theme.text.secondary),
                ),
            ])),
            Rect::new(area.x + 2, area.y, area.width.saturating_sub(2), 1),
        );
        return;
    }
    let tool = activity.and_then(|activity| {
        activity.tool_calls.iter().find(|tool|
        matches!(&phase, Some(LiveTurnPhase::ToolRunning(id)) if id == &tool.tool_call_id))
    });
    let tool = tool.map(tool_activity);
    let asking = tool
        .as_ref()
        .is_some_and(|(title, _)| title.starts_with("Ask: ") || title.starts_with("Ask "));
    let described = tool
        .as_ref()
        .is_some_and(|(_, description)| description.is_some());
    let retry = matches!(phase, Some(LiveTurnPhase::Retrying(_)));
    let color = if header.cancelling {
        theme.status.error
    } else if retry {
        theme.status.warning
    } else if tool.is_some() && !described && !asking {
        theme.status.success
    } else {
        theme.terminal_colors.prompt_accent
    };
    let timer = if asking || area.width.saturating_sub(2) < 60 {
        String::new()
    } else {
        format!(
            " {}",
            crate::app::subagents::duration_label(header.phase_elapsed_ms)
        )
    };
    let budget = usize::from(area.width).saturating_sub(14 + timer.width());
    let truncate = |text: &str, width| crate::app::subagents::activity::truncate_width(text, width);
    let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let icon = frames[(app.transcript_animation_phase() / 4) % frames.len()];
    let mut spans = vec![Span::styled(format!("{icon} "), Style::default().fg(color))];
    if let Some((title, description)) = tool.filter(|_| !header.cancelling && !retry) {
        if asking {
            let detail = title
                .strip_prefix("Ask: ")
                .or_else(|| title.strip_prefix("Ask "))
                .unwrap_or(&title);
            spans.push(Span::styled(
                truncate(&format!("Waiting on answers for {detail}"), budget),
                Style::default().fg(color),
            ));
        } else if let Some(description) = description {
            spans.push(Span::styled(
                truncate(&subject_label(&description), budget),
                Style::default().fg(color),
            ));
        } else {
            let (prefix, detail) = if let Some(query) = title.strip_prefix("Web search: ") {
                ("Search ", query.trim_matches('"').to_owned())
            } else if let Some(url) = title.strip_prefix("Fetch: ") {
                ("Fetch ", url.to_owned())
            } else {
                ("Run ", pretty_tool_name(&title))
            };
            let detail = detail.lines().next().unwrap_or(&detail);
            let detail = truncate(detail, budget.saturating_sub(prefix.width()).max(5));
            spans.push(Span::styled(
                prefix,
                Style::default().fg(theme.text.secondary),
            ));
            if prefix == "Run " {
                let lines = super::ui_syntax_highlight::render_highlighted_code_block(
                    Some("bash"),
                    &detail,
                    &detail,
                    "",
                    theme.agents.build,
                    theme,
                );
                spans.extend(
                    lines
                        .into_iter()
                        .next()
                        .unwrap_or_default()
                        .spans
                        .into_iter()
                        .map(|mut span| {
                            span.style.bg = None;
                            span
                        }),
                );
            } else {
                spans.push(Span::styled(
                    detail,
                    Style::default().fg(theme.agents.build),
                ));
            }
        }
    } else {
        let label = if header.cancelling {
            "Cancelling…".into()
        } else if let (Some(LiveTurnPhase::Retrying(attempt)), Some(activity)) = (&phase, activity)
        {
            retry_label(activity, *attempt)
        } else {
            match header.activity.as_str() {
                "Thinking" | "Responding" | "Compacting" => format!("{}…", header.activity),
                _ => header.activity.clone(),
            }
        };
        spans.push(Span::styled(
            truncate(&label, budget),
            Style::default().fg(color),
        ));
    }
    spans.push(Span::styled(
        timer,
        Style::default().fg(theme.text.secondary),
    ));
    frame.render_widget(
        Paragraph::new(Line::from(spans)),
        Rect::new(
            area.x + 2,
            area.y,
            area.width.saturating_sub(12),
            area.height,
        ),
    );
    frame.render_widget(
        Paragraph::new(" [stop]").style(Style::default().fg(theme.text.secondary)),
        Rect::new(
            area.right().saturating_sub(7),
            area.y,
            7.min(area.width),
            area.height,
        ),
    );
}

fn child_parked(app: &AppState) -> bool {
    app.activities.back().is_some_and(|activity| {
        matches!(
            app.live_turn_phase(activity).0,
            crate::app::LiveTurnPhase::WaitingFor(
                crate::app::WaitingReason::TaskOutput(_)
                    | crate::app::WaitingReason::TasksComplete
                    | crate::app::WaitingReason::Sleep
            )
        )
    })
}

fn render_title(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    header: &crate::app::subagents::SubagentHeader,
    theme: &Theme,
) {
    let running = matches!(
        header.status,
        ActivityStatus::Queued | ActivityStatus::Streaming
    );
    let (icon, color) = if running {
        let frames = ["⋅", ":", "⸬", "⁙", "⋅", ":", "⸬", "⁙"];
        (
            frames[(app.transcript_view.transcript_animation_phase / 4) % frames.len()],
            theme.text.accent,
        )
    } else if header.status == ActivityStatus::Done {
        ("✓", theme.status.success)
    } else {
        ("✗", theme.status.error)
    };
    let elapsed = crate::app::subagents::duration_label(header.elapsed_ms);
    let activity = if running {
        format!("{} · ", header.activity)
    } else {
        String::new()
    };
    let badge = if header.badge.is_empty() {
        String::new()
    } else {
        format!("{} ", header.badge)
    };
    let right = format!("{badge}{activity}{elapsed} ");
    let close_x = area.right().saturating_sub(5);
    let right_width = u16::try_from(right.width()).unwrap_or(u16::MAX);
    let label = if header.description.is_empty() {
        header.label.clone()
    } else {
        format!("{} ", header.label)
    };
    let model = if header.model.is_empty() {
        String::new()
    } else {
        format!(" {}", header.model)
    };
    let budget = usize::from(area.width.saturating_sub(9 + right_width))
        .saturating_sub(label.width() + model.width());
    let description = crate::app::subagents::activity::truncate_width(
        &crate::text::collapse_inline_whitespace(&header.description),
        budget,
    );
    let y = area.y + 1;
    let buffer = frame.buffer_mut();
    buffer.set_span(
        area.x + 1,
        y,
        &Span::styled(format!(" {icon}"), Style::default().fg(color)),
        3,
    );
    let mut x = area.x + 4;
    for (text, style) in [
        (label, Style::default().fg(color)),
        (
            description,
            Style::default()
                .fg(theme.text.primary)
                .add_modifier(Modifier::BOLD),
        ),
        (model, Style::default().fg(theme.text.secondary)),
    ] {
        let width = u16::try_from(text.width()).unwrap_or(u16::MAX);
        buffer.set_span(x, y, &Span::styled(text, style), width);
        x = x.saturating_add(width);
    }
    let close_style = if app.hovered_subagent_frame_target.is_some() {
        Style::default()
            .fg(theme.text.primary)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text.secondary)
    };
    buffer.set_span(close_x, y, &Span::styled("[✗]", close_style), 3);
    let elapsed_width = u16::try_from(elapsed.width()).unwrap_or(u16::MAX);
    let mut x = close_x.saturating_sub(elapsed_width + 1);
    buffer.set_span(
        x,
        y,
        &Span::styled(elapsed, Style::default().fg(theme.text.secondary)),
        elapsed_width,
    );
    for (text, color) in [
        (activity, theme.text.secondary),
        (badge, theme.terminal_colors.muted),
    ] {
        let width = u16::try_from(text.width()).unwrap_or(u16::MAX);
        x = x.saturating_sub(width);
        buffer.set_span(x, y, &Span::styled(text, Style::default().fg(color)), width);
    }
}

fn child_shortcuts(app: &AppState) -> Vec<(&'static str, &'static str)> {
    let can_cancel = !child_parked(app)
        && app.inspected_child_cancel_started().is_none()
        && app.subagent_header().is_some_and(|header| {
            matches!(
                header.status,
                ActivityStatus::Queued | ActivityStatus::Streaming
            )
        });
    if app.transcript_view.search.has_bar() {
        let primary = if !app.composer.vim_mode {
            ("↓/↑", "next/prev")
        } else if app.transcript_view.search.editing {
            ("Enter", "go")
        } else {
            ("n/Shift+n", "next/prev")
        };
        let mut shortcuts = vec![primary, ("Esc", "cancel")];
        if can_cancel {
            shortcuts.push(("Ctrl+c", "cancel"));
        }
        return shortcuts;
    }
    let selected = app.selected_transcript_entry();
    let message = selected
        .as_ref()
        .is_some_and(|entry| entry.kind == super::TranscriptRenderSurfaceKind::AssistantBody);
    let tool = selected
        .as_ref()
        .and_then(|entry| match entry.target.as_ref() {
            Some(super::TranscriptMouseTarget::Tool { tool_call_id }) => Some(tool_call_id),
            _ => None,
        });
    let mut shortcuts = vec![("q/Esc", "back")];
    if message && app.composer.vim_mode {
        shortcuts.push(("y", "copy"));
    }
    if let Some(id) = tool {
        let expanded = app
            .activities
            .iter()
            .flat_map(|activity| &activity.tool_calls)
            .find(|tool| &tool.tool_call_id == id)
            .is_some_and(|tool| app.tool_output_expanded(tool));
        shortcuts.push(if expanded {
            ("←", "collapse")
        } else {
            ("→", "expand")
        });
    }
    if selected
        .as_ref()
        .is_some_and(|entry| entry.kind != super::TranscriptRenderSurfaceKind::User)
    {
        shortcuts.push((
            "Enter",
            if message || tool.is_some() {
                "open"
            } else {
                "expand"
            },
        ));
    }
    if app.composer.vim_mode {
        shortcuts.extend([("j/k", "nav"), ("Shift+l/h", "turn")]);
    }
    shortcuts.extend([(
        "Ctrl+e",
        if app.transcript_view.show_transcript_thinking {
            "collapse thinking"
        } else {
            "expand thinking"
        },
    )]);
    if can_cancel {
        shortcuts.push(("Ctrl+c", "cancel"));
    }
    shortcuts
}
