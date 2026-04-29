use serde_json::Value;

use harness_core::event::{EventV1, ToolCallMetadata};

use super::{AppState, Focus, ToolCallDisplayStatus, ToolCallEntry};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalPanelStatus {
    PendingPermission,
    Queued,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalPanelEntry {
    pub tool_call_id: String,
    pub command: String,
    pub cwd: Option<String>,
    pub status: TerminalPanelStatus,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub exit_code: Option<i64>,
    pub success: Option<bool>,
    pub duration_ms: Option<u64>,
    pub truncated: bool,
    pub output_artifact: Option<String>,
    pub first_seq: u64,
    pub last_seq: u64,
}

impl TerminalPanelStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::PendingPermission => "pending permission",
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }
}

impl From<ToolCallDisplayStatus> for TerminalPanelStatus {
    fn from(status: ToolCallDisplayStatus) -> Self {
        match status {
            ToolCallDisplayStatus::PendingPermission => Self::PendingPermission,
            ToolCallDisplayStatus::Queued => Self::Queued,
            ToolCallDisplayStatus::Running => Self::Running,
            ToolCallDisplayStatus::Succeeded => Self::Succeeded,
            ToolCallDisplayStatus::Failed => Self::Failed,
        }
    }
}

impl AppState {
    pub(crate) fn terminal_panel_visible(&self) -> bool {
        self.terminal_panel_visible
    }

    pub(crate) fn terminal_panel_follow(&self) -> bool {
        self.terminal_panel_follow
    }

    pub(crate) fn terminal_panel_scroll(&self) -> usize {
        self.terminal_panel_scroll
    }

    pub(crate) fn toggle_terminal_panel(&mut self) {
        self.terminal_panel_visible = !self.terminal_panel_visible;
        if !self.terminal_panel_visible {
            self.terminal_panel_scroll = 0;
            self.terminal_panel_follow = true;
            if self.focus == Focus::Terminal {
                self.focus = Focus::Details;
            }
        }
    }

    pub(crate) fn terminal_panel_surface_active(&self) -> bool {
        self.terminal_panel_visible
            && self.focus == Focus::Terminal
            && self.active_review_surface.is_none()
            && !self.startup_shell_visible()
    }

    pub(crate) fn terminal_panel_entries(&self) -> Vec<TerminalPanelEntry> {
        self.activities
            .iter()
            .flat_map(|activity| activity.tool_calls.iter())
            .filter_map(terminal_panel_entry_from_tool_call)
            .collect()
    }

    pub(in crate::app) fn scroll_terminal_panel_up(&mut self, amount: u16) {
        self.terminal_panel_follow = false;
        self.terminal_panel_scroll = self
            .terminal_panel_scroll
            .saturating_add(usize::from(amount.max(1)));
    }

    pub(in crate::app) fn scroll_terminal_panel_down(&mut self, amount: u16) {
        self.terminal_panel_scroll = self
            .terminal_panel_scroll
            .saturating_sub(usize::from(amount.max(1)));
        if self.terminal_panel_scroll == 0 {
            self.terminal_panel_follow = true;
        }
    }
}

pub(in crate::app) fn terminal_panel_event_is_shell(payload: &EventV1) -> bool {
    match payload {
        EventV1::ToolCallRequested(data) => shell_tool_ids_match([
            Some(data.tool_id.as_str()),
            tool_metadata_canonical(&data.metadata),
        ]),
        EventV1::ToolCallFinished(data) => {
            shell_tool_ids_match([tool_metadata_canonical(&data.metadata)])
        }
        _ => false,
    }
}

