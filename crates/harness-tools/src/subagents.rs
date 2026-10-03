//! Public native-reference adapters. The coordinator owns execution and admission.
//!
//! Registration supplies finalized schemas and descriptions from the resolved
//! config/catalog/definition snapshot, rather than guessing actor capabilities
//! here. The actor's spawn reply converts to `ToolResult`; output, wait, kill
//! and message replies retain the public wire types. Coordinator errors convert
//! to `ToolError` without losing native custom codes.

use harness_core::{
    subagent::{
        GetCommandOrSubagentOutputInput, GetCommandOrSubagentOutputResult,
        GetCommandOrSubagentOutputValue, KillCommandOrSubagentInput, KillCommandOrSubagentValue,
        SendSubagentMessageInput, SendSubagentMessageOutput, SpawnSubagentInput,
        WaitCommandsOrSubagentsInput,
    },
    tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult},
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

#[derive(Clone, Copy)]
pub enum SubagentOperation {
    Spawn,
    Output,
    Wait,
    Kill,
    Send,
}

impl SubagentOperation {
    fn id(self) -> &'static str {
        match self {
            Self::Spawn => "spawn_subagent",
            Self::Output => "get_command_or_subagent_output",
            Self::Wait => "wait_commands_or_subagents",
            Self::Kill => "kill_command_or_subagent",
            Self::Send => "send_subagent_message",
        }
    }

    fn alias(self) -> Option<&'static str> {
        match self {
            Self::Spawn => Some("task"),
            Self::Output => Some("get_task_output"),
            Self::Wait => Some("wait_tasks"),
            Self::Kill => Some("kill_task"),
            Self::Send => None,
        }
    }
}

#[derive(Clone)]
pub struct SubagentTool {
    operation: SubagentOperation,
    name: &'static str,
    schema: Value,
    description: String,
}

impl SubagentTool {
    pub(crate) fn new(operation: SubagentOperation, schema: Value, description: String) -> Self {
        Self {
            operation,
            name: operation.id(),
            schema,
            description,
        }
    }

    /// Reference-only aliases; old Harness continuation/history names are gone.
    pub(crate) fn reference_alias(&self) -> Option<Self> {
        let mut alias = self.clone();
        alias.name = self.operation.alias()?;
        if matches!(self.operation, SubagentOperation::Spawn) {
            if let Some(properties) = alias.schema["properties"].as_object_mut() {
                if let Some(background) = properties.remove("background") {
                    properties.insert("run_in_background".into(), background);
                }
            }
        }
        Some(alias)
    }
}

fn decode<T: DeserializeOwned>(args: Value) -> Result<T, ToolError> {
    serde_json::from_value(args).map_err(|error| ToolError::InvalidArguments(error.to_string()))
}

fn structured<T: Serialize>(text: String, output: T) -> Result<ToolResult, ToolError> {
    let value =
        serde_json::to_value(output).map_err(|error| ToolError::Execution(error.to_string()))?;
    Ok(ToolResult::structured(text, value))
}

