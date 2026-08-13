use ratatui::{
    layout::{Alignment, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};

use crate::{
    app::{ActivityEntry, ActivityStatus, AppState, RuntimeStateKind, ToolCallDisplayStatus},
    theme::Theme,
};

use super::{
    ui_chrome::{display_width, truncate_plain_text},
    ui_transcript_style::{pending_diamond_color, transcript_streaming_spinner_frame},
};

const STOP_LABEL: &str = "[stop]";

pub(crate) fn live_turn_stop_rect(app: &AppState, frame_area: Rect) -> Option<Rect> {
    if !app.live_turn_stop_available() || app.active_permission_view().is_some() {
        return None;
    }
    let area = crate::layout::FrameLayoutPlan::for_app(app, frame_area).status?;
    let width = u16::try_from(display_width(STOP_LABEL)).unwrap_or(u16::MAX);
    (area.width >= width).then(|| {
        Rect::new(
            area.x.saturating_add(area.width.saturating_sub(width)),
            area.y,
            width,
            1,
        )
    })
}

pub(super) fn render_live_turn_status(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
) {
    if area.width < 10 || area.height == 0 {
        return;
    }

    let runtime_kind = app.runtime_state().kind;
    if !app.has_live_turn_activity()
        && !matches!(
            runtime_kind,
            RuntimeStateKind::Sending | RuntimeStateKind::Streaming
        )
    {
        return;
    }

    let activity = app
        .runtime_state_activity()
        .filter(|entry| entry.status == ActivityStatus::Streaming);
    let question_detail = app
        .active_permission_view()
        .filter(|permission| permission.kind.eq_ignore_ascii_case("question"))
        .and_then(|permission| permission.question_prompts)
        .and_then(|prompts| prompts.into_iter().next())
        .map(|prompt| prompt.question);
    let status = if app.interrupt_requested() {
        LiveTurnStatus::cancelling(theme)
    } else {
        question_detail.as_deref().map_or_else(
            || {
                activity.map_or_else(
                    || LiveTurnStatus::waiting(theme),
                    |entry| LiveTurnStatus::from_activity(entry, theme),
                )
            },
            |detail| LiveTurnStatus::waiting_on_answers(theme, Some(detail)),
        )
    };
    let waiting_on_answers = question_detail.is_some() && !app.interrupt_requested();
    let projected_total =
        activity.map(|entry| entry.last_mono_ms.saturating_sub(entry.first_mono_ms));
    let live_phase_elapsed_ms =
        activity.and_then(|entry| app.live_turn_phase_elapsed_ms_for(&entry.request_id));
    let phase = live_phase_elapsed_ms
        .or(status.phase_elapsed_ms)
        .or(projected_total)
        .map(format_elapsed_ms)
        .unwrap_or_default();
    let total = match (app.live_turn_elapsed_ms(), projected_total) {
        (Some(live_elapsed), Some(projected_elapsed)) => live_elapsed.max(projected_elapsed),
        (Some(live_elapsed), None) => live_elapsed,
        (None, Some(projected_elapsed)) => projected_elapsed,
        (None, None) => 0,
    };
    let total = format_elapsed_ms(total);
    let tokens = activity
        .and_then(|entry| entry.usage.map(|usage| usage.total_tokens))
        .filter(|tokens| *tokens > 0)
        .or_else(|| {
            app.activities.iter().rev().find_map(|entry| {
                entry
                    .usage
                    .map(|usage| usage.total_tokens)
                    .filter(|tokens| *tokens > 0)
            })
        })
        .map(|tokens| format!(" ⇣{}", format_tokens_short(tokens)));
    let right_meta = format!("{total}{}", tokens.unwrap_or_default());
    let stop = app.live_turn_stop_available().then_some(STOP_LABEL);
    let right = stop.map_or_else(|| right_meta.clone(), |stop| format!("{right_meta} {stop}"));
    let spinner = if waiting_on_answers {
        "◆"
    } else {
        transcript_streaming_spinner_frame(app.transcript_animation_phase())
    };
    let spinner_style = if waiting_on_answers {
        Style::default().fg(pending_diamond_color(
            theme,
            app.transcript_animation_phase(),
        ))
    } else {
        status.style
    };
    let fixed_left_width =
        display_width(spinner)
            .saturating_add(1)
            .saturating_add(if phase.is_empty() {
                0
            } else {
                display_width(&phase).saturating_add(1)
            });
    let label_width = usize::from(area.width)
        .saturating_sub(display_width(&right))
        .saturating_sub(fixed_left_width)
        .saturating_sub(1);
    let label = truncate_plain_text(&status.label, label_width.max(1));

    frame.render_widget(Block::default().style(Style::default()), area);
    let mut left_spans = vec![Span::styled(format!("{spinner} "), spinner_style)];
    left_spans.push(Span::styled(label, status.style));
    if !phase.is_empty() {
        left_spans.push(Span::raw(" "));
        left_spans.push(Span::styled(
            phase,
            Style::default().fg(theme.text.secondary),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(left_spans)), area);
    let mut right_spans = vec![Span::styled(
        right_meta,
        Style::default().fg(theme.text.secondary),
    )];
    if let Some(stop) = stop {
        right_spans.push(Span::raw(" "));
        right_spans.push(Span::styled(
            stop,
            Style::default().fg(if app.live_turn_stop_hovered() {
                theme.status.error
            } else {
                theme.text.secondary
            }),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(right_spans)).alignment(Alignment::Right),
        area,
    );
}

struct LiveTurnStatus {
    label: String,
    style: Style,
    phase_elapsed_ms: Option<u64>,
}

impl LiveTurnStatus {
    fn cancelling(theme: &Theme) -> Self {
        Self {
            label: "Cancelling…".to_string(),
            style: Style::default().fg(theme.status.error),
            phase_elapsed_ms: None,
        }
    }

    fn waiting(theme: &Theme) -> Self {
        Self {
            label: "Waiting for response…".to_string(),
            style: Style::default().fg(theme.text.secondary),
            phase_elapsed_ms: None,
        }
    }

    fn waiting_on_answers(theme: &Theme, detail: Option<&str>) -> Self {
        Self {
            label: detail.map_or_else(
                || "Waiting on answers".to_string(),
                |detail| format!("Waiting on answers for {detail}"),
            ),
            style: Style::default().fg(theme.text.secondary),
            phase_elapsed_ms: None,
        }
    }

    fn from_activity(activity: &ActivityEntry, theme: &Theme) -> Self {
        if let Some(retry) = activity
            .request_data
            .as_ref()
            .and_then(|request| request.metadata.as_ref())
            .and_then(|metadata| metadata.retry)
            .filter(|retry| retry.attempt > 0)
        {
            return Self {
                label: format!("Retrying (attempt {})…", retry.attempt),
                style: Style::default().fg(theme.status.warning),
                phase_elapsed_ms: activity
                    .request_started_mono_ms
                    .map(|started| activity.last_mono_ms.saturating_sub(started)),
            };
        }

        if let Some(tool) = activity
            .tool_calls
            .iter()
            .rev()
            .find(|tool| tool.status == ToolCallDisplayStatus::Running)
        {
            return Self {
                label: format!("Run {}", tool.effective_tool_id()),
                style: Style::default().fg(theme.status.success),
                phase_elapsed_ms: Some(tool.last_mono_ms.saturating_sub(tool.first_mono_ms)),
            };
        }

        if !activity.transcript_text.is_empty() {
            return Self {
                label: "Responding…".to_string(),
                style: Style::default().fg(theme.text.secondary),
                phase_elapsed_ms: activity.responding_duration_ms(),
            };
        }

        if !activity.thinking_text.trim().is_empty() {
            let phase_elapsed_ms = activity.thinking_duration_ms();
            return Self {
                label: "Thinking…".to_string(),
                style: Style::default().fg(theme.text.secondary),
                phase_elapsed_ms,
            };
        }

        Self::waiting(theme)
    }
}

fn format_elapsed_ms(duration_ms: u64) -> String {
    let decimal_duration = u32::try_from(duration_ms).unwrap_or(u32::MAX);
    if duration_ms < 10_000 {
        return format!("{:.1}s", f64::from(decimal_duration) / 1_000.0);
    }
    let total_seconds = duration_ms / 1_000;
    if total_seconds < 60 {
        return format!("{total_seconds}s");
    }
    let minutes = total_seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m{}s", total_seconds % 60);
    }
    format!("{}h{}m", minutes / 60, minutes % 60)
}

fn format_tokens_short(tokens: u32) -> String {
    if tokens < 1_000 {
        return tokens.to_string();
    }
    if tokens < 10_000 {
        return format!("{:.2}k", f64::from(tokens) / 1_000.0);
    }
    if tokens < 100_000 {
        return format!("{:.1}k", f64::from(tokens) / 1_000.0);
    }
    if tokens < 1_000_000 {
        return format!("{}k", tokens / 1_000);
    }
    let decimals = if tokens < 10_000_000 { 2 } else { 1 };
    format!("{:.decimals$}m", f64::from(tokens) / 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::{format_elapsed_ms, format_tokens_short, LiveTurnStatus};
    use crate::theme::Theme;

    #[test]
    fn waiting_status_uses_reference_secondary_text() {
        let theme = Theme::harness_chat();

        let status = LiveTurnStatus::waiting(&theme);

        assert_eq!(status.style.fg, Some(theme.text.secondary));
    }

    #[test]
    fn source_compact_timer_and_token_formats_are_preserved() {
        assert_eq!(format_elapsed_ms(9_999), "10.0s");
        assert_eq!(format_elapsed_ms(10_999), "10s");
        assert_eq!(format_elapsed_ms(3_661_000), "1h1m");
        assert_eq!(format_tokens_short(12_000), "12.0k");
        assert_eq!(format_tokens_short(10_000_000), "10.0m");
    }
}
