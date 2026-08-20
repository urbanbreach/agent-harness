// allow: SIZE_OK — TUI tool path rendering (indivisible view model)
use std::path::Path;

use ratatui::style::Style;

use crate::app::{PermissionEntry, ToolCallEntry};
use crate::text::collapse_inline_whitespace;
use crate::theme::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptQuestionAnswerItem {
    pub(super) question: String,
    pub(super) answer: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptTodoItem {
    pub(super) content: String,
    pub(super) status: TranscriptTodoStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TranscriptTodoStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

pub(super) fn question_tool_title(
    tool_call: &ToolCallEntry,
    question_answers: &[TranscriptQuestionAnswerItem],
) -> String {
    let count = question_count_from_args(&tool_call.args_summary).unwrap_or(question_answers.len());
    match tool_call.status {
        crate::app::ToolCallDisplayStatus::Succeeded => format!(
            "Asked {count} question{}",
            if count == 1 { "" } else { "s" }
        ),
        crate::app::ToolCallDisplayStatus::Failed
        | crate::app::ToolCallDisplayStatus::PendingPermission
        | crate::app::ToolCallDisplayStatus::Queued
        | crate::app::ToolCallDisplayStatus::Running => {
            first_question_subject(tool_call, question_answers)
                .map(|question| format!("Ask {question}"))
                .unwrap_or_else(|| "Ask".to_string())
        }
    }
}

fn first_question_subject(
    tool_call: &ToolCallEntry,
    question_answers: &[TranscriptQuestionAnswerItem],
) -> Option<String> {
    let from_args = question_prompt_texts(&tool_call.args_summary);
    if let Some(question) = from_args
        .into_iter()
        .map(|question| collapse_inline_whitespace(&question))
        .find(|question| !question.is_empty())
    {
        return Some(question);
    }
    if let Some(question) = question_answers
        .iter()
        .map(|item| collapse_inline_whitespace(&item.question))
        .find(|question| !question.is_empty())
    {
        return Some(question);
    }
    tool_call.permissions.iter().find_map(|permission| {
        question_prompt_texts(&permission.summary)
            .into_iter()
            .map(|question| collapse_inline_whitespace(&question))
            .find(|question| !question.is_empty())
    })
}

pub(super) fn todo_items_from_tool_call(
    tool_call: &ToolCallEntry,
    session_path: Option<&Path>,
) -> Vec<TranscriptTodoItem> {
    todo_items_from_value(tool_call.output_json.as_ref())
        .or_else(|| todo_items_from_artifacts(tool_call, session_path))
        .or_else(|| todo_items_from_json_str(tool_call.output_summary.as_deref()?))
        .or_else(|| {
            serde_json::from_str::<serde_json::Value>(&tool_call.args_summary)
                .ok()
                .and_then(|value| todo_items_from_value(Some(&value)))
        })
        .unwrap_or_default()
}

pub(super) fn ordered_todo_items(items: &[TranscriptTodoItem]) -> Vec<&TranscriptTodoItem> {
    items.iter().collect()
}

fn todo_items_from_artifacts(
    tool_call: &ToolCallEntry,
    session_path: Option<&Path>,
) -> Option<Vec<TranscriptTodoItem>> {
    let session_path = session_path?;
    tool_call.artifact_refs.iter().find_map(|artifact| {
        if !(artifact.path.ends_with(".json") || artifact.path.ends_with(".txt")) {
            return None;
        }
        let path = Path::new(&artifact.path);
        if path.is_absolute()
            || path
                .components()
                .any(|component| component == std::path::Component::ParentDir)
        {
            return None;
        }
        std::fs::read_to_string(session_path.join(path))
            .ok()
            .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
            .and_then(|value| todo_items_from_value(Some(&value)))
    })
}

fn todo_items_from_value(value: Option<&serde_json::Value>) -> Option<Vec<TranscriptTodoItem>> {
    let value = value?;
    todo_array_from_value(value)
        .and_then(|todos| todo_items_from_array(todos))
        .or_else(|| todo_items_from_embedded_json_fields(value))
}

fn todo_items_from_array(todos: &[serde_json::Value]) -> Option<Vec<TranscriptTodoItem>> {
    let items = todos
        .iter()
        .filter_map(|todo| {
            let content = todo
                .get("content")
                .or_else(|| todo.get("text"))
                .or_else(|| todo.get("title"))
                .and_then(serde_json::Value::as_str)
                .map(collapse_inline_whitespace)
                .filter(|content| !content.is_empty())?;
            let status = todo
                .get("status")
                .or_else(|| todo.get("state"))
                .and_then(serde_json::Value::as_str)
                .map(TranscriptTodoStatus::from_value)
                .or_else(|| {
                    todo.get("done")
                        .and_then(serde_json::Value::as_bool)
                        .map(|done| {
                            if done {
                                TranscriptTodoStatus::Completed
                            } else {
                                TranscriptTodoStatus::Pending
                            }
                        })
                })
                .unwrap_or(TranscriptTodoStatus::Pending);
            Some(TranscriptTodoItem { content, status })
        })
        .collect::<Vec<_>>();
    (!items.is_empty()).then_some(items)
}

fn todo_items_from_embedded_json_fields(
    value: &serde_json::Value,
) -> Option<Vec<TranscriptTodoItem>> {
    ["output", "result"]
        .iter()
        .find_map(|field| value.get(field).and_then(serde_json::Value::as_str))
        .and_then(todo_items_from_json_str)
}

fn todo_items_from_json_str(value: &str) -> Option<Vec<TranscriptTodoItem>> {
    serde_json::from_str::<serde_json::Value>(value)
        .ok()
        .and_then(|value| todo_items_from_value(Some(&value)))
}

fn todo_array_from_value(value: &serde_json::Value) -> Option<&Vec<serde_json::Value>> {
    value
        .get("todos")
        .and_then(serde_json::Value::as_array)
        .or_else(|| value.as_array())
        .or_else(|| {
            value
                .get("structured_output")
                .and_then(todo_array_from_value)
        })
        .or_else(|| value.get("metadata").and_then(todo_array_from_value))
        .or_else(|| value.get("output").and_then(todo_array_from_value))
        .or_else(|| value.get("result").and_then(todo_array_from_value))
}

impl TranscriptTodoStatus {
    fn from_value(value: &str) -> Self {
        match value {
            "in_progress" => Self::InProgress,
            "completed" => Self::Completed,
            "cancelled" => Self::Cancelled,
            _ => Self::Pending,
        }
    }

    pub(super) fn checkbox_glyph(self, theme: &Theme) -> String {
        let glyphs = theme.live_shell.transcript_glyphs;
        match self {
            Self::Completed => format!("[{}]", glyphs.choice_checked),
            Self::InProgress => format!("[{}]", theme.live_shell.glyphs.streaming),
            Self::Cancelled | Self::Pending => "[ ]".to_string(),
        }
    }

    pub(super) fn style(self, theme: &Theme) -> Style {
        match self {
            Self::InProgress => Style::default().fg(theme.status.warning),
            Self::Cancelled | Self::Completed | Self::Pending => {
                Style::default().fg(theme.text.secondary)
            }
        }
    }

    pub(super) fn content_style(self, theme: &Theme) -> Style {
        self.style(theme)
    }
}

pub(super) fn resolved_question_answer_items(
    tool_call: &ToolCallEntry,
) -> Vec<TranscriptQuestionAnswerItem> {
    tool_call
        .permissions
        .iter()
        .flat_map(question_answer_items_from_permission)
        .collect()
}

fn question_answer_items_from_permission(
    permission: &PermissionEntry,
) -> Vec<TranscriptQuestionAnswerItem> {
    if permission.resolved_decision != Some(harness_core::event::PermissionDecision::Allow)
        || !question_permission_kind(&permission.kind)
    {
        return Vec::new();
    }

    let questions = question_prompt_texts(&permission.summary);
    if questions.is_empty() {
        return Vec::new();
    }

    let answers = permission
        .resolution_reason
        .as_deref()
        .and_then(|reason| serde_json::from_str::<Vec<Vec<String>>>(reason).ok())
        .unwrap_or_default();

    questions
        .into_iter()
        .enumerate()
        .map(|(index, question)| TranscriptQuestionAnswerItem {
            question,
            answer: answers
                .get(index)
                .map(|items| {
                    items
                        .iter()
                        .map(|item| item.trim())
                        .filter(|item| !item.is_empty())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .filter(|answer| !answer.is_empty())
                .unwrap_or_else(|| "(no answer)".to_string()),
        })
        .collect()
}

fn question_permission_kind(kind: &str) -> bool {
    kind.eq_ignore_ascii_case("question")
        || kind.eq_ignore_ascii_case("ask")
        || kind.eq_ignore_ascii_case("ask_user")
}

fn question_count_from_args(args_summary: &str) -> Option<usize> {
    serde_json::from_str::<serde_json::Value>(args_summary)
        .ok()?
        .get("questions")?
        .as_array()
        .map(Vec::len)
}

fn question_prompt_texts(summary: &str) -> Vec<String> {
    serde_json::from_str::<serde_json::Value>(summary)
        .ok()
        .and_then(|value| {
            value
                .get("questions")
                .and_then(serde_json::Value::as_array)
                .cloned()
        })
        .map(|questions| {
            questions
                .into_iter()
                .filter_map(|question| {
                    question
                        .get("question")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod glyph_tests {
    use super::*;
    use crate::theme::GlyphMode;

    #[test]
    fn completed_todo_uses_ascii_marker_in_legacy_mode() {
        // arrange
        // act
        let theme = Theme::harness_chat().with_glyph_mode(GlyphMode::Ascii);

        // assert
        assert_eq!(
            TranscriptTodoStatus::Completed.checkbox_glyph(&theme),
            "[x]"
        );
    }
}
