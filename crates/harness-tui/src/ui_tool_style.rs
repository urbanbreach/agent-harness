use ratatui::style::{Color, Modifier, Style};

use crate::app::{ToolCallDisplayStatus, ToolCallEntry};

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
