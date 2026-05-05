use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::json_file::read_required_json;
use crate::{DEFAULT_LIVE_PROXY_VARIANT, LIVE_CHAT_TODO_CONTENT};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolFlowPhase {
    Requested,
    Finished,
}

impl ToolFlowPhase {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::Finished => "finished",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolFlowSequenceEvent {
    tool_id: String,
    phase: ToolFlowPhase,
    tool_call_id: String,
    status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RequestedToolCall {
    tool_id: String,
    same_file: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolFlowEvidence {
    workspace_root: PathBuf,
    canonical_relative_path: PathBuf,
    same_file_sequence: Vec<ToolFlowSequenceEvent>,
    run_failed: Option<String>,
    saw_edit_applied: bool,
    saw_edit_rejected: bool,
}

impl ToolFlowEvidence {
    pub(crate) fn collect(
        run_dir: &Path,
        workspace_root: &Path,
        canonical_relative_path: &Path,
    ) -> Result<Self, String> {
        let events_path = run_dir.join("events.jsonl");
        let events_body = fs::read_to_string(&events_path).map_err(|err| {
            format!(
                "failed to read tool-flow events {}: {err}",
                events_path.display()
            )
        })?;
        Self::from_events_jsonl(&events_body, workspace_root, canonical_relative_path)
    }

    pub(crate) fn collect_many(
        run_dirs: &[PathBuf],
        workspace_root: &Path,
        canonical_relative_path: &Path,
    ) -> Result<Self, String> {
        let mut run_dirs = run_dirs.iter();
        let Some(first_run_dir) = run_dirs.next() else {
            return Err("expected at least one tool-flow run dir".to_string());
        };

        let mut merged = Self::collect(first_run_dir, workspace_root, canonical_relative_path)?;
        for run_dir in run_dirs {
            let next = Self::collect(run_dir, workspace_root, canonical_relative_path)?;
            merged = merged.merge(next)?;
        }

        Ok(merged)
    }

    pub(crate) fn from_events_jsonl(
        events_body: &str,
        workspace_root: &Path,
        canonical_relative_path: &Path,
    ) -> Result<Self, String> {
        if canonical_relative_path.is_absolute() {
            return Err("canonical tool-flow path must be relative".to_string());
        }

        let canonical_path = canonical_relative_path.display().to_string();
        let mut requested_tool_calls = BTreeMap::new();
        let mut same_file_sequence = Vec::new();
        let mut run_failed = None;
        let mut saw_edit_applied = false;
        let mut saw_edit_rejected = false;

        for (idx, line) in events_body.lines().enumerate() {
            let Some(event) = parse_event_line(line, idx + 1)? else {
                continue;
            };
            let event_type = event
                .get("payload")
                .and_then(|payload| payload.get("event_type"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let data = event
                .get("payload")
                .and_then(|payload| payload.get("data"))
                .cloned()
                .unwrap_or(Value::Null);

            match event_type {
                "tool_call_requested" => {
                    let tool_call_id =
                        required_str(&data, "tool_call_id", event_type, idx + 1)?.to_string();
                    let tool_id = required_str(&data, "tool_id", event_type, idx + 1)?.to_string();
                    let args_summary = required_str(&data, "args_summary", event_type, idx + 1)?;
                    let same_file =
                        tool_targets_canonical_path(&tool_id, args_summary, &canonical_path);

                    requested_tool_calls.insert(
                        tool_call_id.clone(),
                        RequestedToolCall {
                            tool_id: tool_id.clone(),
                            same_file,
                        },
                    );

                    if same_file {
                        same_file_sequence.push(ToolFlowSequenceEvent {
                            tool_id,
                            phase: ToolFlowPhase::Requested,
                            tool_call_id,
                            status: None,
                        });
                    }
                }
                "tool_call_finished" => {
                    let tool_call_id = required_str(&data, "tool_call_id", event_type, idx + 1)?;
                    let status = required_str(&data, "status", event_type, idx + 1)?.to_string();

                    let Some(requested) = requested_tool_calls.get(tool_call_id) else {
                        continue;
                    };
                    if requested.same_file {
                        same_file_sequence.push(ToolFlowSequenceEvent {
                            tool_id: requested.tool_id.clone(),
                            phase: ToolFlowPhase::Finished,
                            tool_call_id: tool_call_id.to_string(),
                            status: Some(status),
                        });
                    }
                }
                "edit_applied" => {
                    if path_matches_event(&data, &canonical_path) {
                        saw_edit_applied = true;
                    }
                }
                "edit_rejected" => {
                    if path_matches_event(&data, &canonical_path) {
                        saw_edit_rejected = true;
                    }
                }
                "run_failed" => {
                    if run_failed.is_none() {
                        run_failed = data
                            .get("error")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned)
                            .or_else(|| Some("run_failed event missing error detail".to_string()));
                    }
                }
                _ => {}
            }
        }

        Ok(Self {
            workspace_root: workspace_root.to_path_buf(),
            canonical_relative_path: canonical_relative_path.to_path_buf(),
            same_file_sequence,
            run_failed,
            saw_edit_applied,
            saw_edit_rejected,
        })
    }

    pub(crate) fn assert_run_succeeded(&self) -> Result<(), String> {
        if let Some(run_failed) = self.run_failed.as_deref() {
            return Err(format!(
                "run failed before tool-flow verification completed: {run_failed}"
            ));
        }

        Ok(())
    }

    pub(crate) fn assert_ordered_same_file_sequence(&self) -> Result<(), String> {
        self.assert_run_succeeded()?;

        let expected = expected_same_file_sequence();
        let actual = self.actual_sequence_signature();
        if actual != expected {
            return Err(format!(
                "expected ordered same-file tool sequence {} for {}; found {}",
                format_sequence_signature(&expected),
                self.canonical_relative_path.display(),
                format_sequence_signature(&actual)
            ));
        }

        if let Some(step) = self.same_file_sequence.iter().find(|step| {
            step.phase == ToolFlowPhase::Finished && step.status.as_deref() != Some("succeeded")
        }) {
            return Err(format!(
                "expected {} finish for {} to succeed; found status `{}`",
                step.tool_id,
                step.tool_call_id,
                step.status.as_deref().unwrap_or("missing")
            ));
        }

        if self.saw_edit_rejected {
            return Err(format!(
                "did not expect edit_rejected for {}",
                self.canonical_relative_path.display()
            ));
        }
        if !self.saw_edit_applied {
            return Err(format!(
                "expected edit_applied for {}",
                self.canonical_relative_path.display()
            ));
        }

        Ok(())
    }

    pub(crate) fn assert_final_workspace_content(
        &self,
        expected_content: &str,
    ) -> Result<(), String> {
        let workspace_path = self.workspace_root.join(&self.canonical_relative_path);
        let actual_content = fs::read_to_string(&workspace_path).map_err(|err| {
            format!(
                "failed to read final tool-flow workspace file {}: {err}",
                workspace_path.display()
            )
        })?;

        if actual_content != expected_content {
            return Err(format!(
                "final workspace content mismatch for {}",
                workspace_path.display()
            ));
        }

        Ok(())
    }

    pub(crate) fn summary_json(&self, run_dirs: &[PathBuf]) -> Result<Value, String> {
        let workspace_path = self.workspace_root.join(&self.canonical_relative_path);
        let final_content = fs::read_to_string(&workspace_path).map_err(|err| {
            format!(
                "failed to read final tool-flow workspace file {}: {err}",
                workspace_path.display()
            )
        })?;

        Ok(serde_json::json!({
            "workspace_root": self.workspace_root.display().to_string(),
            "workspace_path": workspace_path.display().to_string(),
            "canonical_relative_path": self.canonical_relative_path.display().to_string(),
            "run_dirs": run_dirs.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
            "run_failed": self.run_failed.clone(),
            "saw_edit_applied": self.saw_edit_applied,
            "saw_edit_rejected": self.saw_edit_rejected,
            "same_file_sequence": self.same_file_sequence.iter().map(|step| {
                serde_json::json!({
                    "tool_id": step.tool_id.clone(),
                    "phase": step.phase.as_str(),
                    "tool_call_id": step.tool_call_id.clone(),
                    "status": step.status.clone(),
                })
            }).collect::<Vec<_>>(),
            "final_content": final_content,
        }))
    }

    pub(crate) fn sequence_summary_lines(&self) -> Vec<String> {
        self.same_file_sequence
            .iter()
            .map(|step| {
                let mut line = format!(
                    "{} {} ({})",
                    step.tool_id,
                    step.phase.as_str(),
                    step.tool_call_id
                );
                if let Some(status) = step.status.as_deref() {
                    line.push_str(&format!(" status={status}"));
                }
                line
            })
            .collect()
    }

    fn actual_sequence_signature(&self) -> Vec<(String, ToolFlowPhase)> {
        self.same_file_sequence
            .iter()
            .map(|step| (step.tool_id.clone(), step.phase))
            .collect()
    }

    fn merge(mut self, other: Self) -> Result<Self, String> {
        if self.workspace_root != other.workspace_root {
            return Err(format!(
                "cannot merge tool-flow evidence with different workspace roots: {} vs {}",
                self.workspace_root.display(),
                other.workspace_root.display()
            ));
        }
        if self.canonical_relative_path != other.canonical_relative_path {
            return Err(format!(
                "cannot merge tool-flow evidence with different canonical paths: {} vs {}",
                self.canonical_relative_path.display(),
                other.canonical_relative_path.display()
            ));
        }

        self.same_file_sequence.extend(other.same_file_sequence);
        if self.run_failed.is_none() {
            self.run_failed = other.run_failed;
        }
        self.saw_edit_applied |= other.saw_edit_applied;
        self.saw_edit_rejected |= other.saw_edit_rejected;
        Ok(self)
    }
}

pub(crate) fn resolve_tagged_run_dir(
    session_dir: &Path,
    session_namespace: &str,
) -> Result<PathBuf, String> {
    if session_namespace.trim().is_empty() {
        return Err("session namespace cannot be empty".to_string());
    }

    let mut run_dirs = fs::read_dir(session_dir)
        .map_err(|err| {
            format!(
                "failed to read session dir for `{session_namespace}` at {}: {err}",
                session_dir.display()
            )
        })?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir() && path.join("events.jsonl").exists())
        .collect::<Vec<_>>();
    run_dirs.sort();

    match run_dirs.len() {
        1 => Ok(run_dirs.remove(0)),
        0 => Err(format!(
            "expected one run dir with events.jsonl for `{session_namespace}` under {}; found none",
            session_dir.display()
        )),
        count => Err(format!(
            "expected one run dir with events.jsonl for `{session_namespace}` under {}; found {count}",
            session_dir.display()
        )),
    }
}

pub(crate) fn assert_requested_tool_args(
    events_body: &str,
    expected_tool_id: &str,
    expected_args: &Value,
) -> Result<(), String> {
    let args = first_requested_tool_args(events_body, expected_tool_id)?
        .ok_or_else(|| format!("expected requested args for `{expected_tool_id}`"))?;
    if &args == expected_args {
        Ok(())
    } else {
        Err(format!(
            "expected `{expected_tool_id}` args {} ; found {}",
            expected_args, args
        ))
    }
}

pub(crate) fn assert_tool_call_output_contains(
    events_body: &str,
    expected_tool_id: &str,
    needle: &str,
) -> Result<(), String> {
    let output = first_tool_call_output_summary(events_body, expected_tool_id)?
        .ok_or_else(|| format!("expected output summary for `{expected_tool_id}`"))?;
    if output.contains(needle) {
        Ok(())
    } else {
        Err(format!(
            "expected `{expected_tool_id}` output summary to contain `{needle}`; found `{output}`"
        ))
    }
}

pub(crate) fn assert_event_log_contains(events_body: &str, needle: &str) -> Result<(), String> {
    if events_body.contains(needle) {
        Ok(())
    } else {
        Err(format!("expected event log to contain `{needle}`"))
    }
}

pub(crate) fn assert_requested_tool_sequence(
    events_body: &str,
    expected_tools: &[&str],
) -> Result<(), String> {
    let mut requested = Vec::<(String, String)>::new();
    let mut finished = BTreeMap::<String, String>::new();

    for (idx, line) in events_body.lines().enumerate() {
        let Some(event) = parse_event_line(line, idx + 1)? else {
            continue;
        };
        let event_type = event
            .get("payload")
            .and_then(|payload| payload.get("event_type"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let data = event
            .get("payload")
            .and_then(|payload| payload.get("data"))
            .cloned()
            .unwrap_or(Value::Null);

        match event_type {
            "tool_call_requested" => {
                let Some(tool_id) = data.get("tool_id").and_then(Value::as_str) else {
                    continue;
                };
                let Some(tool_call_id) = data.get("tool_call_id").and_then(Value::as_str) else {
                    continue;
                };
                requested.push((tool_id.to_string(), tool_call_id.to_string()));
            }
            "tool_call_finished" => {
                let Some(tool_call_id) = data.get("tool_call_id").and_then(Value::as_str) else {
                    continue;
                };
                let status = data
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("missing");
                finished.insert(tool_call_id.to_string(), status.to_string());
            }
            _ => {}
        }
    }

    let actual_tools = requested
        .iter()
        .map(|(tool_id, _)| tool_id.as_str())
        .collect::<Vec<_>>();
    if actual_tools != expected_tools {
        return Err(format!(
            "expected requested tool sequence {:?}; found {:?}",
            expected_tools, actual_tools
        ));
    }

    for (tool_id, tool_call_id) in requested {
        let status = finished
            .get(&tool_call_id)
            .map(String::as_str)
            .unwrap_or("missing");
        if status != "succeeded" {
            return Err(format!(
                "expected `{tool_id}` ({tool_call_id}) to finish with status `succeeded`; found `{status}`"
            ));
        }
    }

    Ok(())
}

pub(crate) fn assert_run_records_live_runtime_context(
    run_dir: &Path,
    expected_profile: &str,
    expected_model: &str,
    expected_variant: Option<&str>,
) -> Result<(), String> {
    let meta_path = run_dir.join("meta.json");
    let meta = read_required_json(&meta_path)?;
    let context = meta
        .get("recorded_runtime_context")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            format!(
                "expected recorded_runtime_context in {}",
                meta_path.display()
            )
        })?;

    if context.get("profile").and_then(Value::as_str) != Some(expected_profile) {
        return Err(format!(
            "expected runtime context profile `{expected_profile}` in {}; found {:?}",
            meta_path.display(),
            context.get("profile")
        ));
    }
    if context.get("model").and_then(Value::as_str) != Some(expected_model) {
        return Err(format!(
            "expected runtime context model `{expected_model}` in {}; found {:?}",
            meta_path.display(),
            context.get("model")
        ));
    }
    if context.get("variant").and_then(Value::as_str) != expected_variant {
        return Err(format!(
            "expected runtime context variant {:?} in {}; found {:?}",
            expected_variant,
            meta_path.display(),
            context.get("variant")
        ));
    }
    if expected_variant == Some(DEFAULT_LIVE_PROXY_VARIANT)
        && context.get("reasoning_effort").and_then(Value::as_str) != Some("low")
    {
        return Err(format!(
            "expected runtime context reasoning_effort `low` in {}; found {:?}",
            meta_path.display(),
            context.get("reasoning_effort")
        ));
    }

    Ok(())
}

pub(crate) fn assert_todo_state_matches(run_dir: &Path) -> Result<(), String> {
    let todos_path = run_dir.join("control-plane").join("todos.json");
    let todos = read_required_json(&todos_path)?;
    let expected = json!([
        {
            "content": LIVE_CHAT_TODO_CONTENT,
            "status": "pending",
            "priority": "high",
        }
    ]);
    if todos == expected {
        Ok(())
    } else {
        Err(format!(
            "expected {} to equal {}; found {}",
            todos_path.display(),
            expected,
            todos
        ))
    }
}

pub(crate) fn assert_question_state_matches(
    run_dir: &Path,
    events_body: &str,
) -> Result<(), String> {
    let tool_call_id = first_requested_tool_call_id(events_body, "question")?
        .ok_or_else(|| "expected requested question tool_call_id".to_string())?;
    let question_path = run_dir
        .join("control-plane")
        .join("questions")
        .join(format!("{tool_call_id}.json"));
    let question_state = read_required_json(&question_path)?;
    let expected = json!([
        {
            "question": "Pick one",
            "header": "Choice",
            "multiple": Value::Null,
            "options": [
                {"label": "Yes", "description": "Choose yes"},
                {"label": "No", "description": "Choose no"}
            ]
        }
    ]);
    if question_state == expected {
        Ok(())
    } else {
        Err(format!(
            "expected {} to equal {}; found {}",
            question_path.display(),
            expected,
            question_state
        ))
    }
}

pub(crate) fn first_requested_tool_call_id(
    events_body: &str,
    expected_tool_id: &str,
) -> Result<Option<String>, String> {
    for (idx, line) in events_body.lines().enumerate() {
        let Some(event) = parse_event_line(line, idx + 1)? else {
            continue;
        };
        let event_type = event
            .get("payload")
            .and_then(|payload| payload.get("event_type"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        if event_type != "tool_call_requested" {
            continue;
        }
        let data = event
            .get("payload")
            .and_then(|payload| payload.get("data"))
            .cloned()
            .unwrap_or(Value::Null);
        if data.get("tool_id").and_then(Value::as_str) != Some(expected_tool_id) {
            continue;
        }
        if let Some(tool_call_id) = data.get("tool_call_id").and_then(Value::as_str) {
            return Ok(Some(tool_call_id.to_string()));
        }
    }

    Ok(None)
}

pub(crate) fn first_requested_tool_args(
    events_body: &str,
    expected_tool_id: &str,
) -> Result<Option<Value>, String> {
    for (idx, line) in events_body.lines().enumerate() {
        let line_number = idx + 1;
        let Some(event) = parse_event_line(line, line_number)? else {
            continue;
        };
        let event_type = event
            .get("payload")
            .and_then(|payload| payload.get("event_type"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        if event_type != "tool_call_requested" {
            continue;
        }
        let data = event
            .get("payload")
            .and_then(|payload| payload.get("data"))
            .cloned()
            .unwrap_or(Value::Null);
        if data.get("tool_id").and_then(Value::as_str) != Some(expected_tool_id) {
            continue;
        }
        if let Some(args_summary) = data.get("args_summary").and_then(Value::as_str) {
            let args = serde_json::from_str(args_summary).map_err(|err| {
                format!(
                    "failed to parse args_summary for `{expected_tool_id}` on line {}: {err}",
                    line_number
                )
            })?;
            return Ok(Some(args));
        }
    }

    Ok(None)
}

pub(crate) fn first_tool_call_output_summary(
    events_body: &str,
    expected_tool_id: &str,
) -> Result<Option<String>, String> {
    let tool_call_id = first_requested_tool_call_id(events_body, expected_tool_id)?;
    let Some(tool_call_id) = tool_call_id else {
        return Ok(None);
    };

    for (idx, line) in events_body.lines().enumerate() {
        let Some(event) = parse_event_line(line, idx + 1)? else {
            continue;
        };
        let event_type = event
            .get("payload")
            .and_then(|payload| payload.get("event_type"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        if event_type != "tool_call_finished" {
            continue;
        }
        let data = event
            .get("payload")
            .and_then(|payload| payload.get("data"))
            .cloned()
            .unwrap_or(Value::Null);
        if data.get("tool_call_id").and_then(Value::as_str) != Some(tool_call_id.as_str()) {
            continue;
        }
        return Ok(data
            .get("output_summary")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned));
    }

    Ok(None)
}

fn parse_event_line(line: &str, line_number: usize) -> Result<Option<Value>, String> {
    if line.trim().is_empty() {
        return Ok(None);
    }

    serde_json::from_str(line)
        .map(Some)
        .map_err(|err| format!("events line {line_number} is invalid JSON: {err}"))
}

fn required_str<'a>(
    data: &'a Value,
    field: &str,
    event_type: &str,
    line_number: usize,
) -> Result<&'a str, String> {
    data.get(field).and_then(Value::as_str).ok_or_else(|| {
        format!("events line {line_number} missing string field `{field}` for {event_type}")
    })
}

fn path_matches_event(data: &Value, canonical_path: &str) -> bool {
    data.get("path")
        .and_then(Value::as_str)
        .map(|path| path == canonical_path)
        .unwrap_or(false)
}

fn tool_targets_canonical_path(tool_id: &str, args_summary: &str, canonical_path: &str) -> bool {
    let args_json = serde_json::from_str::<Value>(args_summary).ok();

    match tool_id {
        "read" | "edit" => args_json
            .as_ref()
            .and_then(|value| value.get("path").or_else(|| value.get("filePath")))
            .and_then(Value::as_str)
            .map(|path| path == canonical_path)
            .unwrap_or_else(|| args_summary.contains(canonical_path)),
        "bash" => args_json
            .as_ref()
            .map(|value| json_value_contains_path(value, canonical_path))
            .unwrap_or_else(|| args_summary.contains(canonical_path)),
        _ => false,
    }
}

fn json_value_contains_path(value: &Value, canonical_path: &str) -> bool {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
        Value::String(text) => text.contains(canonical_path),
        Value::Array(items) => items
            .iter()
            .any(|item| json_value_contains_path(item, canonical_path)),
        Value::Object(map) => map
            .values()
            .any(|value| json_value_contains_path(value, canonical_path)),
    }
}

fn expected_same_file_sequence() -> Vec<(String, ToolFlowPhase)> {
    vec![
        ("edit".to_string(), ToolFlowPhase::Requested),
        ("edit".to_string(), ToolFlowPhase::Finished),
        ("read".to_string(), ToolFlowPhase::Requested),
        ("read".to_string(), ToolFlowPhase::Finished),
        ("edit".to_string(), ToolFlowPhase::Requested),
        ("edit".to_string(), ToolFlowPhase::Finished),
        ("read".to_string(), ToolFlowPhase::Requested),
        ("read".to_string(), ToolFlowPhase::Finished),
    ]
}

fn format_sequence_signature(sequence: &[(String, ToolFlowPhase)]) -> String {
    if sequence.is_empty() {
        return "<empty>".to_string();
    }

    sequence
        .iter()
        .map(|(tool_id, phase)| format!("{tool_id} {}", phase.as_str()))
        .collect::<Vec<_>>()
        .join(" -> ")
}
