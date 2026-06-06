use std::collections::BTreeSet;

use crate::app::{ToolCallDisplayStatus, ToolCallEntry};
use crate::text::collapse_inline_whitespace;

use super::ui_secondary::format_detail_payload;
use super::ui_tool_metadata::{tool_json_nested_string, tool_json_string};
use super::ui_transcript::{TranscriptToolCallDetailBlock, TranscriptToolCallDetailTone};

const DEFAULT_TOOL_ERROR_BODY: &str = "No error details available.";

#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolErrorDisplay {
    subtitle: String,
    body: String,
}

pub(super) fn tool_call_denied(tool_call: &ToolCallEntry) -> bool {
    tool_call.permissions.iter().any(|permission| {
        permission.resolved_decision == Some(harness_core::event::PermissionDecision::Deny)
    })
}

pub(super) fn tool_error_subtitle(tool_call: &ToolCallEntry) -> Option<String> {
    tool_error_display(tool_call).map(|display| display.subtitle)
}

pub(super) fn tool_error_text(tool_call: &ToolCallEntry) -> Option<String> {
    tool_error_display(tool_call).map(|display| display.body)
}

pub(super) fn push_failed_tool_error_block(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    tool_call: &ToolCallEntry,
) {
    let Some(error) = tool_error_text(tool_call) else {
        return;
    };
    if detail_blocks_surface_error(detail_blocks, &error) {
        return;
    }
    detail_blocks.push(TranscriptToolCallDetailBlock::Message {
        text: error,
        tone: TranscriptToolCallDetailTone::Error,
    });
}

fn detail_blocks_surface_error(blocks: &[TranscriptToolCallDetailBlock], error: &str) -> bool {
    let error = error.trim();
    blocks.iter().any(|block| match block {
        TranscriptToolCallDetailBlock::Message { text, tone } => {
            *tone == TranscriptToolCallDetailTone::Error && text.trim() == error
        }
        TranscriptToolCallDetailBlock::BashPanel { output, .. } => output.trim() == error,
        TranscriptToolCallDetailBlock::FileSection(section) => {
            detail_blocks_surface_error(&section.detail_blocks, error)
        }
        TranscriptToolCallDetailBlock::TodoList { .. }
        | TranscriptToolCallDetailBlock::StructuredDiff { .. } => false,
    })
}

fn tool_error_display(tool_call: &ToolCallEntry) -> Option<ToolErrorDisplay> {
    if tool_call.status != ToolCallDisplayStatus::Failed {
        return None;
    }

    let formatted = raw_tool_error_text(tool_call)
        .map(|text| format_detail_payload(&text))
        .unwrap_or_default();
    let stripped_error = strip_prefix_ignore_ascii_case(formatted.trim(), "Error:")
        .map(str::trim)
        .unwrap_or_else(|| formatted.trim());
    let cleaned = strip_tool_error_prefix(tool_call, stripped_error);
    let default_subtitle = default_tool_error_subtitle(tool_call);

    if cleaned.is_empty() {
        return Some(ToolErrorDisplay {
            subtitle: default_subtitle.to_string(),
            body: DEFAULT_TOOL_ERROR_BODY.to_string(),
        });
    }

    if let Some((subtitle, body)) = split_tool_error_subtitle_and_body(&cleaned) {
        let subtitle = if tool_call_denied(tool_call) {
            default_subtitle.to_string()
        } else {
            subtitle
        };
        return Some(ToolErrorDisplay { subtitle, body });
    }

    Some(ToolErrorDisplay {
        subtitle: default_subtitle.to_string(),
        body: cleaned,
    })
}

fn default_tool_error_subtitle(tool_call: &ToolCallEntry) -> &'static str {
    if tool_call_denied(tool_call) {
        "Denied"
    } else {
        "Failed"
    }
}

