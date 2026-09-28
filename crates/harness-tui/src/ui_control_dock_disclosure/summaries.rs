use super::*;
use crate::app::RuntimeStateKind;
use std::borrow::Cow;
use std::fmt::Write;

pub(super) fn candidates<'a>(app: &'a AppState, theme: &Theme, active: bool) -> Vec<Line<'a>> {
    let base = Style::default().bg(theme.surface.canvas);
    if active {
        return [false, true]
            .into_iter()
            .map(|compact| {
                let text = compaction_text(app, compact);
                if text.is_empty() {
                    Line::default()
                } else {
                    Line::from(Span::styled(text, base.fg(theme.text.secondary)))
                }
            })
            .chain(std::iter::once(Line::default()))
            .collect();
    }
    let runtime = app.runtime_state_view();
    let color = match runtime.kind {
        RuntimeStateKind::Success
        | RuntimeStateKind::Failure
        | RuntimeStateKind::Cancelled
        | RuntimeStateKind::Disconnected
        | RuntimeStateKind::PermissionBlocked => theme.text.accent,
        _ => theme.terminal_colors.primary,
    };
    let short = runtime
        .summary
        .split(" · ")
        .next()
        .unwrap_or(&runtime.summary)
        .to_string();
    let segment = if app.completed_session_shell_active() {
        super::super::control_dock_summary_segment(app)
    } else {
        app.runtime_context_summary_segment()
    };
    let extra = segment.map(|segment| {
        use crate::view_model::ControlDockSummaryTone;
        let color = match segment.tone {
            ControlDockSummaryTone::Secondary => theme.text.secondary,
            ControlDockSummaryTone::Warning => theme.terminal_colors.primary,
            _ => theme.text.accent,
        };
        [
            Line::from(Span::styled(
                format!("{}  ·  {}", runtime.summary, segment.text),
                base.fg(color),
            )),
            Line::from(Span::styled(segment.text, base.fg(color))),
        ]
    });
    [
        Line::from(Span::styled(runtime.summary, base.fg(color))),
        Line::from(Span::styled(short, base.fg(color))),
    ]
    .into_iter()
    .chain(extra.into_iter().flatten())
    .chain(std::iter::once(Line::default()))
    .collect()
}

fn compaction_text(app: &AppState, compact: bool) -> Cow<'_, str> {
    let metrics = app.compaction_usage_metrics();
    if metrics.completed_count == 0 {
        return app
            .compaction_status()
            .filter(|status| status.state != crate::app::CompactionState::Applied)
            .map_or("", |status| status.message.as_str())
            .into();
    }
    let count_label = if compact { "cmp" } else { "compactions" };
    let summary_label = if compact { "sum" } else { "summary" };
    let mut text = format!(
        "{count_label} {} · {summary_label} {} tok",
        metrics.completed_count,
        usage_count(metrics.summary_tokens_estimate)
    );
    if metrics.reduction_tokens_estimate > 0 && !compact {
        let _ = write!(
            text,
            " · saved {} tok",
            usage_count(metrics.reduction_tokens_estimate)
        );
    } else if let Some(percent) = metrics
        .last_reduction_percent_estimate
        .filter(|value| *value > 0)
    {
        let _ = write!(text, " · {percent}% saved");
    }
    text.into()
}

fn usage_count(value: u64) -> String {
    let clamped = f64::from(u32::try_from(value).unwrap_or(u32::MAX));
    if value >= 1_000_000 {
        format!("{:.1}M", clamped / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}K", clamped / 1_000.0)
    } else {
        value.to_string()
    }
}
