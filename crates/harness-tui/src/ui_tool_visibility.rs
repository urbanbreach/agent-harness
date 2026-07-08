use crate::app::{ToolCallDisplayStatus, ToolCallEntry};
use crate::text::has_trimmed_content;

use super::ui_tool_diffs::tool_call_has_preview_content;
use super::ui_tool_paths::{todo_write_tool_id, tool_id_matches};
use super::ui_tool_titles::is_mcp_tool_id;
use super::ui_transcript_bash::shell_tool_output;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TranscriptToolCallDisclosureState {
    Collapsed,
    Expanded,
}

pub(super) fn tool_hidden_from_transcript(tool_call: &ToolCallEntry) -> bool {
    tool_id_matches(tool_call, &["todo.read", "todoread"])
}

pub(super) fn tool_call_should_remain_visible_without_tool_details(
    tool_call: &ToolCallEntry,
) -> bool {
    if todo_write_tool_id(tool_call.effective_tool_id()) || todo_write_tool_id(&tool_call.tool_id) {
        return true;
    }

    if tool_id_matches(tool_call, &["agent.spawn", "task"]) {
        return true;
    }

    if tool_id_matches(tool_call, &["background_output"]) {
        return true;
    }

    matches!(
        tool_call.effective_tool_id(),
        "edit.hashline_apply" | "fs.write" | "edit" | "apply_patch"
    ) && tool_call_has_preview_content(tool_call)
}

pub(super) fn tool_call_has_transcript_disclosure(tool_call: &ToolCallEntry) -> bool {
    if tool_call.effective_tool_id() == "apply_patch" {
        return false;
    }

    if matches!(
        tool_call.effective_tool_id(),
        "fs.read" | "read" | "fs.glob" | "glob" | "fs.grep" | "grep" | "fs.ls" | "list"
    ) {
        return false;
    }

    if tool_output_hidden_behind_disclosure_by_default(tool_call) {
        return true;
    }

    let shell_output = shell_tool_output(tool_call);
    let output = tool_call.output_summary.as_deref().unwrap_or_default();
    let output_line_count = output.lines().count();
    !tool_call.artifact_refs.is_empty()
        || match tool_call.effective_tool_id() {
            "shell.run" | "bash" => shell_output
                .as_deref()
                .or(tool_call.output_summary.as_deref())
                .is_some_and(has_trimmed_content),
            "edit.hashline_apply" => tool_call_has_preview_content(tool_call),
            "agent.spawn" | "task" => true,
            _ => has_trimmed_content(output) && output_line_count > 3,
        }
}

pub(super) fn tool_disclosure_state(
    tool_call: &ToolCallEntry,
    tool_output_expanded: bool,
) -> Option<TranscriptToolCallDisclosureState> {
    tool_call_has_transcript_disclosure(tool_call).then_some(if tool_output_expanded {
        TranscriptToolCallDisclosureState::Expanded
    } else {
        TranscriptToolCallDisclosureState::Collapsed
    })
}

pub(super) fn tool_header_disclosure_glyph(
    disclosure_state: Option<TranscriptToolCallDisclosureState>,
) -> Option<&'static str> {
    match disclosure_state {
        Some(TranscriptToolCallDisclosureState::Collapsed) => Some("▸"),
        Some(TranscriptToolCallDisclosureState::Expanded) => Some("▾"),
        None => None,
    }
}

fn tool_output_hidden_behind_disclosure_by_default(tool_call: &ToolCallEntry) -> bool {
    tool_call.status == ToolCallDisplayStatus::Succeeded
        && is_mcp_tool_id(tool_call.effective_tool_id())
        && tool_call
            .output_summary
            .as_deref()
            .is_some_and(has_trimmed_content)
}
