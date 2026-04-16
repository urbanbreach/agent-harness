use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

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
            if line.trim().is_empty() {
                continue;
            }

            let event: Value = serde_json::from_str(line)
                .map_err(|err| format!("events line {} is invalid JSON: {err}", idx + 1))?;
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
        "write" => args_json
            .as_ref()
            .and_then(|value| value.get("filePath"))
            .and_then(Value::as_str)
            .map(|path| path == canonical_path)
            .unwrap_or_else(|| args_summary.contains(canonical_path)),
        "read" | "edit.hashline_scan" | "edit.hashline_apply" => args_json
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
        ("write".to_string(), ToolFlowPhase::Requested),
        ("write".to_string(), ToolFlowPhase::Finished),
        ("read".to_string(), ToolFlowPhase::Requested),
        ("read".to_string(), ToolFlowPhase::Finished),
        ("edit.hashline_scan".to_string(), ToolFlowPhase::Requested),
        ("edit.hashline_scan".to_string(), ToolFlowPhase::Finished),
        ("edit.hashline_apply".to_string(), ToolFlowPhase::Requested),
        ("edit.hashline_apply".to_string(), ToolFlowPhase::Finished),
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
