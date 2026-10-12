use super::BashTool;
use harness_core::{
    coord::{FileEvent, MonitorLaunch, PreparedMonitor},
    tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult},
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{path::PathBuf, time::Duration};

pub(super) struct MonitorTool {
    pub(super) bash: BashTool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Args {
    /// `create` (default) starts a monitor; `rearm` resumes paused monitors.
    #[serde(default)]
    action: Action,
    /// Short label for what is being watched; required for create.
    #[schemars(length(min = 1, max = 200))]
    description: Option<String>,
    /// Shell command to run and watch, validated like bash. Set this or `path`.
    command: Option<String>,
    /// File to watch instead of a command.
    path: Option<PathBuf>,
    /// With `path`: `create` (default) fires when the file appears, `modify` when an
    /// existing file changes.
    event: Option<Event>,
    /// With `command`: Rust regex (unanchored, case-sensitive) a line must match to be
    /// reported. Without it every non-empty line is reported.
    filter: Option<String>,
    /// Kill the monitor after this many milliseconds; default 300000.
    #[schemars(range(min = 1, max = 3_600_000))]
    timeout_ms: Option<u64>,
    /// With `rearm`: the monitor to resume; omit to resume all paused monitors.
    task_id: Option<String>,
}

#[derive(Deserialize, JsonSchema, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Action {
    #[default]
    Create,
    Rearm,
}

#[derive(Deserialize, JsonSchema, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Event {
    Create,
    Modify,
}

fn invalid(message: impl Into<String>) -> ToolError {
    ToolError::InvalidArguments(message.into())
}

