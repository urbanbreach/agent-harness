use crate::app::ToolCallEntry;
use crate::text::{collapse_inline_whitespace, has_trimmed_content};

use super::ui_tool_metadata::tool_summary_number;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptPathMetadata {
    pub(super) leaf: String,
    pub(super) parent: Option<String>,
}

pub(super) fn tool_call_path_metadata(path: Option<&str>) -> Option<TranscriptPathMetadata> {
    let path = path?.trim();
    if path.is_empty() {
        return None;
    }

    let path = collapse_inline_whitespace(path);
    let (parent, leaf) = path
        .rsplit_once('/')
        .map(|(parent, leaf)| (Some(parent.to_string()), leaf.to_string()))
        .unwrap_or((None, path));
    Some(TranscriptPathMetadata { leaf, parent })
}

pub(super) fn tool_path_display(tool_call: &ToolCallEntry) -> Option<String> {
    tool_call
        .edit_path_display()
        .map(|path| collapse_inline_whitespace(&path))
}

pub(super) fn tool_in_path_description(tool_call: &ToolCallEntry) -> Option<String> {
    tool_path_display(tool_call)
        .filter(|path| path != ".")
        .map(|path| format!("in {path}"))
}

pub(super) fn read_tool_input_suffix(tool_call: &ToolCallEntry) -> String {
    let offset = tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("offset"))
        .and_then(serde_json::Value::as_u64)
        .or_else(|| tool_summary_number(&tool_call.args_summary, &["offset", "start_line"]));
    let limit = tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("limit"))
        .and_then(serde_json::Value::as_u64)
        .or_else(|| tool_summary_number(&tool_call.args_summary, &["limit"]));
    let mut parts = Vec::new();
    if let Some(offset) = offset {
        parts.push(format!("offset={offset}"));
    }
    if let Some(limit) = limit {
        parts.push(format!("limit={limit}"));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" [{}]", parts.join(", "))
    }
}

pub(super) fn tool_match_count_description(tool_call: &ToolCallEntry) -> Option<String> {
    tool_match_count(tool_call)
        .map(|count| format!("{count} match{}", if count == 1 { "" } else { "es" }))
}

fn tool_match_count(tool_call: &ToolCallEntry) -> Option<u64> {
    tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("total_count").or_else(|| value.get("count")))
        .and_then(serde_json::Value::as_u64)
        .or_else(|| {
            tool_call.output_summary.as_deref().map(|output| {
                u64::try_from(
                    output
                        .lines()
                        .filter(|line| has_trimmed_content(line))
                        .count(),
                )
                .unwrap_or(0)
            })
        })
}

pub(super) fn search_result_count_suffix(
    tool_call: &ToolCallEntry,
    display_tool_id: &str,
) -> String {
    let count = tool_call
        .output_json
        .as_ref()
        .and_then(|value| {
            if display_tool_id == "search.web" {
                value.get("numResults")
            } else {
                value
                    .get("results")
                    .or_else(|| value.get("result_count"))
                    .or_else(|| value.get("numResults"))
            }
        })
        .and_then(serde_json::Value::as_u64);
    count
        .map(|count| format!(" ({count} result{})", if count == 1 { "" } else { "s" }))
        .unwrap_or_default()
}

pub(super) fn todo_write_tool_id(tool_id: &str) -> bool {
    matches!(tool_id, "todo.write" | "todowrite")
}

pub(super) fn tool_id_matches(tool_call: &ToolCallEntry, expected: &[&str]) -> bool {
    expected.contains(&tool_call.effective_tool_id())
        || expected.contains(&tool_call.tool_id.as_str())
}

pub(super) fn context_group_tool_id(tool_id: &str) -> bool {
    matches!(
        tool_id,
        "fs.read"
            | "read"
            | "fs.glob"
            | "glob"
            | "fs.grep"
            | "grep"
            | "fs.ls"
            | "list"
            | "skill"
            | "skill.load"
    )
}

pub(super) fn join_tool_subtitles(
    primary: Option<String>,
    secondary: Option<String>,
) -> Option<String> {
    match (primary, secondary) {
        (Some(primary), Some(secondary)) => Some(format!("{primary} · {secondary}")),
        (Some(primary), None) => Some(primary),
        (None, Some(secondary)) => Some(secondary),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::context_group_tool_id;

    #[test]
    fn context_groups_include_skill_aliases() {
        // arrange
        // act
        // Given: both shipped spellings of the skill loader.
        // When: the context-group classifier evaluates them.
        // Then: either spelling participates in compact context summaries.
        // assert
        assert!(context_group_tool_id("skill"));
        assert!(context_group_tool_id("skill.load"));
    }
}
