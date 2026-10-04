use super::super::{ActivityEntry, LiveTurnPhase, SessionProjection, ToolCallEntry};
use serde_json::Value;

// Activity subjects use a 40-code-point limit. Terminal geometry still uses cells.
pub(crate) fn clamp_subject(subject: &str) -> String {
    subject
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_else(|| subject.trim())
        .chars()
        .take(40)
        .collect()
}

// Match the activity UI's scalar-width cutoff, including its treatment of ZWJ sequences.
pub(crate) fn truncate_width(text: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    let end_at = |budget| {
        let mut used = 0;
        for (offset, ch) in text.char_indices() {
            used += ch.width().unwrap_or(0);
            if used > budget {
                return offset;
            }
        }
        text.len()
    };
    if end_at(width) == text.len() {
        text.to_owned()
    } else if width == 0 {
        String::new()
    } else {
        format!("{}…", &text[..end_at(width - 1)])
    }
}

pub(crate) fn subject_label(subject: &str) -> String {
    let subject = clamp_subject(subject);
    if subject.is_empty() {
        "Waiting on task output…".into()
    } else {
        format!("{subject}…")
    }
}

pub(crate) fn writing_label(name: Option<&str>, ordinal: usize) -> String {
    let ordinal = if ordinal > 1 {
        format!(" ({ordinal})")
    } else {
        String::new()
    };
    let copy = match name {
        None => "Preparing tool call",
        Some("spawn_subagent" | "task" | "agent.spawn") => "Writing subagent prompt",
        Some("use_tool") => "Preparing MCP tool",
        Some("search_tool" | "search_tools") => "Searching MCP tools",
        Some("write" | "fs.write") => "Writing file",
        Some(
            "search_replace" | "edit" | "hashline_edit" | "apply_patch" | "edit.hashline_apply",
        ) => "Writing edit",
        Some("run_terminal_command" | "run_terminal_cmd" | "bash" | "shell.run") => {
            "Writing command"
        }
        Some("todo_write" | "todowrite" | "todo.write") => "Updating todo list",
        Some("workflow") => "Writing workflow",
        Some("send_feedback") => "Writing feedback draft",
        Some("image_gen" | "image_edit") => "Writing image prompt",
        Some("image_to_video" | "reference_to_video") => "Writing video prompt",
        Some("ask_user_question" | "question" | "user.question") => "Preparing question",
        Some(name) => {
            return format!(
                "Preparing {}{ordinal}…",
                clamp_subject(&pretty_tool_name(name))
            )
        }
    };
    format!("{copy}{ordinal}…")
}

pub(crate) fn pretty_tool_name(name: &str) -> String {
    let pair = name
        .split_once("__")
        .filter(|(server, action)| {
            !server.is_empty()
                && !action.is_empty()
                && !action.contains("__")
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_-.:".contains(c))
        })
        .or_else(|| {
            name.strip_prefix("mcp.")
                .and_then(|name| name.split_once('.'))
        });
    pair.map_or_else(
        || name.to_owned(),
        |(server, action)| {
            fn titleize(text: &str) -> String {
                text.split('_')
                    .map(|word| {
                        let mut chars = word.chars();
                        chars.next().map_or_else(String::new, |first| {
                            first.to_uppercase().chain(chars).collect()
                        })
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            }
            format!("({}) {}", titleize(server), titleize(action))
        },
    )
}

pub(crate) fn tool_activity(tool: &ToolCallEntry) -> (String, Option<String>) {
    let args = serde_json::from_str::<Value>(&tool.args_summary).ok();
    let description = args
        .as_ref()
        .and_then(|args| args.get("description"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|description| !description.is_empty())
        .map(clamp_subject);
    let title = args
        .as_ref()
        .and_then(|args| args.get("command"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| tool.effective_tool_id())
        .to_string();
    (title, description)
}

impl SessionProjection {
    pub(crate) fn reported_child_activity(&self, activity: &ActivityEntry) -> String {
        self.child_activity_for_phase(activity, self.reported_child_phase(activity))
    }

    pub(crate) fn child_activity(&self, activity: &ActivityEntry) -> String {
        self.child_activity_for_phase(activity, self.live_turn_phase(activity).0)
    }

    fn child_activity_for_phase(&self, activity: &ActivityEntry, phase: LiveTurnPhase) -> String {
        match phase {
            LiveTurnPhase::Waiting => "Waiting for response…".into(),
            LiveTurnPhase::WaitingFor(reason) => self.waiting_label(&reason, &activity.request_id),
            LiveTurnPhase::Compacting => "Compacting".into(),
            LiveTurnPhase::Thinking => "Thinking".into(),
            LiveTurnPhase::Responding => "Responding".into(),
            LiveTurnPhase::Retrying(attempt) => {
                let maximum = activity
                    .request_data
                    .as_ref()
                    .and_then(|request| request.metadata.as_ref())
                    .and_then(|metadata| metadata.retry)
                    .map_or(0, |retry| retry.max_attempts.saturating_sub(1));
                format!("Retrying ({attempt}/{maximum})")
            }
            LiveTurnPhase::WritingToolCall {
                ordinal, tool_name, ..
            } => writing_label(tool_name.as_deref(), ordinal),
            LiveTurnPhase::ToolRunning(id) => {
                let Some(tool) = activity
                    .tool_calls
                    .iter()
                    .find(|tool| tool.tool_call_id == id)
                else {
                    return "Running tool".into();
                };
                let (title, description) = tool_activity(tool);
                if let Some(description) = description {
                    return subject_label(&description);
                }
                if title.is_empty() {
                    return "Running tool".into();
                }
                let title = title.lines().next().unwrap_or(&title);
                format!(
                    "Running: {}{}",
                    title.chars().take(40).collect::<String>(),
                    if title.chars().count() > 40 {
                        "…"
                    } else {
                        ""
                    }
                )
            }
        }
    }
}

pub(crate) fn retry_label(activity: &ActivityEntry, attempt: u32) -> String {
    let retry = activity
        .request_data
        .as_ref()
        .and_then(|request| request.metadata.as_ref())
        .and_then(|metadata| metadata.retry);
    let headline = retry
        .and_then(|retry| {
            retry.failure.or_else(|| {
                retry.category.map(|category| {
                    harness_core::event::ProviderRetryFailure::from_error(Some(category), "")
                })
            })
        })
        .and_then(harness_core::event::ProviderRetryFailure::headline);
    let clause = format!("Retrying (attempt {attempt})...");
    headline.map_or_else(
        || clause.clone(),
        |headline| format!("{headline} | {clause}"),
    )
}