fn terminal_panel_entry_from_tool_call(tool_call: &ToolCallEntry) -> Option<TerminalPanelEntry> {
    if !is_shell_tool_call(tool_call) {
        return None;
    }

    let command = shell_command(tool_call).unwrap_or_else(|| "shell".to_string());
    let cwd = shell_cwd(tool_call);
    let stdout = shell_output_field(tool_call, "stdout");
    let stderr = shell_output_field(tool_call, "stderr");
    let output_summary = tool_call
        .output_summary
        .as_ref()
        .map(|summary| summary.to_string());
    let (stdout, stderr) = match (stdout, stderr, output_summary) {
        (None, None, Some(summary)) if tool_call.status == ToolCallDisplayStatus::Failed => {
            (None, Some(summary))
        }
        (None, None, Some(summary)) => (Some(summary), None),
        (stdout, stderr, _) => (stdout, stderr),
    };

    Some(TerminalPanelEntry {
        tool_call_id: tool_call.tool_call_id.clone(),
        command,
        cwd,
        status: tool_call.status.into(),
        stdout: non_empty(stdout),
        stderr: non_empty(stderr),
        exit_code: shell_output_i64(tool_call, "status"),
        success: shell_output_bool(tool_call, "success"),
        duration_ms: tool_call.duration_ms(),
        truncated: shell_output_bool(tool_call, "truncated").unwrap_or(false),
        output_artifact: shell_output_artifact(tool_call),
        first_seq: tool_call.first_seq,
        last_seq: tool_call.last_seq,
    })
}

fn is_shell_tool_call(tool_call: &ToolCallEntry) -> bool {
    shell_tool_ids_match([
        Some(tool_call.tool_id.as_str()),
        Some(tool_call.invoked_tool_id()),
        Some(tool_call.effective_tool_id()),
        Some(tool_call.canonical_tool_id()),
    ])
}

fn shell_tool_ids_match<'a>(tool_ids: impl IntoIterator<Item = Option<&'a str>>) -> bool {
    tool_ids
        .into_iter()
        .flatten()
        .any(|tool_id| matches!(tool_id, "bash" | "shell.run"))
}

fn tool_metadata_canonical(metadata: &Option<ToolCallMetadata>) -> Option<&str> {
    metadata
        .as_ref()
        .and_then(|metadata| metadata.canonical_tool_id.as_deref())
}

fn shell_command(tool_call: &ToolCallEntry) -> Option<String> {
    shell_json_string(tool_call.output_json.as_ref(), &["command"])
        .or_else(|| shell_json_command_from_cmd(tool_call.output_json.as_ref()))
        .or_else(|| {
            serde_json::from_str::<Value>(&tool_call.args_summary)
                .ok()
                .and_then(|value| {
                    shell_json_string(Some(&value), &["command"])
                        .or_else(|| shell_json_command_from_cmd(Some(&value)))
                })
        })
}

fn shell_cwd(tool_call: &ToolCallEntry) -> Option<String> {
    shell_json_string(tool_call.output_json.as_ref(), &["workdir", "cwd"]).or_else(|| {
        serde_json::from_str::<Value>(&tool_call.args_summary)
            .ok()
            .and_then(|value| shell_json_string(Some(&value), &["workdir", "cwd"]))
    })
}

fn shell_json_command_from_cmd(value: Option<&Value>) -> Option<String> {
    let cmd = shell_json_string(value, &["cmd"])?;
    let args = value
        .and_then(|value| value.get("args"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .filter(|item| !item.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if args.is_empty() {
        Some(cmd)
    } else {
        Some(format!("{cmd} {}", args.join(" ")))
    }
}

fn shell_output_field(tool_call: &ToolCallEntry, key: &str) -> Option<String> {
    tool_call
        .output_json
        .as_ref()?
        .get(key)?
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn shell_output_i64(tool_call: &ToolCallEntry, key: &str) -> Option<i64> {
    tool_call.output_json.as_ref()?.get(key)?.as_i64()
}

fn shell_output_bool(tool_call: &ToolCallEntry, key: &str) -> Option<bool> {
    tool_call.output_json.as_ref()?.get(key)?.as_bool()
}

fn shell_output_artifact(tool_call: &ToolCallEntry) -> Option<String> {
    tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("output_artifact"))
        .and_then(|artifact| artifact.get("path"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            tool_call
                .artifact_refs
                .first()
                .map(|artifact| artifact.path.clone())
        })
}

fn shell_json_string(value: Option<&Value>, keys: &[&str]) -> Option<String> {
    let value = value?;
    keys.iter()
        .filter_map(|key| value.get(*key))
        .find_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}