fn raw_tool_error_text(tool_call: &ToolCallEntry) -> Option<String> {
    tool_call
        .permissions
        .iter()
        .find_map(|permission| {
            (permission.resolved_decision == Some(harness_core::event::PermissionDecision::Deny))
                .then_some(permission.resolution_reason.as_deref())
                .flatten()
                .map(str::trim)
                .filter(|reason| !reason.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            tool_call
                .output_summary
                .as_deref()
                .map(str::trim)
                .filter(|summary| !summary.is_empty())
                .map(str::to_string)
        })
        .or_else(|| tool_json_nested_string(tool_call.output_json.as_ref(), &["error", "message"]))
        .or_else(|| {
            tool_json_string(
                tool_call.output_json.as_ref(),
                &[
                    "error",
                    "message",
                    "detail",
                    "reason",
                    "result_summary",
                    "summary",
                ],
            )
        })
        .or_else(|| {
            tool_call
                .output_json
                .as_ref()
                .and_then(|value| serde_json::to_string_pretty(value).ok())
        })
}

fn strip_tool_error_prefix(tool_call: &ToolCallEntry, text: &str) -> String {
    let trimmed = text.trim();
    for prefix in tool_error_prefix_candidates(tool_call) {
        for separator in [" ", ": ", " - ", " — "] {
            let prefixed = format!("{prefix}{separator}");
            if let Some(stripped) = strip_prefix_ignore_ascii_case(trimmed, &prefixed) {
                return stripped.trim().to_string();
            }
        }
    }
    trimmed.to_string()
}

fn tool_error_prefix_candidates(tool_call: &ToolCallEntry) -> Vec<String> {
    let mut candidates = BTreeSet::new();
    for candidate in [
        Some(tool_call.tool_id.as_str()),
        Some(tool_call.effective_tool_id()),
        Some(tool_call.invoked_tool_id()),
        tool_call.resolved_canonical_tool_id(),
        tool_call.resolved_alias_source_tool_id(),
    ]
    .into_iter()
    .flatten()
    {
        let normalized = collapse_inline_whitespace(candidate);
        if !normalized.is_empty() {
            candidates.insert(normalized);
        }
        for alias in tool_error_aliases(candidate) {
            candidates.insert(alias.to_string());
        }
    }
    candidates.into_iter().collect()
}

fn tool_error_aliases(tool_id: &str) -> &'static [&'static str] {
    match tool_id {
        "fs.read" | "read" => &["read"],
        "fs.ls" | "list" => &["list"],
        "fs.glob" | "glob" => &["glob"],
        "fs.grep" | "grep" => &["grep"],
        "agent.spawn" | "task" => &["task"],
        "web.fetch" | "webfetch" => &["webfetch"],
        "search.web" | "websearch" => &["websearch"],
        "search.code" | "codesearch" => &["codesearch"],
        "shell.run" | "bash" => &["bash"],
        "apply_patch" => &["apply_patch"],
        "user.question" | "question" => &["question"],
        _ => &[],
    }
}

fn strip_prefix_ignore_ascii_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    text.get(..prefix.len())
        .filter(|candidate| candidate.eq_ignore_ascii_case(prefix))
        .map(|_| &text[prefix.len()..])
}

fn split_tool_error_subtitle_and_body(text: &str) -> Option<(String, String)> {
    let (subtitle, body) = text.split_once(": ")?;
    let subtitle = subtitle.trim();
    let body = body.trim();
    if !is_tool_error_label(subtitle) || body.is_empty() {
        return None;
    }

    Some((subtitle.to_string(), body.to_string()))
}

fn is_tool_error_label(subtitle: &str) -> bool {
    let lowered = subtitle.trim().to_ascii_lowercase();
    if lowered.is_empty()
        || lowered.contains('\n')
        || lowered.chars().count() > 48
        || !lowered
            .chars()
            .any(|character| character.is_ascii_alphabetic())
        || lowered.chars().any(|character| {
            matches!(
                character,
                '/' | '\\' | '{' | '}' | '[' | ']' | '(' | ')' | '"' | '\'' | '='
            )
        })
    {
        return false;
    }

    [
        "failed",
        "error",
        "denied",
        "limited",
        "timeout",
        "dismissed",
        "not found",
        "invalid",
        "forbidden",
        "unauthorized",
        "unavailable",
        "missing",
        "blocked",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
}
