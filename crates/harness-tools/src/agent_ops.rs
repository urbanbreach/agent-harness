use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use harness_core::event::{ActorKind, EventActor, EventV1};
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
            .request_agent_turn(supervisor, agent_id.clone(), request.prompt.clone())
            .await
            .map_err(|err| ToolError::Execution(format!("failed to request agent turn: {err}")))?;

        let lineage = json!({
            "parent_tool_call_id": ctx.tool_call_id.clone(),
            "child_session_id": agent_id.clone(),
            "child_request_id": request_id.clone(),
        });

        if request.run_in_background {
            return Ok(ToolResult {
                display_text: format!(
                    "task_id: {agent_id}\nrequest_id: {request_id}\n\n<task_result>Background task scheduled.</task_result>"
                ),
                structured_json: Some(json!({
                    "description": request.description,
                    "profile": request.profile_name,
                    "task_id": agent_id.clone(),
                    "session_id": agent_id.clone(),
                    "request_id": request_id.clone(),
                    "child_session_id": agent_id.clone(),
                    "child_request_id": request_id.clone(),
                    "lineage": lineage.clone(),
                    "load_skills": request.load_skills.clone(),
                    "skills": request.load_skills,
                    "command": request.command,
                    "background": true,
                    "mode": "background",
                    "status": "scheduled",
                    "resumed_existing_session": resumed_existing_session,
                })),
                artifacts: Vec::new(),
            });
        }

        let wait_started = Instant::now();
        let result = wait_for_request_completion(ctx, &request_id).await?;
        let duration_ms = wait_started.elapsed().as_millis() as u64;
        let child_tool_call_count = count_request_tool_calls(ctx, &request_id).await.ok();

        Ok(ToolResult {
            display_text: format!(
                "task_id: {agent_id}\nrequest_id: {request_id}\n\n<task_result>\n{}\n</task_result>",
                result
            ),
            structured_json: Some(json!({
                "description": request.description,
                "profile": request.profile_name,
                "task_id": agent_id.clone(),
                "session_id": agent_id.clone(),
                "request_id": request_id.clone(),
                "child_session_id": agent_id.clone(),
                "child_request_id": request_id.clone(),
                "lineage": lineage,
                "load_skills": request.load_skills.clone(),
                "skills": request.load_skills,
                "command": request.command,
                "background": false,
                "mode": "foreground",
                "status": "completed",
                "duration_ms": duration_ms,
                "result_summary": result,
                "child_tool_call_count": child_tool_call_count,
                "resumed_existing_session": resumed_existing_session,
            })),
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
            if index >= MAX_BATCH_CALLS {
                let canonical_tool_id = canonical_tool_id_for(&tool_id).map(str::to_string);
                outcomes.push(BatchCallOutcome {
                    index,
                    tool_id,
                    canonical_tool_id,
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
                    result: Err(BATCH_NESTED_ERROR.to_string()),
                });
                continue;
            }

            let parameters = call.parameters;
            let coordinator = ctx.coordinator.clone();
            let actor = ctx.actor.clone();
            let category = ctx.category.clone();
            join_set.spawn(async move {
                let result = coordinator
                    .execute_agent_tool_call(actor, category, tool_id.clone(), parameters)
                    .await;
                BatchCallOutcome {
                    index,
                    tool_id,
                    canonical_tool_id,
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
            .map(|outcome| match outcome.result {
                Ok(value) => json!({
                    "index": outcome.index,
                    "tool_id": outcome.tool_id,
                    "canonical_tool_id": outcome.canonical_tool_id,
                    "success": true,
                    "status": "succeeded",
                    "summary": value.display_text,
                    "structured_output": value.structured_json,
                    "artifacts": value.artifacts,
                }),
                Err(error) => json!({
                    "index": outcome.index,
                    "tool_id": outcome.tool_id,
                    "canonical_tool_id": outcome.canonical_tool_id,
                    "success": false,
                    "status": "failed",
                    "error": error,
                }),
            })
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
    result: Result<ToolResult, String>,
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

#[async_trait]
impl Tool for AgentSpawnTool {
    fn id(&self) -> &str {
        "agent.spawn"
    }

    fn description(&self) -> &str {
        "Spawns a child agent and optionally waits for completion."
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
) -> Result<String, ToolError> {
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
                    return Ok(data.result_summary.clone())
                }
                EventV1::TaskCancelled(data)
                    if event.correlation_id.as_deref() == Some(request_id) =>
                {
                    return Err(ToolError::Execution(format!(
                        "subtask cancelled: {}",
                        data.reason
                    )))
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
    Err(ToolError::Execution(format!(
        "timed out waiting for task request {request_id}"
    )))
}

async fn count_request_tool_calls(ctx: &ToolContext, request_id: &str) -> Result<u64, ToolError> {
    let store = ctx
        .coordinator
        .event_store()
        .await
        .map_err(|err| ToolError::Execution(format!("failed to access event store: {err}")))?;
    let mut replay = store
        .replay(1)
        .map_err(|err| ToolError::Execution(format!("failed to replay events: {err}")))?;

    let mut count = 0_u64;
    while let Some(next) = replay.next().await {
        let event = next
            .map_err(|err| ToolError::Execution(format!("failed to replay event stream: {err}")))?;
        if event.correlation_id.as_deref() != Some(request_id) {
            continue;
        }
        if matches!(event.payload, EventV1::ToolCallRequested(_)) {
            count = count.saturating_add(1);
        }
    }

    Ok(count)
}
