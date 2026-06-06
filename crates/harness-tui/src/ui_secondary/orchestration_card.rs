use super::*;
use crate::app::{OrchestrationTaskRow, OrchestrationTaskState};

#[cfg(test)]
pub(crate) fn orchestration_card_text_for_test(
    app: &AppState,
    height: u16,
    width: u16,
) -> Vec<String> {
    orchestration_card_lines(
        app,
        &app.orchestration_visible_rows(),
        app.theme(),
        height,
        width,
    )
    .into_iter()
    .map(|line| {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    })
    .collect()
}

#[cfg(test)]
fn orchestration_card_lines(
    app: &AppState,
    rows: &[OrchestrationTaskRow],
    theme: &Theme,
    height: u16,
    width: u16,
) -> Vec<Line<'static>> {
    if height == 0 {
        return Vec::new();
    }

    let mut lines = Vec::new();
    lines.push(orchestration_summary_line(app, theme, width));

    if height == 1 {
        return lines;
    }

    lines.push(orchestration_warning_line(app, theme, width));
    let task_slots = usize::from(height.saturating_sub(2));
    if task_slots == 0 || rows.is_empty() {
        return lines;
    }

    if rows.len() <= task_slots {
        lines.extend(
            rows.iter()
                .map(|row| orchestration_task_line(app, row, theme, width)),
        );
        return lines;
    }

    if task_slots == 1 {
        lines.push(orchestration_overflow_line(rows.len(), theme));
        return lines;
    }

    let visible_task_count = task_slots.saturating_sub(1);
    lines.extend(
        rows.iter()
            .take(visible_task_count)
            .map(|row| orchestration_task_line(app, row, theme, width)),
    );
    lines.push(orchestration_overflow_line(
        rows.len().saturating_sub(visible_task_count),
        theme,
    ));
    lines
}

#[cfg(test)]
fn orchestration_summary_line(app: &AppState, theme: &Theme, width: u16) -> Line<'static> {
    let summary = app.orchestration_summary();
    let text = format!(
        "overview · {} active agents · {} queued · {} running · {} stale",
        summary.active_agents, summary.queued, summary.running, summary.stale
    );
    Line::from(Span::styled(
        truncate_plain_text(&text, usize::from(width)),
        muted_meta_style(theme),
    ))
}

#[cfg(test)]
fn orchestration_warning_line(app: &AppState, theme: &Theme, width: u16) -> Line<'static> {
    let warning = app.orchestration_latest_warning().unwrap_or("none");
    let text = format!("watch · {warning}");
    Line::from(Span::styled(
        truncate_plain_text(&text, usize::from(width)),
        Style::default().fg(theme.status.warning),
    ))
}

#[cfg(test)]
fn orchestration_task_line(
    app: &AppState,
    row: &OrchestrationTaskRow,
    theme: &Theme,
    width: u16,
) -> Line<'static> {
    let (state_label, state_color) = orchestration_state_tokens(row.state, theme);
    let owner = app.orchestration_owner_labels(row);
    let queue_key = row.queue_key.as_deref().unwrap_or("queue:none");
    let detail = format!(
        "{} · {}/{} · {}",
        row.task_id, owner.label, owner.profile, queue_key
    );

    let badge_width = state_label.chars().count().saturating_add(4);
    let detail = truncate_plain_text(&detail, usize::from(width).saturating_sub(badge_width));

    Line::from(vec![
        status_badge(state_label, state_color, theme),
        Span::raw(" "),
        Span::styled(detail, muted_meta_style(theme)),
    ])
}

#[cfg(test)]
fn orchestration_overflow_line(hidden_count: usize, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(
        format!("+{hidden_count} more"),
        Style::default()
            .fg(theme.text.tertiary)
            .add_modifier(Modifier::BOLD),
    ))
}

#[cfg(test)]
fn orchestration_state_tokens(
    state: OrchestrationTaskState,
    theme: &Theme,
) -> (&'static str, Color) {
    match state {
        OrchestrationTaskState::Queued => ("queued", theme.text.secondary),
        OrchestrationTaskState::Running => ("running", theme.status.info),
        OrchestrationTaskState::Stale => ("stale", theme.status.warning),
        OrchestrationTaskState::Completed => ("completed", theme.status.success),
        OrchestrationTaskState::Cancelled => ("cancelled", theme.status.error),
        OrchestrationTaskState::Failed => ("failed", theme.status.error),
        OrchestrationTaskState::TimedOut => ("timed-out", theme.status.error),
        OrchestrationTaskState::LateResult => ("late-result", theme.status.warning),
    }
}
