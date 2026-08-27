use ratatui::{
    layout::{Alignment, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};

use crate::{
    app::{ActivityStatus, AppState, RuntimeStateKind},
    theme::Theme,
};

use super::{
    ui_chrome::{display_width, truncate_plain_text},
    ui_context_budget::ContextBudget,
    ui_transcript_style::{monitor_pulse_frame, transcript_streaming_spinner_frame},
};

#[path = "ui_live_turn_status_presentation.rs"]
mod status_model;

#[path = "ui_live_turn_status_geometry.rs"]
mod geometry;

#[path = "ui_live_turn_status_render_parts.rs"]
mod render_parts;

use geometry::{
    live_turn_background_label, live_turn_control_visibility, live_turn_is_parked, parked_suffix,
};
pub(crate) use geometry::{
    live_turn_background_rect, live_turn_stop_rect, live_turn_watching_rect,
};
use render_parts::RightStatusInput;
use status_model::{format_tokens_short, LiveTurnStatus};

pub(super) fn format_elapsed_ms(duration_ms: u64) -> String {
    status_model::format_elapsed_ms(duration_ms)
}

pub(super) fn render_live_turn_status(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
) {
    let area = crate::layout::live_turn_status_content_area(area, theme);
    if area.width == 0 || area.height == 0 || !app.live_turn_status_visible() {
        return;
    }

    let runtime_kind = app.runtime_state().kind;
    let activity = app
        .runtime_state_activity()
        .filter(|entry| entry.status == ActivityStatus::Streaming);
    let foreground_work = app.live_turn_stop_available();
    let watchers = app.live_turn_watchers();
    let background_tasks = watchers.total();
    let parked = activity.is_some_and(|entry| entry.is_parked_wait());
    let status = if app.interrupt_requested() {
        LiveTurnStatus::cancelling(theme)
    } else {
        match runtime_kind {
            RuntimeStateKind::Degraded => LiveTurnStatus::recovering(theme),
            RuntimeStateKind::Disconnected => LiveTurnStatus::reconnecting(theme),
            RuntimeStateKind::Sending | RuntimeStateKind::Streaming => activity.map_or_else(
                || {
                    if background_tasks > 0 && !foreground_work {
                        LiveTurnStatus::background(watchers, theme)
                    } else {
                        LiveTurnStatus::waiting(theme)
                    }
                },
                |entry| {
                    if parked {
                        LiveTurnStatus::parked(watchers, theme)
                    } else {
                        LiveTurnStatus::from_activity(entry, theme)
                    }
                },
            ),
            RuntimeStateKind::Ready | RuntimeStateKind::Success if foreground_work => {
                LiveTurnStatus::waiting(theme)
            }
            RuntimeStateKind::Ready | RuntimeStateKind::Success if background_tasks > 0 => {
                LiveTurnStatus::background(watchers, theme)
            }
            RuntimeStateKind::Ready
            | RuntimeStateKind::Success
            | RuntimeStateKind::Failure
            | RuntimeStateKind::Cancelled
            | RuntimeStateKind::PermissionBlocked
            | RuntimeStateKind::PermissionPending => return,
        }
    };
    let projected_total = (!parked)
        .then(|| activity.map(|entry| entry.last_mono_ms.saturating_sub(entry.first_mono_ms)))
        .flatten();
    let live_phase_elapsed_ms = (!parked)
        .then(|| activity.and_then(|entry| app.live_turn_phase_elapsed_ms_for(&entry.request_id)))
        .flatten();
    let phase = live_phase_elapsed_ms
        .or(status.phase_elapsed_ms)
        .or(projected_total)
        .map(format_elapsed_ms)
        .unwrap_or_default();
    let total = (!parked
        && matches!(
            runtime_kind,
            RuntimeStateKind::Sending
                | RuntimeStateKind::Streaming
                | RuntimeStateKind::Degraded
                | RuntimeStateKind::Disconnected
        ))
    .then(|| match (app.live_turn_elapsed_ms(), projected_total) {
        (Some(live_elapsed), Some(projected_elapsed)) => Some(live_elapsed.max(projected_elapsed)),
        (Some(live_elapsed), None) => Some(live_elapsed),
        (None, Some(projected_elapsed)) => Some(projected_elapsed),
        (None, None) => None,
    })
    .flatten()
    .map(format_elapsed_ms);
    let tokens = (!parked)
        .then_some(activity)
        .flatten()
        .and_then(|entry| entry.usage.map(|usage| usage.total_tokens))
        .filter(|tokens| *tokens > 0)
        .map(|tokens| format!("⇣{}", format_tokens_short(tokens)));
    let send_now = if parked {
        Some(parked_suffix(app))
    } else {
        (status.allows_send_now && app.queued_prompt_send_now_available())
            .then(|| format!("· {} queued — Enter to send now", app.queued_prompt_count))
    };
    let uses_monitor_pulse = parked || (background_tasks > 0 && !foreground_work);
    let spinner = if uses_monitor_pulse {
        monitor_pulse_frame(app.transcript_animation_phase())
    } else {
        transcript_streaming_spinner_frame(app.transcript_animation_phase())
    };
    let spinner_width = display_width(spinner).saturating_add(1);
    let full_label_width = display_width(&status.label);
    let control_visibility = live_turn_control_visibility(app, area);
    let stop_visible = status.allows_stop && control_visibility.stop;
    let background_label = live_turn_background_label(app);
    let background_visible = status.allows_stop && control_visibility.background;
    let mut right_parts = Vec::new();
    if background_visible {
        right_parts.push(background_label.to_string());
    }
    if stop_visible {
        right_parts.push(geometry::STOP_LABEL.to_string());
    }
    let right_width = |parts: &[String]| {
        parts
            .iter()
            .map(|part| display_width(part))
            .sum::<usize>()
            .saturating_add(parts.len().saturating_sub(1))
    };
    let gap_width = usize::from(!right_parts.is_empty());
    let phase_width = display_width(&phase).saturating_add(usize::from(!phase.is_empty()));
    let phase_visible = !phase.is_empty()
        && spinner_width
            .saturating_add(full_label_width)
            .saturating_add(phase_width)
            .saturating_add(right_width(&right_parts))
            .saturating_add(gap_width)
            <= usize::from(area.width);
    let reserved_left = spinner_width
        .saturating_add(full_label_width)
        .saturating_add(if phase_visible { phase_width } else { 0 });
    let context_budget = ContextBudget::from_app(app);
    let mut context_label = None;
    if send_now.is_none() {
        if let Some(budget) = context_budget.as_ref() {
            for candidate in [budget.full_label(), budget.compact_label()] {
                if context_label.as_deref() == Some(candidate) {
                    continue;
                }
                let control_count = usize::from(stop_visible) + usize::from(background_visible);
                let insert_at = right_parts.len().saturating_sub(control_count);
                right_parts.insert(insert_at, candidate.to_string());
                if reserved_left
                    .saturating_add(right_width(&right_parts))
                    .saturating_add(1)
                    <= usize::from(area.width)
                {
                    context_label = Some(candidate.to_string());
                    break;
                }
                right_parts.remove(insert_at);
            }
        }
    }
    for candidate in [total, tokens].into_iter().flatten() {
        let control_count = usize::from(stop_visible) + usize::from(background_visible);
        let insert_at = right_parts.len().saturating_sub(control_count);
        right_parts.insert(insert_at, candidate);
        if reserved_left
            .saturating_add(right_width(&right_parts))
            .saturating_add(1)
            > usize::from(area.width)
        {
            right_parts.remove(insert_at);
        }
    }
    let right_width = right_width(&right_parts);
    let send_now_visible = send_now.as_ref().is_some_and(|hint| {
        reserved_left
            .saturating_add(display_width(hint))
            .saturating_add(1)
            .saturating_add(right_width)
            .saturating_add(usize::from(right_width > 0))
            <= usize::from(area.width)
    });
    let send_now_width = send_now
        .as_ref()
        .filter(|_| send_now_visible)
        .map_or(0, |hint| display_width(hint).saturating_add(1));
    let fixed_left_width = spinner_width
        .saturating_add(if phase_visible { phase_width } else { 0 })
        .saturating_add(send_now_width);
    let label_width = usize::from(area.width)
        .saturating_sub(right_width)
        .saturating_sub(usize::from(right_width > 0))
        .saturating_sub(fixed_left_width);
    let label = truncate_plain_text(&status.label, label_width.max(1));

    frame.render_widget(Block::default().style(Style::default()), area);
    let spinner_style = if uses_monitor_pulse {
        Style::default().fg(theme.status.info)
    } else {
        status.style
    };
    let mut left_spans = vec![Span::styled(format!("{spinner} "), spinner_style)];
    left_spans.push(Span::styled(label, status.style));
    if phase_visible {
        left_spans.push(Span::raw(" "));
        left_spans.push(Span::styled(
            phase,
            Style::default().fg(theme.text.secondary),
        ));
    }
    if send_now_visible {
        left_spans.push(Span::raw(" "));
        left_spans.push(Span::styled(
            send_now.unwrap_or_default(),
            Style::default().fg(theme.text.secondary),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(left_spans)), area);
    let right_spans = RightStatusInput {
        parts: &right_parts,
        background_label,
        background_visible,
        stop_visible,
        background_hovered: app.live_turn_background_hovered(),
        stop_hovered: app.live_turn_stop_hovered(),
        context_label: context_label.as_deref(),
        context_tone: context_budget.as_ref().map(ContextBudget::tone),
    }
    .into_spans(theme);
    frame.render_widget(
        Paragraph::new(Line::from(right_spans)).alignment(Alignment::Right),
        area,
    );
}