#[async_trait::async_trait]
impl Tool for SubagentTool {
    fn id(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_json_schema(&self) -> Value {
        self.schema.clone()
    }

    fn capability(&self) -> ToolCapability {
        // Waits must not occupy the tool slot required by the child.
        ToolCapability::SpawnAgent
    }

    fn permission_requests(&self, _: &Value) -> Vec<(String, String)> {
        // The actor checks the resolved type, including implicit and resume
        // types, before any child work. Raw JSON is not permission authority.
        vec![(self.operation.id().into(), "*".into())]
    }

    async fn call(&self, ctx: ToolContext, mut args: Value) -> Result<ToolResult, ToolError> {
        let call_id = ctx.tool_call_id.to_string();
        match self.operation {
            SubagentOperation::Spawn => {
                if self.name == "task" {
                    if let Some(properties) = args.as_object_mut() {
                        if let Some(background) = properties.remove("run_in_background") {
                            properties.insert("background".into(), background);
                        }
                    }
                }
                let input: SpawnSubagentInput = decode(args)?;
                ctx.coordinator
                    .spawn_subagent(ctx.actor, call_id, input)
                    .await
                    .map(Into::into)
                    .map_err(Into::into)
            }
            SubagentOperation::Output => {
                let mut input: GetCommandOrSubagentOutputInput = decode(args)?;
                let mut ids = Vec::new();
                for id in input.task_ids {
                    let id = id.trim();
                    if !id.is_empty() && !ids.iter().any(|seen| seen == id) {
                        ids.push(id.to_owned());
                    }
                }
                if ids.is_empty() {
                    return Err(ToolError::InvalidArguments(
                        "Provide a non-empty task_ids list.".into(),
                    ));
                }
                check_count(ids.len())?;
                input.task_ids = ids;
                let value = ctx
                    .coordinator
                    .get_command_or_subagent_output(ctx.actor, call_id, input)
                    .await
                    .map_err(ToolError::from)?;
                output_result(value)
            }
            SubagentOperation::Wait => {
                let input: WaitCommandsOrSubagentsInput = decode(args)?;
                if input.task_ids.is_empty() {
                    return Err(ToolError::InvalidArguments(
                        "task_ids must not be empty.".into(),
                    ));
                }
                // Native legacy wait checks raw entries, without trim/dedup.
                check_count(input.task_ids.len())?;
                let value = ctx
                    .coordinator
                    .wait_commands_or_subagents(ctx.actor, call_id, input)
                    .await
                    .map_err(ToolError::from)?;
                output_result(value.into())
            }
            SubagentOperation::Kill => {
                let input: KillCommandOrSubagentInput = decode(args)?;
                let value = ctx
                    .coordinator
                    .kill_command_or_subagent(ctx.actor, call_id, input)
                    .await
                    .map_err(ToolError::from)?;
                let text = match &value {
                    KillCommandOrSubagentValue::Result(result) => {
                        format!("{}: {}", result.outcome, result.message)
                    }
                    KillCommandOrSubagentValue::TaskNotFound(message) => message.clone(),
                };
                let missing = matches!(value, KillCommandOrSubagentValue::TaskNotFound(_));
                let mut result = structured(text, value)?;
                mark_not_found(&mut result, missing);
                Ok(result)
            }
            SubagentOperation::Send => {
                let input: SendSubagentMessageInput = decode(args)?;
                // Sender grants, target syntax, byte limits and their ordering
                // are actor-owned. JSON cannot choose a sender or generation.
                let output = ctx
                    .coordinator
                    .send_subagent_message(ctx.actor, call_id, input)
                    .await
                    .map_err(ToolError::from)?;
                structured(message_text(&output), output)
            }
        }
    }
}

fn check_count(count: usize) -> Result<(), ToolError> {
    if count > 20 {
        return Err(ToolError::InvalidArguments(
            "task_ids exceeds maximum of 20 entries.".into(),
        ));
    }
    Ok(())
}

fn output_result(value: GetCommandOrSubagentOutputValue) -> Result<ToolResult, ToolError> {
    let text = match &value {
        GetCommandOrSubagentOutputValue::Result(result) => single_output_text(result),
        GetCommandOrSubagentOutputValue::TaskNotFound(message) => message.clone(),
        GetCommandOrSubagentOutputValue::MultiResult(multi) => {
            let mut lines = vec![format!("=== Multi-wait ({}) ===", multi.mode)];
            for result in &multi.results {
                lines.push(format!(
                    "--- Task {} [{}] ---\nCommand: {}\nDuration: {:.2}s",
                    result.task_id, result.status, result.command, result.duration_secs,
                ));
                if let Some(code) = result.exit_code {
                    lines.push(format!("Exit Code: {code}"));
                }
                if !result.output.is_empty() {
                    lines.push(result.output.clone());
                }
            }
            lines.push(format!("\n{}", multi.summary));
            lines.join("\n")
        }
    };
    let missing = matches!(value, GetCommandOrSubagentOutputValue::TaskNotFound(_));
    let mut result = structured(text, value)?;
    mark_not_found(&mut result, missing);
    Ok(result)
}

fn single_output_text(result: &GetCommandOrSubagentOutputResult) -> String {
    let mut lines = vec![
        format!("=== Task {} ===", result.task_id),
        format!("Command: {}", result.command),
        format!("Status: {}", result.status),
        format!("Duration: {:.2}s", result.duration_secs),
    ];
    if let Some(code) = result.exit_code {
        lines.push(format!("Exit Code: {code}"));
    }
    if !result.output_file.is_empty() {
        lines.push(format!("Output File: {}", result.output_file));
    }
    lines.push(String::new());
    lines.push("=== Output ===".into());
    lines.push(if result.output.is_empty() {
        if result.status == "running" {
            "(no output yet)".into()
        } else {
            "(no output)".into()
        }
    } else {
        result.output.clone()
    });
    if result.truncated {
        lines.push(result.truncation_hint.clone());
    }
    lines.join("\n")
}

fn mark_not_found(result: &mut ToolResult, missing: bool) {
    // Native TaskNotFound is a logical error output, not a transport error.
    if missing {
        if let Some(value) = result.structured_json.as_mut() {
            value["is_error"] = Value::Bool(true);
        }
    }
}

fn message_text(output: &SendSubagentMessageOutput) -> String {
    use SendSubagentMessageOutput as Output;
    match output {
        Output::Accepted { message_id } => {
            format!("Message accepted (message_id: {message_id}).")
        }
        Output::NotFoundOrNotOwned => "Subagent not found or not owned by this session.".into(),
        Output::NotActiveOrFinalizing => "Subagent is not active or is finalizing.".into(),
        Output::Saturated { max_in_flight } => format!(
            "Message admission is saturated (maximum {max_in_flight} in flight)."
        ),
        Output::QuotaExceeded { kind, limit } => {
            format!("Agent-message quota exceeded ({kind:?}, limit {limit}).")
        }
        Output::AdmissionUncertain => "Message admission could not be confirmed; the message may or may not have been accepted.".into(),
        Output::NotAcceptedBeforeDeadline => "Message was not accepted before the delivery deadline.".into(),
        Output::Unsupported => "Active agent messages are unsupported in this context.".into(),
        Output::Limit { max_bytes, observed_bytes } => format!(
            "Message size is invalid: observed {observed_bytes} bytes; maximum is {max_bytes} bytes."
        ),
        Output::ChannelClosed => "Message was not accepted because the subagent channel closed.".into(),
        _ => format!("{output:?}"),
    }
}
