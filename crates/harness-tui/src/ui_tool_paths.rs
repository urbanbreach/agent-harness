use crate::app::ToolCallEntry;
use crate::text::{collapse_inline_whitespace, has_trimmed_content};

use super::ui_tool_metadata::tool_summary_number;

pub(super) fn tool_path_color(theme: &crate::theme::Theme) -> ratatui::style::Color {
    use ratatui::style::Color;
    if theme.text.primary == Color::Reset {
        return Color::Reset;
    }
    crate::theme::quantize_color(
        if theme.is_dark() {
            Color::Rgb(255, 158, 100)
        } else {
            Color::Rgb(195, 105, 30)
        },
        theme.color_level(),
    )
}

pub(super) fn tool_header_path(path: &str, expanded: bool) -> String {
    let path = std::path::Path::new(path);
    let relative = !path.is_absolute()
        && !path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir));
    let display = if expanded && relative {
        path.to_string_lossy()
    } else {
        path.file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
    };
    super::ui_tool_output::safe_tool_text(&display)
}

/// Search scopes keep their relative path, shortening leading components only
/// when the query and result summary leave too little space.
pub(super) fn shorten_search_path(path: &str, budget: usize) -> String {
    use super::ui_chrome::{display_width, truncate_plain_text};
    use unicode_segmentation::UnicodeSegmentation;

    if budget == 0 {
        return String::new();
    }
    if display_width(path) <= budget {
        return path.to_string();
    }
    let mut parts = path.split('/').map(str::to_string).collect::<Vec<_>>();
    for index in 0..parts.len().saturating_sub(1) {
        if display_width(&parts.join("/")) <= budget {
            break;
        }
        parts[index] = parts[index]
            .graphemes(true)
            .next()
            .unwrap_or_default()
            .to_string();
    }
    let shortened = parts.join("/");
    if display_width(&shortened) <= budget {
        return shortened;
    }
    for (index, _) in path.match_indices('/') {
        let tail = format!("…{}", &path[index..]);
        if display_width(&tail) <= budget {
            return tail;
        }
    }
    truncate_plain_text(path, budget)
}

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

pub(super) fn read_tool_input_suffix(tool_call: &ToolCallEntry) -> String {
    let display = tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.pointer("/metadata/display"));
    let offset = tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("offset"))
        .and_then(serde_json::Value::as_u64)
        .or_else(|| tool_summary_number(&tool_call.args_summary, &["offset", "start_line"]))
        .or_else(|| {
            display
                .and_then(|value| value.get("lineStart"))
                .and_then(serde_json::Value::as_u64)
        });
    let limit = tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("limit"))
        .and_then(serde_json::Value::as_u64)
        .or_else(|| tool_summary_number(&tool_call.args_summary, &["limit"]));
    let start = offset.unwrap_or(1);
    let total = tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("total_lines"))
        .or_else(|| display.and_then(|value| value.get("totalLines")))
        .and_then(serde_json::Value::as_u64);
    let end = display
        .and_then(|value| value.get("lineEnd"))
        .and_then(serde_json::Value::as_u64)
        .or_else(|| {
            limit
                .filter(|limit| *limit > 0)
                .and_then(|limit| start.checked_add(limit - 1))
        })
        .map(|end| total.map_or(end, |total| end.min(total)));
    match end {
        Some(end) if end >= start => match total {
            Some(total) if total > end.saturating_sub(start).saturating_add(1) => {
                format!("({start}-{end} of {total})")
            }
            _ => format!("({start}-{end})"),
        },
        _ => String::new(),
    }
}

pub(super) fn tool_match_count_description(tool_call: &ToolCallEntry) -> Option<String> {
    if tool_call.status == crate::app::ToolCallDisplayStatus::Failed {
        let files = matches!(tool_call.effective_tool_id(), "fs.glob" | "glob")
            || super::ui_tool_metadata::tool_summary_string(
                &tool_call.args_summary,
                &["output_mode"],
            )
            .is_some_and(|mode| mode == "files_with_matches");
        return Some(if files { "(no files)" } else { "(no matches)" }.into());
    }
    super::ui_recorded_tool_output::project(tool_call)
        .and_then(|output| output.search_summary())
        .or_else(|| {
            tool_match_count(tool_call)
                .map(|count| format!("{count} match{}", if count == 1 { "" } else { "es" }))
        })
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
