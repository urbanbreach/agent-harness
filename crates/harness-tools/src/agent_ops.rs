use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use harness_core::event::{ActorKind, EventActor, EventV1, TaskScheduleState, ToolCallStatus};
use harness_core::tool::{
    canonical_tool_id_for, Tool, ToolCapability, ToolContext, ToolError, ToolResult,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::task::JoinSet;
use tokio::time::{sleep, Instant};
use tokio_stream::StreamExt;

const DEFAULT_TASK_WAIT_TIMEOUT_MS: u64 = 300_000;
const MAX_BATCH_CALLS: usize = 25;
const BATCH_NESTED_ERROR: &str = "tool.batch cannot be nested inside tool.batch";
const BATCH_MAX_CALLS_ERROR: &str = "Maximum of 25 tools allowed in batch";

pub(crate) struct AgentOpsExecutor;

impl AgentOpsExecutor {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) async fn spawn_agent(
        &self,
        ctx: &ToolContext,
        request: AgentSpawnRequest,
    ) -> Result<ToolResult, ToolError> {
        let supervisor = EventActor::new(ActorKind::Supervisor, None);
        let existing_session_id = request.session_id.clone().or(request.task_id.clone());
        let resumed_existing_session = existing_session_id.is_some();
        let agent_id = if let Some(session_id) = existing_session_id {
            session_id
        } else {
            ctx.coordinator
                .spawn_agent_idle(
                    supervisor.clone(),
                    request.profile_name.clone(),
                    ctx.actor.agent_id.clone(),
                )
                .await
                .map_err(|err| ToolError::Execution(format!("failed to spawn agent: {err}")))?
        };
        let request_id = ctx
            .coordinator
            .request_agent_turn(supervisor, agent_id.clone(), build_child_prompt(&request))
            .await
            .map_err(|err| ToolError::Execution(format!("failed to request agent turn: {err}")))?;

        let lineage = json!({
            "parent_tool_call_id": ctx.tool_call_id.clone(),
            "child_session_id": agent_id.clone(),
            "child_request_id": request_id.clone(),
        });
        let permissions = child_permission_metadata(ctx, &request);

        if request.run_in_background {
            let child_session = child_session_observability(
                &agent_id,
                &request_id,
                &request,
                resumed_existing_session,
                &permissions,
                ChildRequestObservability {
                    status: "scheduled",
                    duration_ms: None,
                    result_summary: None,
                    failure_summary: None,
                    tool_calls: ChildToolCallCounts::default(),
                },
            );
            return Ok(ToolResult {
                display_text: format!(
                    "task_id: {agent_id}\nrequest_id: {request_id}\n\n<task_result>Background task scheduled.</task_result>"
                ),
                structured_json: Some(spawn_result_json(
                    &request,
                    &agent_id,
                    &request_id,
                    lineage.clone(),
                    &child_session,
                )),
                artifacts: Vec::new(),
            });
        }

        let child_observability = wait_for_request_completion(ctx, &request_id).await?;
        let child_session = child_session_observability(
            &agent_id,
            &request_id,
            &request,
            resumed_existing_session,
            &permissions,
            child_observability,
        );
        let task_result = child_session
            .result_summary
            .clone()
            .or_else(|| child_session.failure_summary.clone())
            .unwrap_or_else(|| match child_session.status {
                "timed_out" => {
                    format!("timed out waiting for child session request {request_id}")
                }
                _ => "Child session finished without a summary.".to_string(),
            });

        Ok(ToolResult {
            display_text: format!(
                "task_id: {agent_id}\nrequest_id: {request_id}\n\n<task_result>\n{}\n</task_result>",
                task_result
            ),
            structured_json: Some(spawn_result_json(
                &request,
                &agent_id,
                &request_id,
                lineage,
                &child_session,
            )),
            artifacts: Vec::new(),
        })
    }

    pub(crate) async fn execute_batch(
        &self,
        ctx: &ToolContext,
        calls: Vec<BatchCall>,
    ) -> Result<ToolResult, ToolError> {
        if calls.is_empty() {
            return Err(ToolError::InvalidArguments(
                "Provide at least one tool call".to_string(),
            ));
        }

        let requested_call_count = calls.len();
        let mut join_set = JoinSet::new();

        let mut outcomes = Vec::new();
        for (index, call) in calls.into_iter().enumerate() {
            let tool_id = call.tool;
            let parameters = call.parameters;
            if index >= MAX_BATCH_CALLS {
                let canonical_tool_id = canonical_tool_id_for(&tool_id).map(str::to_string);
                outcomes.push(BatchCallOutcome {
                    index,
                    tool_id,
                    canonical_tool_id,
                    parameters,
                    result: Err(BATCH_MAX_CALLS_ERROR.to_string()),
                });
                continue;
            }

            let canonical_tool_id = canonical_tool_id_for(&tool_id).map(str::to_string);
            if canonical_tool_id.as_deref() == Some("tool.batch") {
                outcomes.push(BatchCallOutcome {
                    index,
                    tool_id,
                    canonical_tool_id,
                    parameters,
                    result: Err(BATCH_NESTED_ERROR.to_string()),
                });
                continue;
            }

            let coordinator = ctx.coordinator.clone();
            let actor = ctx.actor.clone();
            let category = ctx.category.clone();
            join_set.spawn(async move {
                let execution_parameters = parameters.clone();
                let result = coordinator
                    .execute_agent_tool_call(actor, category, tool_id.clone(), execution_parameters)
                    .await;
                BatchCallOutcome {
                    index,
                    tool_id,
                    canonical_tool_id,
                    parameters,
                    result,
                }
            });
        }

        while let Some(joined) = join_set.join_next().await {
            outcomes.push(
                joined.map_err(|err| ToolError::Execution(format!("batch join failed: {err}")))?,
            );
        }

        outcomes.sort_by_key(|outcome| outcome.index);

        let successful = outcomes
            .iter()
            .filter(|outcome| outcome.result.is_ok())
            .count();
        let failed = outcomes.len().saturating_sub(successful);

        let details = outcomes
            .into_iter()
            .map(batch_detail_json)
            .collect::<Vec<_>>();

        Ok(ToolResult {
            display_text: if failed == 0 {
                format!("All {successful} tools executed successfully.")
            } else {
                format!("Executed {successful} tools successfully. {failed} failed.")
            },
            structured_json: Some(json!({
                "successful": successful,
                "failed": failed,
                "requested_call_count": requested_call_count,
                "processed_call_count": details.len(),
                "discarded_call_count": requested_call_count.saturating_sub(MAX_BATCH_CALLS),
                "max_calls": MAX_BATCH_CALLS,
                "execution": {
                    "concurrency": "parallel",
                    "result_order": "input",
                    "nested_batch_disallowed": true,
                },
                "audit": {
                    "successful": successful,
                    "failed": failed,
                    "requested_call_count": requested_call_count,
                    "processed_call_count": details.len(),
                    "discarded_call_count": requested_call_count.saturating_sub(MAX_BATCH_CALLS),
                },
                "details": details,
            })),
            artifacts: Vec::new(),
        })
    }
}

