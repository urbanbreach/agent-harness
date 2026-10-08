use harness_core::{
    coord::{CommandSnapshot, PreparedCommand},
    tool::{ToolContext, ToolError, ToolResult},
};
use serde_json::json;
use std::time::Duration;

pub(super) async fn run(
    context: ToolContext,
    command: tokio::process::Command,
    scratch: Option<tempfile::TempDir>,
    label: String,
    description: Option<String>,
    timeout_ms: Option<u64>,
    block_until_ms: u64,
    sandbox_policy: &str,
) -> Result<ToolResult, ToolError> {
    let coordinator = context.coordinator.clone();
    let actor = context.actor.clone();
    let cancellation = context.cancellation.clone();
    let snapshot = coordinator
        .start_command(
            context,
            PreparedCommand {
                command,
                label,
                description,
                // The native background actor's independent safety ceiling is 10 hours.
                timeout: Duration::from_millis(timeout_ms.unwrap_or(36_000_000)),
                scratch,
            },
        )
        .await
        .map_err(|error| ToolError::Execution(error.to_string()))?;
    let id = snapshot.result.task_id.clone();
    let started_result = snapshot.result.clone();
    let mut subscription = coordinator
        .subscribe_command(actor.clone(), id.clone())
        .await
        .map_err(|error| ToolError::Execution(error.to_string()))?
        .ok_or_else(|| ToolError::Execution("admitted command disappeared".into()))?;
    if block_until_ms > 0 && !subscription.snapshot.is_terminal() {
        let waited = tokio::select! {
            biased;
            () = cancellation.cancelled() => None,
            waited = async {
                tokio::time::timeout(
                    Duration::from_millis(block_until_ms.min(3_600_000)),
                    subscription.updates.wait_for(CommandSnapshot::is_terminal),
                )
                .await
                .map(|result| result.map(|snapshot| snapshot.clone()))
            } => Some(waited),
        };
        match waited {
            None => {
                coordinator
                    .kill_command(actor, id)
                    .await
                    .map_err(|error| ToolError::Execution(error.to_string()))?;
                tokio::time::timeout(
                    Duration::from_secs(3),
                    subscription.updates.wait_for(CommandSnapshot::is_terminal),
                )
                .await
                .map_err(|_| {
                    ToolError::Execution("cancelled command cleanup did not finish".into())
                })?
                .map_err(|_| {
                    ToolError::Execution("command owner disconnected during cleanup".into())
                })?;
                return Err(ToolError::Cancelled);
            }
            Some(Ok(snapshot)) => {
                subscription.snapshot = snapshot
                    .map_err(|_| ToolError::Execution("command owner disconnected".into()))?;
            }
            Some(Err(_)) => {
                subscription.snapshot = subscription.updates.borrow().clone();
            }
        }
    }
    let snapshot = subscription.snapshot;
    if block_until_ms > 0 && snapshot.is_terminal() {
        coordinator
            .observe_command_result(actor, id)
            .await
            .map_err(|error| ToolError::Execution(error.to_string()))?;
        let result = &snapshot.result;
        return Ok(ToolResult::structured(
            format!(
                "{}\nExit code: {}{}",
                result.output,
                result
                    .exit_code
                    .map_or_else(|| "signal".into(), |code| code.to_string()),
                if result.truncated {
                    " (output truncated)"
                } else {
                    ""
                },
            ),
            json!({
                "type":"Foreground",
                "task_id":result.task_id,
                "status":result.status,
                "exit_code":result.exit_code,
                "is_error":result.status != "completed",
                "stdout":snapshot.stdout,
                "stderr":snapshot.stderr,
                "output":result.output,
                "output_file":result.output_file,
                "truncated":result.truncated,
                "raw_output_bytes":result.raw_output_bytes,
                "started":result.started,
                "ended":result.ended,
                "duration_secs":result.duration_secs,
                "os_sandbox_policy":sandbox_policy,
            }),
        ));
    }
    let result = if block_until_ms == 0 {
        started_result
    } else {
        snapshot.result
    };
    let summary = format!(
        "Command started in background with task_id: {}",
        result.task_id
    );
    let hint = "Use get_command_or_subagent_output with task_ids to retrieve output and status.";
    Ok(ToolResult::structured(
        format!("{summary}\n{hint}"),
        json!({
            "type":"Background",
            "task_id":result.task_id,
            "task_type":"bash",
            "output_file":result.output_file,
            "status":result.status,
            "command":result.command,
            "summary":summary,
            "retrieval_hint":hint,
        }),
    ))
}
