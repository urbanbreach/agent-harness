use ratatui::style::Style;

use crate::{
    app::{ActivityEntry, LiveTurnWatchers, ToolCallDisplayStatus},
    theme::Theme,
};

pub(super) struct LiveTurnStatus {
    pub(super) label: String,
    pub(super) style: Style,
    pub(super) phase_elapsed_ms: Option<u64>,
    pub(super) allows_stop: bool,
    pub(super) allows_send_now: bool,
}

impl LiveTurnStatus {
    pub(super) fn cancelling(theme: &Theme) -> Self {
        Self {
            label: "Cancelling…".to_string(),
            style: Style::default().fg(theme.status.error),
            phase_elapsed_ms: None,
            allows_stop: true,
            allows_send_now: false,
        }
    }

    pub(super) fn waiting(theme: &Theme) -> Self {
        Self {
            label: "Waiting for response…".to_string(),
            style: Style::default().fg(theme.text.secondary),
            phase_elapsed_ms: None,
            allows_stop: true,
            allows_send_now: false,
        }
    }

    pub(super) fn recovering(theme: &Theme) -> Self {
        Self {
            label: "Recovering live state…".to_string(),
            style: Style::default().fg(theme.status.warning),
            phase_elapsed_ms: None,
            allows_stop: true,
            allows_send_now: false,
        }
    }

    pub(super) fn reconnecting(theme: &Theme) -> Self {
        Self {
            label: "Reconnecting live state…".to_string(),
            style: Style::default().fg(theme.status.warning),
            phase_elapsed_ms: None,
            allows_stop: true,
            allows_send_now: false,
        }
    }

    pub(super) fn background(watchers: LiveTurnWatchers, theme: &Theme) -> Self {
        let label = format_still_running(watchers);
        Self {
            label,
            style: Style::default().fg(theme.text.secondary),
            phase_elapsed_ms: None,
            allows_stop: false,
            allows_send_now: false,
        }
    }

    pub(super) fn parked(watchers: LiveTurnWatchers, theme: &Theme) -> Self {
        Self {
            label: if watchers.total() > 0 {
                format_still_running(watchers)
            } else {
                "waiting".to_string()
            },
            style: Style::default().fg(theme.text.secondary),
            phase_elapsed_ms: None,
            allows_stop: false,
            allows_send_now: true,
        }
    }

    pub(super) fn from_activity(activity: &ActivityEntry, theme: &Theme) -> Self {
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
                allows_stop: true,
                allows_send_now: false,
            };
        }

        if let Some(tool) = activity
            .tool_calls
            .iter()
            .rev()
            .find(|tool| tool.status == ToolCallDisplayStatus::Running)
        {
            let (label, style, phase_elapsed_ms) = match tool.effective_tool_id() {
                "question" | "user.question" => (
                    "Waiting on answers".to_string(),
                    Style::default().fg(theme.text.secondary),
                    None,
                ),
                "task" | "agent.spawn" => (
                    "Waiting on subagent…".to_string(),
                    Style::default().fg(theme.text.secondary),
                    Some(tool.last_mono_ms.saturating_sub(tool.first_mono_ms)),
                ),
                "background_output" => (
                    "Waiting on task output…".to_string(),
                    Style::default().fg(theme.text.secondary),
                    Some(tool.last_mono_ms.saturating_sub(tool.first_mono_ms)),
                ),
                _ => (
                    format!("Run {}", tool.effective_tool_id()),
                    Style::default().fg(theme.status.success),
                    Some(tool.last_mono_ms.saturating_sub(tool.first_mono_ms)),
                ),
            };
            return Self {
                label,
                style,
                phase_elapsed_ms,
                allows_stop: true,
                allows_send_now: activity.is_sendable_wait(),
            };
        }

        if !activity.transcript_text.is_empty() {
            return Self {
                label: "Responding…".to_string(),
                style: Style::default().fg(theme.text.secondary),
                phase_elapsed_ms: activity.responding_duration_ms(),
                allows_stop: true,
                allows_send_now: false,
            };
        }

        if !activity.thinking_text.trim().is_empty() {
            let phase_elapsed_ms = activity.thinking_duration_ms();
            return Self {
                label: "Thinking…".to_string(),
                style: Style::default().fg(theme.text.secondary),
                phase_elapsed_ms,
                allows_stop: true,
                allows_send_now: false,
            };
        }

        Self::waiting(theme)
    }
}

pub(super) fn format_still_running(watchers: LiveTurnWatchers) -> String {
    let mut kinds = Vec::with_capacity(5);
    for (count, noun) in [
        (watchers.commands, "command"),
        (watchers.monitors, "monitor"),
        (watchers.loops, "loop"),
        (watchers.subagents, "subagent"),
        (watchers.workflows, "workflow"),
    ] {
        if count > 0 {
            kinds.push(format!(
                "{count} {noun}{}",
                if count == 1 { "" } else { "s" }
            ));
        }
    }
    format!("{} still running", kinds.join(" · "))
}

pub(super) fn format_elapsed_ms(duration_ms: u64) -> String {
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

pub(super) fn format_tokens_short(tokens: u32) -> String {
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
