use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};

use crate::app::{AppState, OrchestrationTaskState, SubagentSessionInfo};
use crate::theme::Theme;

use super::{display_width, muted_meta_style};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubagentStatus {
    Running,
    Completed,
    Cancelled,
    Error,
}

const SUBAGENT_SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub(super) fn render_subagent_footer(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    _text_area: Rect,
    theme: &Theme,
    info: &SubagentSessionInfo,
) {
    let surface = theme.surface.panel;
    let style = Style::default().bg(surface);
    frame.render_widget(Block::default().style(style), area);
    if area.width == 0 || area.height == 0 {
        return;
    }

    let content_y = if area.height >= 3 {
        area.y.saturating_add(1)
    } else {
        area.y
    };
    let content_x = area.x.saturating_add(1);
    let content_right_padding = 3;
    let used_left = content_x.saturating_sub(area.x);
    let content_width = area
        .width
        .saturating_sub(used_left)
        .saturating_sub(content_right_padding);
    if content_width == 0 {
        return;
    }
    let content_height = area.height.saturating_sub(content_y.saturating_sub(area.y));
    let content_area = Rect::new(content_x, content_y, content_width, content_height);

    let count_text = subagent_count_text(info);
    let count_width = count_text
        .as_deref()
        .map(display_width)
        .unwrap_or_default()
        .min(usize::from(content_area.width));
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(u16::try_from(count_width).unwrap_or(u16::MAX)),
        ])
        .split(Rect::new(
            content_area.x,
            content_area.y,
            content_area.width,
            1,
        ));

    let status = subagent_status(app);
    let mut left_spans = vec![Span::styled(
        status.icon(app.transcript_animation_phase()),
        Style::default().fg(status.color(theme)).bg(surface),
    )];
    left_spans.push(Span::styled(" ", Style::default().bg(surface)));
    left_spans.extend(subagent_title_spans(info, theme, surface));

    if columns[0].width > 0 {
        frame.render_widget(
            Paragraph::new(Line::from(left_spans)).style(style),
            columns[0],
        );
    }
    if columns[1].width > 0 {
        if let Some(count_text) = count_text {
            frame.render_widget(
                Paragraph::new(count_text).style(muted_meta_style(theme).bg(surface)),
                columns[1],
            );
        }
    }

    render_subagent_activity_body(frame, app, content_area, theme, info, surface);
}

fn subagent_status(app: &AppState) -> SubagentStatus {
    if let Some(current_session_id) = app.current_session_id() {
        if let Some(row) = app.orchestration_visible_rows().into_iter().find(|row| {
            row.effective_child_session_id() == Some(current_session_id)
                || row.task_id == current_session_id
        }) {
            return SubagentStatus::from_orchestration_state(row.state);
        }
    }

    if app
        .activities
        .iter()
        .any(|activity| matches!(activity.status, crate::app::ActivityStatus::Error))
    {
        return SubagentStatus::Error;
    }

    if app.activities.iter().any(|activity| {
        matches!(
            activity.status,
            crate::app::ActivityStatus::Queued | crate::app::ActivityStatus::Streaming
        )
    }) {
        SubagentStatus::Running
    } else {
        SubagentStatus::Completed
    }
}

impl SubagentStatus {
    const fn from_orchestration_state(state: OrchestrationTaskState) -> Self {
        match state {
            OrchestrationTaskState::Queued
            | OrchestrationTaskState::Running
            | OrchestrationTaskState::Stale => Self::Running,
            OrchestrationTaskState::Completed | OrchestrationTaskState::LateResult => {
                Self::Completed
            }
            OrchestrationTaskState::Cancelled => Self::Cancelled,
            OrchestrationTaskState::Failed | OrchestrationTaskState::TimedOut => Self::Error,
        }
    }

    fn icon(self, animation_phase: usize) -> &'static str {
        match self {
            Self::Running => {
                SUBAGENT_SPINNER_FRAMES[animation_phase % SUBAGENT_SPINNER_FRAMES.len()]
            }
            Self::Completed => "●",
            Self::Cancelled => "○",
            Self::Error => "◍",
        }
    }

    fn color(self, theme: &Theme) -> ratatui::style::Color {
        match self {
            Self::Running | Self::Completed => theme.text.accent,
            Self::Cancelled => theme.text.secondary,
            Self::Error => theme.status.error,
        }
    }
}

fn subagent_title_spans(
    info: &SubagentSessionInfo,
    theme: &Theme,
    surface: ratatui::style::Color,
) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(
        info.title.clone(),
        Style::default().fg(theme.text.primary).bg(surface),
    )];
    if info.title != info.label {
        spans.push(Span::styled(
            format!("  {}", info.label),
            muted_meta_style(theme).bg(surface),
        ));
    }
    spans
}

fn subagent_count_text(info: &SubagentSessionInfo) -> Option<String> {
    (info.total > 1 && info.index > 0).then(|| format!("{} of {}", info.index, info.total))
}

fn render_subagent_activity_body(
    frame: &mut Frame,
    app: &AppState,
    content_area: Rect,
    theme: &Theme,
    info: &SubagentSessionInfo,
    surface: ratatui::style::Color,
) {
    let body_y = content_area.y.saturating_add(2);
    let body_height = content_area
        .height
        .saturating_sub(body_y.saturating_sub(content_area.y));
    if body_height == 0 {
        return;
    }
    let body_area = Rect::new(content_area.x, body_y, content_area.width, body_height);
    let body = subagent_activity_body_lines(app, theme, info, body_area.width, surface);
    let visible_start = body.len().saturating_sub(usize::from(body_area.height));
    let body = body.into_iter().skip(visible_start).collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(body).style(Style::default().bg(surface)),
        body_area,
    );
}

fn subagent_activity_body_lines(
    app: &AppState,
    theme: &Theme,
    _info: &SubagentSessionInfo,
    width: u16,
    surface: ratatui::style::Color,
) -> Vec<Line<'static>> {
    if app.activities.is_empty() {
        return subagent_empty_activity_lines(theme, surface);
    }

    let lines = super::super::ui_transcript::build_subagent_footer_lines_for_width(
        app, theme, width, surface,
    );
    if lines.is_empty()
        || (lines.len() == 1 && line_plain_text(&lines[0]) == "Waiting for first turn…")
    {
        subagent_empty_activity_lines(theme, surface)
    } else {
        lines
    }
}

fn subagent_empty_activity_lines(
    theme: &Theme,
    surface: ratatui::style::Color,
) -> Vec<Line<'static>> {
    vec![Line::from(Span::styled(
        "No subagent activity yet",
        muted_meta_style(theme).bg(surface),
    ))]
}

fn line_plain_text(line: &Line<'static>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}