#[async_trait::async_trait]
impl Tool for MonitorTool {
    fn id(&self) -> &str {
        "monitor"
    }
    fn description(&self) -> &str {
        "Watch a command's output or a file in the background and get notified when something relevant happens, instead of sleeping or polling. A command monitor runs in a terminal (stdout and stderr merged) from the workspace root; every complete output line matching `filter` is delivered to you as a notification, collected over 2 seconds, at most once per 5 seconds per monitor and 50 lines per notification, and its exit is reported too. A file monitor reports once when `path` is created or, with `event: modify`, when the existing file changes, then ends. Returns a task_id immediately: get_command_or_subagent_output reads the full output and kill_command_or_subagent stops it. After 5 notifications in a row with no tool call or user message between them, the monitors sending lines pause; resume them with `action: \"rearm\"`. Commands are validated and permitted like bash. Default timeout 5 minutes, at most 1 hour."
    }
    fn parameters_json_schema(&self) -> Value {
        schemars::schema_for!(Args).to_value()
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }
    fn permission_requests(&self, value: &Value) -> Vec<(String, String)> {
        if let Some(command) = value.get("command").and_then(Value::as_str) {
            self.bash
                .permission_requests(&json!({ "command": command }))
        } else if let Some(path) = value.get("path").and_then(Value::as_str) {
            vec![("read".into(), path.into())]
        } else {
            vec![("task".into(), "monitor rearm".into())]
        }
    }
    fn permission_always_patterns(&self, value: &Value) -> Vec<String> {
        value
            .get("command")
            .and_then(Value::as_str)
            .map(|command| {
                self.bash
                    .permission_always_patterns(&json!({ "command": command }))
            })
            .unwrap_or_default()
    }
    fn filesystem_paths(&self, value: &Value) -> Result<Vec<PathBuf>, ToolError> {
        if let Some(command) = value.get("command").and_then(Value::as_str) {
            self.bash.filesystem_paths(&json!({ "command": command }))
        } else {
            Ok(value
                .get("path")
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .into_iter()
                .collect())
        }
    }
    async fn call(&self, context: ToolContext, value: Value) -> Result<ToolResult, ToolError> {
        let args: Args = serde_json::from_value(value).map_err(|e| invalid(e.to_string()))?;
        let coordinator = context.coordinator.clone();
        if args.action == Action::Rearm {
            let target = args.task_id.filter(|id| !id.trim().is_empty());
            let (resumed, dropped) = coordinator
                .rearm_monitors(context.actor.clone(), target)
                .await
                .map_err(|error| ToolError::Execution(error.to_string()))?;
            let text = match (resumed.len(), dropped) {
                (0, _) => "No paused monitor to resume.".to_owned(),
                (count, 0) => format!("Resumed {count} monitor(s)."),
                (count, dropped) => format!(
                    "Resumed {count} monitor(s); {dropped} matching lines were dropped while paused."
                ),
            };
            return Ok(ToolResult::structured(
                text,
                json!({"type": "MonitorRearm", "resumed": resumed, "dropped": dropped}),
            ));
        }
        let required = "create needs a description and a command or path";
        let description = args
            .description
            .as_deref()
            .map(str::trim)
            .filter(|description| !description.is_empty())
            .ok_or_else(|| invalid(required))?;
        if description.chars().count() > 200 {
            return Err(invalid("description must be at most 200 characters"));
        }
        let timeout = args.timeout_ms.unwrap_or(300_000);
        if !(1..=3_600_000).contains(&timeout) {
            return Err(invalid("timeout_ms must be between 1 and 3600000"));
        }
        let filter = args
            .filter
            .as_deref()
            .map(regex::Regex::new)
            .transpose()
            .map_err(|error| {
                invalid(format!("filter is not a valid regular expression: {error}"))
            })?;
        let command = args.command.as_deref().filter(|c| !c.trim().is_empty());
        let launch = match (command, &args.path) {
            (Some(_), Some(_)) => return Err(invalid("set command or path, not both")),
            (None, None) => return Err(invalid(required)),
            (Some(_), None) if args.event.is_some() => {
                return Err(invalid("event applies only to path monitors"));
            }
            (None, Some(_)) if filter.is_some() => {
                return Err(invalid("filter applies only to command monitors"));
            }
            (Some(source), None) => {
                let (mut command, scratch, _) = self.bash.command(&context, source, None)?;
                // Output goes to a log and the model, not a screen.
                command.env("TERM", "dumb").env("NO_COLOR", "1");
                MonitorLaunch::Command {
                    command: Box::new(command),
                    label: source.to_owned(),
                    scratch,
                }
            }
            (None, Some(path)) => {
                let path = context.resolve_workspace_path(path)?;
                if !path.parent().is_some_and(std::path::Path::is_dir) {
                    return Err(invalid("the parent directory of path must exist"));
                }
                let event = args.event.unwrap_or(Event::Create);
                match std::fs::symlink_metadata(&path) {
                    Ok(meta) if meta.file_type().is_symlink() => {
                        return Err(invalid("path must not be a symlink"));
                    }
                    Ok(meta) if !meta.is_file() => {
                        return Err(invalid("path must be a regular file"));
                    }
                    Err(_) if event == Event::Modify => {
                        return Err(invalid("modify needs an existing regular file"));
                    }
                    _ => {}
                }
                let event = match event {
                    Event::Create => FileEvent::Create,
                    Event::Modify => FileEvent::Modify,
                };
                MonitorLaunch::File { path, event }
            }
        };
        let file_event = match &launch {
            MonitorLaunch::File { event, .. } => Some(event.as_str()),
            MonitorLaunch::Command { .. } => None,
        };
        let (snapshot, notifications) = coordinator
            .start_monitor(
                context,
                PreparedMonitor {
                    launch,
                    description: description.to_owned(),
                    filter,
                    timeout: Duration::from_millis(timeout),
                },
            )
            .await
            .map_err(|error| ToolError::Execution(error.to_string()))?;
        let id = &snapshot.result.task_id;
        let text = if notifications {
            format!("Monitor started with task_id {id}. Matching lines and its exit arrive as notifications, so keep working instead of polling. Stop it with kill_command_or_subagent; get_command_or_subagent_output returns its full output.")
        } else {
            format!("Monitor started with task_id {id}. Notifications are off in this session, so its lines are not delivered: wait for it with wait_commands_or_subagents or read it with get_command_or_subagent_output.")
        };
        Ok(ToolResult::structured(
            text,
            json!({
                "type": "Monitor",
                "task_id": id,
                "description": description,
                "command": command,
                "path": args.path,
                "event": file_event,
                "filter": args.filter,
                "status": "running",
                "output_file": snapshot.result.output_file,
                "notifications": notifications,
            }),
        ))
    }
}
