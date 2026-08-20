use ratatui::style::{Color, Modifier, Style};

use crate::app::{ToolCallDisplayStatus, ToolCallEntry, ToolCallPresentationStatus};
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TranscriptToolCallVisualStyle {
    Inline,
    TaskInline,
    Block,
}

pub(super) fn generic_tool_visual_style(
    tool_call: &ToolCallEntry,
    generic_output_visible: bool,
) -> TranscriptToolCallVisualStyle {
    if tool_call.status == ToolCallDisplayStatus::Failed {
        return TranscriptToolCallVisualStyle::Block;
    }

    if generic_output_visible && tool_call.output_summary.is_some() {
        TranscriptToolCallVisualStyle::Block
    } else {
        TranscriptToolCallVisualStyle::Inline
    }
}

pub(super) fn inline_tool_color(status: ToolCallPresentationStatus, theme: &Theme) -> Color {
    match status {
        ToolCallPresentationStatus::Queued | ToolCallPresentationStatus::Succeeded => {
            theme.text.secondary
        }
        ToolCallPresentationStatus::Running => theme.text.primary,
        ToolCallPresentationStatus::Waiting => theme.status.warning,
        ToolCallPresentationStatus::Failed => theme.status.error,
        ToolCallPresentationStatus::Cancelled => theme.status.disabled,
    }
}

pub(super) fn task_inline_tool_color(
    status: ToolCallPresentationStatus,
    theme: &Theme,
    clickable_hovered: bool,
) -> Color {
    match status {
        ToolCallPresentationStatus::Waiting => theme.status.warning,
        ToolCallPresentationStatus::Running | ToolCallPresentationStatus::Queued => {
            theme.text.primary
        }
        ToolCallPresentationStatus::Succeeded if clickable_hovered => theme.text.primary,
        ToolCallPresentationStatus::Succeeded => theme.text.secondary,
        ToolCallPresentationStatus::Failed => theme.status.error,
        ToolCallPresentationStatus::Cancelled => theme.status.disabled,
    }
}

pub(super) fn block_tool_color(status: ToolCallPresentationStatus, theme: &Theme) -> Color {
    block_status_color(status, theme, theme.text.primary)
}

fn block_status_color(
    status: ToolCallPresentationStatus,
    theme: &Theme,
    active_color: Color,
) -> Color {
    match status {
        ToolCallPresentationStatus::Waiting => theme.status.warning,
        ToolCallPresentationStatus::Failed => theme.status.error,
        ToolCallPresentationStatus::Cancelled => theme.status.disabled,
        ToolCallPresentationStatus::Queued
        | ToolCallPresentationStatus::Running
        | ToolCallPresentationStatus::Succeeded => active_color,
    }
}

pub(super) fn status_label<'a>(
    status: ToolCallDisplayStatus,
    pending_queued: &'a str,
    running: &'a str,
    succeeded: &'a str,
    failed: &'a str,
) -> &'a str {
    match status {
        ToolCallDisplayStatus::PendingPermission | ToolCallDisplayStatus::Queued => pending_queued,
        ToolCallDisplayStatus::Running => running,
        ToolCallDisplayStatus::Succeeded => succeeded,
        ToolCallDisplayStatus::Failed => failed,
    }
}

pub(super) fn tool_call_header_style(struck_out: bool, color: Color) -> Style {
    let mut style = Style::default().fg(color);
    if struck_out {
        style = style.add_modifier(Modifier::CROSSED_OUT);
    }
    style
}

#[cfg(test)]
mod presentation_style_tests {
    use super::*;
    use crate::app::ToolCallPresentationStatus;

    #[test]
    fn inline_status_colors_distinguish_running_success_error_waiting_and_cancelled() {
        // arrange
        // act
        let theme = Theme::default();

        // assert
        assert_eq!(
            inline_tool_color(ToolCallPresentationStatus::Running, &theme),
            theme.text.primary
        );
        assert_eq!(
            inline_tool_color(ToolCallPresentationStatus::Succeeded, &theme),
            theme.text.secondary
        );
        assert_eq!(
            inline_tool_color(ToolCallPresentationStatus::Failed, &theme),
            theme.status.error
        );
        assert_eq!(
            inline_tool_color(ToolCallPresentationStatus::Waiting, &theme),
            theme.status.warning
        );
        assert_eq!(
            inline_tool_color(ToolCallPresentationStatus::Cancelled, &theme),
            theme.status.disabled
        );
    }
}