pub(crate) struct AgentSpawnTool {
    executor: Arc<AgentOpsExecutor>,
}

impl AgentSpawnTool {
    pub(crate) fn new(executor: Arc<AgentOpsExecutor>) -> Self {
        Self { executor }
    }
}

pub(crate) struct ToolBatchTool {
    executor: Arc<AgentOpsExecutor>,
}

impl ToolBatchTool {
    pub(crate) fn new(executor: Arc<AgentOpsExecutor>) -> Self {
        Self { executor }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AgentSpawnRequest {
    pub(crate) description: String,
    pub(crate) profile_name: String,
    pub(crate) prompt: String,
    pub(crate) task_id: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) run_in_background: bool,
    pub(crate) load_skills: Vec<String>,
    pub(crate) command: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AgentSpawnArgs {
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    profile_name: Option<String>,
    #[serde(default)]
    system_prompt: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    task: Option<String>,
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    background: Option<bool>,
    #[serde(default)]
    run_in_background: Option<bool>,
    #[serde(default)]
    skills: Option<Vec<String>>,
    #[serde(default)]
    load_skills: Option<Vec<String>>,
    #[serde(default)]
    command: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct BatchArgs {
    #[serde(default)]
    calls: Vec<BatchCallArgs>,
    #[serde(default)]
    tool_calls: Vec<BatchCallArgs>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct BatchCallArgs {
    #[serde(default)]
    tool_id: Option<String>,
    #[serde(default)]
    tool: Option<String>,
    #[serde(default)]
    args: Option<Value>,
    #[serde(default)]
    parameters: Option<Value>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct BatchCall {
    pub(crate) tool: String,
    pub(crate) parameters: Value,
}

#[derive(Debug)]
struct BatchCallOutcome {
    index: usize,
    tool_id: String,
    canonical_tool_id: Option<String>,
    parameters: Value,
    result: Result<ToolResult, String>,
}

#[derive(Debug, Clone, Default, Serialize)]
struct ChildToolCallCounts {
    requested: u64,
    succeeded: u64,
    failed: u64,
}

#[derive(Debug, Clone, Serialize)]
struct ChildPermissionMetadata {
    spawn_permission_kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_scope: Option<String>,
    child_scope: String,
    scope_relation: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct ChildSessionObservability {
    session_id: String,
    request_id: String,
    profile: String,
    background: bool,
    mode: &'static str,
    status: &'static str,
    resumed_existing_session: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_summary: Option<String>,
    tool_calls: ChildToolCallCounts,
    permissions: ChildPermissionMetadata,
}

#[derive(Debug, Clone)]
struct ChildRequestObservability {
    status: &'static str,
    duration_ms: Option<u64>,
    result_summary: Option<String>,
    failure_summary: Option<String>,
    tool_calls: ChildToolCallCounts,
}

#[derive(Debug, Clone, Copy)]
enum ChildTerminalState {
    Completed,
    Failed,
    TimedOut,
}

fn resolve_string_alias(
    primary: Option<String>,
    alias: Option<String>,
    primary_name: &str,
    alias_name: &str,
) -> Result<Option<String>, ToolError> {
    match (primary, alias) {
        (Some(primary), Some(alias)) if primary != alias => Err(ToolError::InvalidArguments(
            format!("{primary_name} and {alias_name} must match when both are provided"),
        )),
        (Some(primary), Some(_)) => Ok(Some(primary)),
        (Some(primary), None) => Ok(Some(primary)),
        (None, Some(alias)) => Ok(Some(alias)),
        (None, None) => Ok(None),
    }
}

fn resolve_bool_alias(
    primary: Option<bool>,
    alias: Option<bool>,
    primary_name: &str,
    alias_name: &str,
) -> Result<Option<bool>, ToolError> {
    match (primary, alias) {
        (Some(primary), Some(alias)) if primary != alias => Err(ToolError::InvalidArguments(
            format!("{primary_name} and {alias_name} must match when both are provided"),
        )),
        (Some(primary), Some(_)) => Ok(Some(primary)),
        (Some(primary), None) => Ok(Some(primary)),
        (None, Some(alias)) => Ok(Some(alias)),
        (None, None) => Ok(None),
    }
}

fn normalize_agent_spawn_args(args: AgentSpawnArgs) -> Result<AgentSpawnRequest, ToolError> {
    let profile_name =
        resolve_string_alias(args.profile, args.profile_name, "profile", "profile_name")?
            .ok_or_else(|| ToolError::InvalidArguments("profile is required".to_string()))?;
    if profile_name.trim().is_empty() {
        return Err(ToolError::InvalidArguments(
            "profile must not be empty".to_string(),
        ));
    }

    let prompt = resolve_string_alias(args.prompt, args.task, "prompt", "task")?
        .ok_or_else(|| ToolError::InvalidArguments("prompt is required".to_string()))?;
    if prompt.trim().is_empty() {
        return Err(ToolError::InvalidArguments(
            "prompt must not be empty".to_string(),
        ));
    }

    let run_in_background = resolve_bool_alias(
        args.background,
        args.run_in_background,
        "background",
        "run_in_background",
    )?
    .unwrap_or(false);

    let load_skills = match (args.skills, args.load_skills) {
        (Some(skills), Some(load_skills)) if skills != load_skills => {
            return Err(ToolError::InvalidArguments(
                "skills and load_skills must match when both are provided".to_string(),
            ));
        }
        (Some(skills), Some(_)) => skills,
        (Some(skills), None) => skills,
        (None, Some(load_skills)) => load_skills,
        (None, None) => Vec::new(),
    };

    let session_id = resolve_string_alias(args.session_id, args.task_id, "session_id", "task_id")?;

    let _ = args.system_prompt.as_deref();

    Ok(AgentSpawnRequest {
        description: args
            .description
            .unwrap_or_else(|| "Delegated task".to_string()),
        profile_name,
        prompt,
        task_id: session_id.clone(),
        session_id,
        run_in_background,
        load_skills,
        command: args.command,
    })
}

fn normalize_batch_calls(args: BatchArgs) -> Result<Vec<BatchCall>, ToolError> {
    let incoming = match (args.calls.is_empty(), args.tool_calls.is_empty()) {
        (false, false) => {
            return Err(ToolError::InvalidArguments(
                "provide either calls or tool_calls, not both".to_string(),
            ));
        }
        (false, true) => args.calls,
        (true, false) => args.tool_calls,
        (true, true) => Vec::new(),
    };

    incoming
        .into_iter()
        .enumerate()
        .map(|(index, call)| {
            let tool = resolve_string_alias(call.tool_id, call.tool, "tool_id", "tool")?
                .ok_or_else(|| {
                    ToolError::InvalidArguments(format!(
                        "calls[{index}] must include tool_id (or tool)",
                    ))
                })?;
            if tool.trim().is_empty() {
                return Err(ToolError::InvalidArguments(format!(
                    "calls[{index}] tool_id must not be empty",
                )));
            }

            let parameters = match (call.args, call.parameters) {
                (Some(args), Some(parameters)) if args != parameters => {
                    return Err(ToolError::InvalidArguments(format!(
                        "calls[{index}] args and parameters must match when both are provided",
                    )));
                }
                (Some(args), Some(_)) => args,
                (Some(args), None) => args,
                (None, Some(parameters)) => parameters,
                (None, None) => json!({}),
            };

            Ok(BatchCall { tool, parameters })
        })
        .collect()
}

fn build_child_prompt(request: &AgentSpawnRequest) -> String {
    if request.load_skills.is_empty() && request.command.is_none() {
        return request.prompt.clone();
    }

    let mut prompt = String::from("Delegation context from parent:\n");
    if !request.load_skills.is_empty() {
        prompt.push_str("- Load and apply these skills before starting: ");
        prompt.push_str(&request.load_skills.join(", "));
        prompt.push('\n');
    }
    if let Some(command) = request.command.as_deref() {
        prompt.push_str("- Treat this command as required execution context: ");
        prompt.push_str(command);
        prompt.push('\n');
    }
    prompt.push_str("\nTask:\n");
    prompt.push_str(&request.prompt);
    prompt
}

#[async_trait]
impl Tool for AgentSpawnTool {
    fn id(&self) -> &str {
        "agent.spawn"
    }

    fn description(&self) -> &str {
        "Spawns a child agent and optionally waits for completion. `load_skills`/`skills` and `command` are prepended to the child prompt as explicit delegation instructions."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<AgentSpawnArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: AgentSpawnArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;

        let request = normalize_agent_spawn_args(args)?;
        self.executor.spawn_agent(&ctx, request).await
    }
}

#[async_trait]
impl Tool for ToolBatchTool {
    fn id(&self) -> &str {
        "tool.batch"
    }

    fn description(&self) -> &str {
        "Executes multiple tool calls through the coordinator and waits for all results."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<BatchArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: BatchArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        let calls = normalize_batch_calls(args)?;
        self.executor.execute_batch(&ctx, calls).await
    }
}

pub(crate) fn select_profile_name(
    category: Option<&str>,
    subagent_type: Option<&str>,
) -> Result<String, ToolError> {
    match (category, subagent_type) {
        (Some(_), Some(_)) => Err(ToolError::InvalidArguments(
            "provide either category or subagent_type, not both".to_string(),
        )),
        (Some(category), None) => Ok(category.to_string()),
        (None, Some(subagent_type)) => Ok(subagent_type.to_string()),
        (None, None) => Err(ToolError::InvalidArguments(
            "category or subagent_type is required".to_string(),
        )),
    }
}

async fn wait_for_request_completion(
    ctx: &ToolContext,
    request_id: &str,
) -> Result<ChildRequestObservability, ToolError> {
    let store = ctx
        .coordinator
        .event_store()
        .await
        .map_err(|err| ToolError::Execution(format!("failed to access event store: {err}")))?;
    let mut stream = store
        .subscribe(1)
        .map_err(|err| ToolError::Execution(format!("failed to subscribe to events: {err}")))?;
    let deadline = Instant::now() + Duration::from_millis(DEFAULT_TASK_WAIT_TIMEOUT_MS);
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let next =
            tokio::time::timeout(remaining.min(Duration::from_millis(250)), stream.next()).await;
        match next {
            Ok(Some(Ok(event))) => match &event.payload {
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(request_id) =>
                {
                    let _ = data;
                    return summarize_child_request(ctx, request_id, ChildTerminalState::Completed)
                        .await;
                }
                EventV1::TaskCancelled(data)
                    if event.correlation_id.as_deref() == Some(request_id) =>
                {
                    let _ = data;
                    return summarize_child_request(ctx, request_id, ChildTerminalState::Failed)
                        .await;
                }
                _ => {}
            },
            Ok(Some(Err(err))) => {
                return Err(ToolError::Execution(format!(
                    "failed to consume event stream: {err}"
                )));
            }
            Ok(None) | Err(_) => {
                sleep(Duration::from_millis(10)).await;
            }
        }
    }
    summarize_child_request(ctx, request_id, ChildTerminalState::TimedOut).await
}

async fn summarize_child_request(
    ctx: &ToolContext,
    request_id: &str,
    terminal_state: ChildTerminalState,
) -> Result<ChildRequestObservability, ToolError> {
    let store = ctx
        .coordinator
        .event_store()
        .await
        .map_err(|err| ToolError::Execution(format!("failed to access event store: {err}")))?;
    let mut replay = store
        .replay(1)
        .map_err(|err| ToolError::Execution(format!("failed to replay events: {err}")))?;

    let mut tool_calls = ChildToolCallCounts::default();
    let mut started_mono_ms = None;
    let mut observed_result_summary = None;
    let mut observed_failure_summary = None;
    let mut observed_duration_ms = None;
    let mut observed_status = None;

    while let Some(next) = replay.next().await {
        let event = next
            .map_err(|err| ToolError::Execution(format!("failed to replay event stream: {err}")))?;
        if event.correlation_id.as_deref() != Some(request_id) {
            continue;
        }

        match &event.payload {
            EventV1::TaskScheduled(data) if data.state == TaskScheduleState::Started => {
                started_mono_ms.get_or_insert(event.mono_ms);
            }
            EventV1::ToolCallRequested(_) => {
                tool_calls.requested = tool_calls.requested.saturating_add(1);
            }
            EventV1::ToolCallFinished(data) => match data.status {
                ToolCallStatus::Succeeded => {
                    tool_calls.succeeded = tool_calls.succeeded.saturating_add(1);
                }
                ToolCallStatus::Failed => {
                    tool_calls.failed = tool_calls.failed.saturating_add(1);
                }
            },
            EventV1::TaskCompleted(data) => {
                observed_status = Some("completed");
                observed_result_summary = Some(data.result_summary.clone());
                let timing = data
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.timing.as_ref());
                started_mono_ms = timing
                    .and_then(|metadata| metadata.started_mono_ms)
                    .or(started_mono_ms);
                observed_duration_ms = timing
                    .and_then(|metadata| metadata.elapsed_ms)
                    .or_else(|| elapsed_ms_from_events(started_mono_ms, event.mono_ms));
            }
            EventV1::TaskCancelled(data) => {
                observed_status = Some("failed");
                observed_failure_summary = Some(data.reason.clone());
                observed_duration_ms = elapsed_ms_from_events(started_mono_ms, event.mono_ms);
            }
            _ => {}
        }
    }

    let (status, failure_summary) = match (observed_status, terminal_state) {
        (Some(status), _) => (status, observed_failure_summary),
        (None, ChildTerminalState::Completed) => ("completed", observed_failure_summary),
        (None, ChildTerminalState::Failed) => ("failed", observed_failure_summary),
        (None, ChildTerminalState::TimedOut) => (
            "timed_out",
            Some(format!("timed out waiting for task request {request_id}")),
        ),
    };

    Ok(ChildRequestObservability {
        status,
        duration_ms: observed_duration_ms,
        result_summary: observed_result_summary,
        failure_summary,
        tool_calls,
    })
}

fn elapsed_ms_from_events(started_mono_ms: Option<u64>, finished_mono_ms: u64) -> Option<u64> {
    started_mono_ms.map(|started| finished_mono_ms.saturating_sub(started))
}

fn child_permission_metadata(
    ctx: &ToolContext,
    request: &AgentSpawnRequest,
) -> ChildPermissionMetadata {
    let parent_scope = ctx.category.clone();
    let scope_relation = if parent_scope.as_deref() == Some(request.profile_name.as_str()) {
        "inherits_parent_scope"
    } else {
        "isolated_by_requested_profile"
    };

    ChildPermissionMetadata {
        spawn_permission_kind: "task",
        parent_scope,
        child_scope: request.profile_name.clone(),
        scope_relation,
    }
}

fn child_session_observability(
    session_id: &str,
    request_id: &str,
    request: &AgentSpawnRequest,
    resumed_existing_session: bool,
    permissions: &ChildPermissionMetadata,
    observability: ChildRequestObservability,
) -> ChildSessionObservability {
    ChildSessionObservability {
        session_id: session_id.to_string(),
        request_id: request_id.to_string(),
        profile: request.profile_name.clone(),
        background: request.run_in_background,
        mode: if request.run_in_background {
            "background"
        } else {
            "foreground"
        },
        status: observability.status,
        resumed_existing_session,
        duration_ms: observability.duration_ms,
        result_summary: observability.result_summary,
        failure_summary: observability.failure_summary,
        tool_calls: observability.tool_calls,
        permissions: permissions.clone(),
    }
}

fn spawn_result_json(
    request: &AgentSpawnRequest,
    agent_id: &str,
    request_id: &str,
    lineage: Value,
    child_session: &ChildSessionObservability,
) -> Value {
    json!({
        "description": request.description,
        "profile": request.profile_name,
        "task_id": agent_id,
        "session_id": agent_id,
        "request_id": request_id,
        "child_session_id": agent_id,
        "child_request_id": request_id,
        "lineage": lineage,
        "load_skills": request.load_skills,
        "skills": request.load_skills,
        "command": request.command,
        "background": child_session.background,
        "mode": child_session.mode,
        "status": child_session.status,
        "duration_ms": child_session.duration_ms,
        "result_summary": child_session.result_summary,
        "failure_summary": child_session.failure_summary,
        "child_tool_call_count": child_session.tool_calls.requested,
        "child_tool_call_counts": child_session.tool_calls,
        "resumed_existing_session": child_session.resumed_existing_session,
        "permissions": child_session.permissions,
        "child_session": child_session,
    })
}

fn batch_detail_json(outcome: BatchCallOutcome) -> Value {
    let request = json!({
        "tool_id": outcome.tool_id,
        "canonical_tool_id": outcome.canonical_tool_id,
        "parameters": outcome.parameters,
    });

    match outcome.result {
        Ok(value) => {
            let result = json!({
                "success": true,
                "status": "succeeded",
                "summary": value.display_text,
                "structured_output": value.structured_json,
                "artifacts": value.artifacts,
            });

            json!({
                "index": outcome.index,
                "tool_id": request.get("tool_id").cloned(),
                "canonical_tool_id": request.get("canonical_tool_id").cloned(),
                "parameters": request.get("parameters").cloned(),
                "request": request,
                "success": true,
                "status": "succeeded",
                "summary": result.get("summary").cloned(),
                "structured_output": result.get("structured_output").cloned(),
                "artifacts": result.get("artifacts").cloned(),
                "result": result,
            })
        }
        Err(error) => {
            let result = json!({
                "success": false,
                "status": "failed",
                "error": error,
            });

            json!({
                "index": outcome.index,
                "tool_id": request.get("tool_id").cloned(),
                "canonical_tool_id": request.get("canonical_tool_id").cloned(),
                "parameters": request.get("parameters").cloned(),
                "request": request,
                "success": false,
                "status": "failed",
                "error": result.get("error").cloned(),
                "result": result,
            })
        }
    }
}
