use crate::app::{ToolCallDisplayStatus, ToolCallEntry};
use crate::text::has_trimmed_content;

use super::ui_tool_diffs::tool_call_has_preview_content;
use super::ui_tool_paths::tool_id_matches;
use super::ui_tool_titles::is_mcp_tool_id;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TranscriptToolCallDisclosureState {
    Collapsed,
    Expanded,
}

pub(super) fn tool_hidden_from_transcript(tool_call: &ToolCallEntry) -> bool {
    tool_id_matches(tool_call, &["todo.read", "todoread", "todo.write", "todowrite"])
        // Streaming argument bytes have no canonical tool identity yet. Grok
        // keeps them in turn status until the actual ToolCall arrives.
        || (tool_call.tool_id == "tool"
            && tool_call.canonical_tool_id.is_none()
            && tool_call.args_digest.is_empty())
}

pub(super) fn tool_call_has_transcript_disclosure(tool_call: &ToolCallEntry) -> bool {
    if tool_output_is_viewer_only(tool_call) {
        return false;
    }
    if tool_call
        .hook_executions
        .iter()
        .any(|hook| hook.status != harness_core::event::HookExecutionStatus::Skipped)
    {
        return true;
    }
    if tool_call.effective_tool_id() == "apply_patch" {
        return false;
    }

    if matches!(tool_call.effective_tool_id(), "skill" | "skill.load") {
        return false;
    }

    if tool_output_hidden_behind_disclosure_by_default(tool_call) {
        return true;
    }

    let output = tool_call.output_summary.as_deref().unwrap_or_default();
    !tool_call.artifact_refs.is_empty()
        || match tool_call.effective_tool_id() {
            "shell.run" | "bash" => true,
            "edit.hashline_apply" => tool_call_has_preview_content(tool_call),
            "write" | "fs.write" | "edit" => {
                matches!(
                    tool_call.status,
                    ToolCallDisplayStatus::Succeeded | ToolCallDisplayStatus::Failed
                ) && (tool_call_has_preview_content(tool_call) || has_trimmed_content(output))
            }
            "agent.spawn" | "task" => false,
            _ => has_trimmed_content(output),
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

fn tool_output_hidden_behind_disclosure_by_default(tool_call: &ToolCallEntry) -> bool {
    tool_call.status == ToolCallDisplayStatus::Succeeded
        && is_mcp_tool_id(tool_call.effective_tool_id())
        && tool_call
            .output_summary
            .as_deref()
            .is_some_and(has_trimmed_content)
}

pub(crate) fn tool_error_is_viewer_only(tool: &ToolCallEntry) -> bool {
    tool_error_hidden_inline(tool)
        && tool
            .hook_executions
            .iter()
            .all(|hook| hook.status == harness_core::event::HookExecutionStatus::Skipped)
}

pub(crate) fn tool_output_is_viewer_only(tool: &ToolCallEntry) -> bool {
    super::ui_tool_metadata::read_media_mime(tool).is_some() || tool_error_is_viewer_only(tool)
}

pub(super) fn tool_error_hidden_inline(tool: &ToolCallEntry) -> bool {
    tool.status == ToolCallDisplayStatus::Failed
        && (super::ui_tool_titles::generic_tool_id(tool.effective_tool_id())
            || matches!(
                tool.effective_tool_id(),
                "fs.read"
                    | "read"
                    | "fs.glob"
                    | "glob"
                    | "fs.grep"
                    | "grep"
                    | "fs.ls"
                    | "list"
                    | "search.web"
                    | "websearch"
                    | "web.fetch"
                    | "webfetch"
            ))
}
