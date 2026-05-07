use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use harness_core::coord::{AgentRuntimeInfo, CoordinatorError};
use harness_core::event::{ActorKind, EventActor, EventV1, TaskScheduleState, ToolCallStatus};
use harness_core::tool::{canonical_tool_id_for, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::task::JoinSet;
use tokio::time::{sleep, Instant};
use tokio_stream::StreamExt;

const DEFAULT_TASK_WAIT_TIMEOUT_MS: u64 = 300_000;
const MAX_BACKGROUND_OUTPUT_TIMEOUT_MS: u64 = 300_000;
const MAX_BATCH_CALLS: usize = 25;
const BATCH_NESTED_ERROR: &str = "batch cannot be nested inside batch";
const BATCH_MAX_CALLS_ERROR: &str = "Maximum of 25 tools allowed in batch";
const CATEGORY_FALLBACK_PROFILE: &str = "general";

pub(crate) struct AgentOpsExecutor;

impl AgentOpsExecutor {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) async fn spawn_agent(
        &self,
        ctx: &ToolContext,
        mut request: AgentSpawnRequest,
    ) -> Result<ToolResult, ToolError> {
        enforce_parent_child_profile_policy(ctx, &request)?;

        let supervisor = EventActor::new(ActorKind::Supervisor, None);
        let existing_session_id = request.session_id.clone().or(request.task_id.clone());
        let resumed_existing_session = existing_session_id.is_some();
        let agent_id = if let Some(session_id) = existing_session_id {
            let target_info = ctx
                .coordinator
                .agent_runtime_info(session_id.clone())
                .await
                .map_err(|err| map_request_agent_turn_error(err, &request))?;
            apply_category_fallback_for_existing_session(ctx, &mut request, &target_info);
            authorize_existing_child_session(ctx, &request, &target_info)?;
            session_id
        } else {
            spawn_new_child_agent(ctx, &mut request, supervisor.clone()).await?
        };
        let request_id = ctx
            .coordinator
            .request_agent_turn(supervisor, agent_id.clone(), build_child_prompt(&request))
            .await
            .map_err(|err| map_request_agent_turn_error(err, &request))?;

        let lineage = json!({
            "parent_tool_call_id": ctx.tool_call_id.clone(),
            "parent_session_id": ctx.run_id.clone(),
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
                    "task_id: {agent_id} (for resuming to continue this task if needed)\nrequest_id: {request_id}\n\n<task_result>Background task scheduled.</task_result>"
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
                "task_id: {agent_id} (for resuming to continue this task if needed)\nrequest_id: {request_id}\n\n<task_result>\n{}\n</task_result>",
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
            if canonical_tool_id.as_deref() == Some("batch") {
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

    pub(crate) async fn background_output(
        &self,
        ctx: &ToolContext,
        request: BackgroundOutputRequest,
    ) -> Result<ToolResult, ToolError> {
        if request.timeout_ms > MAX_BACKGROUND_OUTPUT_TIMEOUT_MS {
            return Err(ToolError::InvalidArguments(format!(
                "background_output timeout must be <= {MAX_BACKGROUND_OUTPUT_TIMEOUT_MS} ms"
            )));
        }
        let request_ref = resolve_background_request_ref(ctx, &request).await?;
        let mut summary = summarize_background_request(ctx, &request_ref).await?;
        let mut timed_out = false;
        if request.block && !summary.terminal {
            let observed_terminal = wait_for_background_request_terminal(
                ctx,
                &request_ref.request_id,
                request.timeout_ms,
            )
            .await?;
            summary = summarize_background_request(ctx, &request_ref).await?;
            timed_out = !observed_terminal && !summary.terminal;
        }

        Ok(ToolResult {
            display_text: format_background_output(&summary, timed_out),
            structured_json: Some(json!({
                "request_id": summary.request_id,
                "task_id": summary.session_id,
                "session_id": summary.session_id,
                "scheduler_task_id": summary.scheduler_task_id,
                "status": summary.status,
                "terminal": summary.terminal,
                "block": request.block,
                "timed_out": timed_out,
                "timeout_ms": request.timeout_ms,
                "duration_ms": summary.duration_ms,
                "result_summary": summary.result_summary,
                "failure_summary": summary.failure_summary,
                "child_tool_call_count": summary.tool_calls.requested,
                "child_tool_call_counts": summary.tool_calls,
                "late_result": summary.late_result,
                "source": "event_replay",
            })),
            artifacts: Vec::new(),
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AgentSpawnRequest {
    pub(crate) description: String,
    pub(crate) profile_name: String,
    pub(crate) category_selector: Option<String>,
    pub(crate) prompt: String,
    pub(crate) task_id: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) run_in_background: bool,
    pub(crate) load_skills: Vec<String>,
    pub(crate) command: Option<String>,
}

impl AgentSpawnRequest {
    fn category_fallback_profile(&self) -> Option<&'static str> {
        self.category_selector
            .as_deref()
            .filter(|category| !category.eq_ignore_ascii_case(CATEGORY_FALLBACK_PROFILE))
            .map(|_| CATEGORY_FALLBACK_PROFILE)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BackgroundOutputRequest {
    pub(crate) task_id: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) request_id: Option<String>,
    pub(crate) block: bool,
    pub(crate) timeout_ms: u64,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct BatchCall {
    pub(crate) tool: String,
    #[serde(alias = "args", alias = "arguments")]
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

#[derive(Debug, Clone)]
struct BackgroundRequestRef {
    request_id: String,
    session_id_hint: Option<String>,
}

#[derive(Debug, Clone)]
struct BackgroundRequestSummary {
    request_id: String,
    session_id: Option<String>,
    scheduler_task_id: Option<String>,
    status: String,
    terminal: bool,
    duration_ms: Option<u64>,
    result_summary: Option<String>,
    failure_summary: Option<String>,
    tool_calls: ChildToolCallCounts,
    late_result: bool,
}

#[derive(Debug, Clone, Copy)]
enum ChildTerminalState {
    Completed,
    Failed,
    TimedOut,
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

fn enforce_parent_child_profile_policy(
    ctx: &ToolContext,
    request: &AgentSpawnRequest,
) -> Result<(), ToolError> {
    if ctx.category.as_deref() == Some(harness_core::plan::PLAN_AGENT_NAME)
        && request.profile_name != "explore"
    {
        return Err(ToolError::InvalidArguments(format!(
            "Plan mode may only delegate to the read-only `explore` profile; requested `{}`",
            request.profile_name
        )));
    }

    Ok(())
}

fn authorize_existing_child_session(
    ctx: &ToolContext,
    request: &AgentSpawnRequest,
    target_info: &AgentRuntimeInfo,
) -> Result<(), ToolError> {
    let parent_agent_id = ctx.actor.agent_id.as_deref().ok_or_else(|| {
        ToolError::InvalidArguments(
            "task session re-entry requires a worker-owned parent agent".to_string(),
        )
    })?;

    if target_info.parent_agent_id.as_deref() != Some(parent_agent_id) {
        return Err(ToolError::InvalidArguments(format!(
            "task session `{}` is not a direct child of the calling agent",
            target_info.agent_id
        )));
    }

    if target_info.profile_name != request.profile_name {
        return Err(ToolError::InvalidArguments(format!(
            "task session `{}` uses profile `{}`, but the request selected `{}`",
            target_info.agent_id, target_info.profile_name, request.profile_name
        )));
    }

    Ok(())
}

async fn spawn_new_child_agent(
    ctx: &ToolContext,
    request: &mut AgentSpawnRequest,
    supervisor: EventActor,
) -> Result<String, ToolError> {
    match spawn_child_agent_once(ctx, request, supervisor.clone()).await {
        Ok(agent_id) => Ok(agent_id),
        Err(CoordinatorError::UnknownAgent(_))
            if !category_fallback_disabled(ctx)
                && request.category_fallback_profile().is_some() =>
        {
            let fallback = request
                .category_fallback_profile()
                .expect("fallback checked")
                .to_string();
            request.profile_name = fallback;
            spawn_child_agent_once(ctx, request, supervisor)
                .await
                .map_err(|err| map_spawn_agent_error(err, request))
        }
        Err(err) => Err(map_spawn_agent_error(err, request)),
    }
}

async fn spawn_child_agent_once(
    ctx: &ToolContext,
    request: &AgentSpawnRequest,
    supervisor: EventActor,
) -> Result<String, CoordinatorError> {
    ctx.coordinator
        .spawn_agent_idle_with_child_title(
            supervisor,
            request.profile_name.clone(),
            ctx.actor.agent_id.clone(),
            format!(
                "{} (@{} subagent)",
                request.description, request.profile_name
            ),
        )
        .await
}

fn apply_category_fallback_for_existing_session(
    ctx: &ToolContext,
    request: &mut AgentSpawnRequest,
    target_info: &AgentRuntimeInfo,
) {
    if category_fallback_disabled(ctx) {
        return;
    }
    if request.category_fallback_profile() == Some(target_info.profile_name.as_str()) {
        request.profile_name = target_info.profile_name.clone();
    }
}

fn category_fallback_disabled(ctx: &ToolContext) -> bool {
    ctx.category.as_deref() == Some(harness_core::plan::PLAN_AGENT_NAME)
}

pub(crate) fn select_profile_name(
    category: Option<&str>,
    subagent_type: Option<&str>,
) -> Result<String, ToolError> {
    let category = normalize_profile_selector(category).map(str::to_string);
    let subagent_type = normalize_subagent_selector(subagent_type);
    match (category, subagent_type) {
        (Some(category), Some(subagent_type)) if category == subagent_type => Ok(category),
        (Some(_), Some(subagent_type)) => Ok(subagent_type),
        (Some(category), None) => Ok(category),
        (None, Some(subagent_type)) => Ok(subagent_type),
        (None, None) => Err(ToolError::InvalidArguments(
            "category or subagent_type is required".to_string(),
        )),
    }
}

fn normalize_profile_selector(selector: Option<&str>) -> Option<&str> {
    selector.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}

fn normalize_subagent_selector(selector: Option<&str>) -> Option<String> {
    normalize_profile_selector(selector).map(str::to_string)
}

fn map_request_agent_turn_error(err: CoordinatorError, request: &AgentSpawnRequest) -> ToolError {
    match err {
        CoordinatorError::UnknownAgent(agent_id)
            if request.session_id.is_some() || request.task_id.is_some() =>
        {
            let mut message = format!(
                "Unknown child session `{agent_id}`. Provide a `session_id`/`task_id` returned by a prior `task` call, or omit it to start a new child session."
            );
            if let Some(agent_hint) = normalize_subagent_selector(Some(&agent_id)) {
                if agent_hint != agent_id {
                    message.push_str(&format!(
                        " If you meant the `{agent_hint}` agent, set `subagent_type: \"{agent_hint}\"` instead."
                    ));
                }
            }
            ToolError::InvalidArguments(message)
        }
        other => ToolError::Execution(format!("failed to request agent turn: {other}")),
    }
}

fn map_spawn_agent_error(err: CoordinatorError, request: &AgentSpawnRequest) -> ToolError {
    match err {
        CoordinatorError::UnknownAgent(profile_name) => ToolError::InvalidArguments(format!(
            "Unknown child profile `{profile_name}`. Configure that agent profile before using task with category/subagent_type `{}`.",
            request.profile_name
        )),
        other => ToolError::Execution(format!("failed to spawn agent: {other}")),
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
                EventV1::TaskCompleted(_)
                    if event.correlation_id.as_deref() == Some(request_id) =>
                {
                    return summarize_child_request(ctx, request_id, ChildTerminalState::Completed)
                        .await;
                }
                EventV1::TaskCancelled(_)
                    if event.correlation_id.as_deref() == Some(request_id) =>
                {
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

async fn wait_for_background_request_terminal(
    ctx: &ToolContext,
    request_id: &str,
    timeout_ms: u64,
) -> Result<bool, ToolError> {
    let store = ctx
        .coordinator
        .event_store()
        .await
        .map_err(|err| ToolError::Execution(format!("failed to access event store: {err}")))?;
    let mut stream = store
        .subscribe(1)
        .map_err(|err| ToolError::Execution(format!("failed to subscribe to events: {err}")))?;
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);

    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let next =
            tokio::time::timeout(remaining.min(Duration::from_millis(250)), stream.next()).await;
        match next {
            Ok(Some(Ok(event))) => {
                if event.correlation_id.as_deref() == Some(request_id)
                    && matches!(
                        event.payload,
                        EventV1::TaskCompleted(_) | EventV1::TaskCancelled(_)
                    )
                {
                    return Ok(true);
                }
            }
            Ok(Some(Err(err))) => {
                return Err(ToolError::Execution(format!(
                    "failed to consume event stream: {err}"
                )));
            }
            Ok(None) | Err(_) => sleep(Duration::from_millis(10)).await,
        }
    }

    Ok(false)
}

async fn resolve_background_request_ref(
    ctx: &ToolContext,
    request: &BackgroundOutputRequest,
) -> Result<BackgroundRequestRef, ToolError> {
    let explicit_request_id = trimmed_selector(request.request_id.as_deref());
    let selector_hint = trimmed_selector(request.session_id.as_deref())
        .or_else(|| trimmed_selector(request.task_id.as_deref()));

    if explicit_request_id.is_none() && selector_hint.is_none() {
        return Err(ToolError::InvalidArguments(
            "provide request_id, task_id, or session_id returned by a background task call"
                .to_string(),
        ));
    }

    let store = ctx
        .coordinator
        .event_store()
        .await
        .map_err(|err| ToolError::Execution(format!("failed to access event store: {err}")))?;
    let mut replay = store
        .replay(1)
        .map_err(|err| ToolError::Execution(format!("failed to replay events: {err}")))?;
    let mut latest_request_id = None;
    let mut parent_by_agent = BTreeMap::new();
    let mut saw_matching_unauthorized = false;
    let mut saw_explicit_request = false;

    while let Some(next) = replay.next().await {
        let event = next
            .map_err(|err| ToolError::Execution(format!("failed to replay event stream: {err}")))?;
        match &event.payload {
            EventV1::AgentSpawned(data) => {
                if let Some(parent_agent_id) = data.parent_agent_id.as_deref() {
                    parent_by_agent.insert(data.agent_id.clone(), parent_agent_id.to_string());
                }
            }
            EventV1::TaskScheduled(data) => {
                let event_request_id = event.correlation_id.as_deref();
                let matches_explicit_request = explicit_request_id == event_request_id;
                let matches_session = selector_hint.is_some_and(|selector| {
                    event.actor.agent_id.as_deref() == Some(selector) || data.task_id == selector
                });
                if !matches_explicit_request && !matches_session {
                    continue;
                }
                if matches_explicit_request {
                    saw_explicit_request = true;
                }
                if background_request_authorized(
                    ctx,
                    &parent_by_agent,
                    event.actor.agent_id.as_deref(),
                ) {
                    latest_request_id = event.correlation_id.clone();
                } else {
                    saw_matching_unauthorized = true;
                }
            }
            _ => {}
        }
    }

    let request_id = match latest_request_id {
        Some(request_id) => request_id,
        None if saw_matching_unauthorized => {
            return Err(ToolError::InvalidArguments(
                "background request is not in the caller's task lineage".to_string(),
            ));
        }
        None if explicit_request_id.is_some() && !saw_explicit_request => {
            return Err(ToolError::InvalidArguments(format!(
                "could not resolve background request `{}`",
                explicit_request_id.expect("explicit request id checked")
            )));
        }
        None => {
            return Err(ToolError::InvalidArguments(format!(
                "could not resolve background request for task_id/session_id `{}`; pass the request_id returned by task(run_in_background=true)",
                selector_hint.expect("selector checked")
            )));
        }
    };

    Ok(BackgroundRequestRef {
        request_id,
        session_id_hint: selector_hint.map(str::to_string),
    })
}

fn background_request_authorized(
    ctx: &ToolContext,
    parent_by_agent: &BTreeMap<String, String>,
    request_agent_id: Option<&str>,
) -> bool {
    if ctx.actor.kind != ActorKind::Worker {
        return true;
    }
    let Some(caller_agent_id) = ctx.actor.agent_id.as_deref() else {
        return false;
    };
    let Some(mut candidate_agent_id) = request_agent_id else {
        return false;
    };

    if candidate_agent_id == caller_agent_id {
        return true;
    }

    let mut seen = BTreeSet::new();
    while seen.insert(candidate_agent_id.to_string()) {
        let Some(parent_agent_id) = parent_by_agent.get(candidate_agent_id) else {
            return false;
        };
        if parent_agent_id == caller_agent_id {
            return true;
        }
        candidate_agent_id = parent_agent_id;
    }

    false
}

async fn summarize_background_request(
    ctx: &ToolContext,
    request_ref: &BackgroundRequestRef,
) -> Result<BackgroundRequestSummary, ToolError> {
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
    let mut session_id = request_ref.session_id_hint.clone();
    let mut scheduler_task_id = None;
    let mut latest_scheduled_state = None;
    let mut result_summary = None;
    let mut failure_summary = None;
    let mut duration_ms = None;
    let mut terminal_status = None;
    let mut late_result = false;
    let mut saw_event = false;

    while let Some(next) = replay.next().await {
        let event = next
            .map_err(|err| ToolError::Execution(format!("failed to replay event stream: {err}")))?;
        if event.correlation_id.as_deref() != Some(request_ref.request_id.as_str()) {
            continue;
        }
        saw_event = true;

        match &event.payload {
            EventV1::TaskScheduled(data) => {
                latest_scheduled_state = Some(data.state);
                scheduler_task_id = Some(data.task_id.clone());
                session_id = event.actor.agent_id.clone().or(session_id);
                if data.state == TaskScheduleState::Started {
                    started_mono_ms.get_or_insert(event.mono_ms);
                }
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
                terminal_status = Some("completed".to_string());
                scheduler_task_id = Some(data.task_id.clone());
                result_summary = Some(data.result_summary.clone());
                let timing = data
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.timing.as_ref());
                started_mono_ms = timing
                    .and_then(|metadata| metadata.started_mono_ms)
                    .or(started_mono_ms);
                duration_ms = timing
                    .and_then(|metadata| metadata.elapsed_ms)
                    .or_else(|| elapsed_ms_from_events(started_mono_ms, event.mono_ms));
            }
            EventV1::TaskCancelled(data) => {
                terminal_status = Some("cancelled".to_string());
                scheduler_task_id = Some(data.task_id.clone());
                failure_summary = Some(data.reason.clone());
                duration_ms = elapsed_ms_from_events(started_mono_ms, event.mono_ms);
            }
            EventV1::TaskResultLate(_) => {
                late_result = true;
            }
            _ => {}
        }
    }

    let (status, terminal) = if let Some(status) = terminal_status {
        (status, true)
    } else {
        let status = match latest_scheduled_state {
            Some(TaskScheduleState::Started) => "running",
            Some(TaskScheduleState::Queued) => "queued",
            None if saw_event => "observed",
            None => "not_found",
        };
        (status.to_string(), false)
    };

    Ok(BackgroundRequestSummary {
        request_id: request_ref.request_id.clone(),
        session_id,
        scheduler_task_id,
        status,
        terminal,
        duration_ms,
        result_summary,
        failure_summary,
        tool_calls,
        late_result,
    })
}

fn trimmed_selector(selector: Option<&str>) -> Option<&str> {
    selector.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}

fn format_background_output(summary: &BackgroundRequestSummary, timed_out: bool) -> String {
    let task_id = summary.session_id.as_deref().unwrap_or("<unknown>");
    let duration = summary
        .duration_ms
        .map(|duration| format!("\nDuration: {duration} ms"))
        .unwrap_or_default();
    let timeout_note = if timed_out {
        "\nTimed out waiting for completion; the child task was not cancelled."
    } else {
        ""
    };
    let body = match summary.status.as_str() {
        "completed" => summary
            .result_summary
            .as_deref()
            .unwrap_or("Background task completed without a result summary."),
        "cancelled" => summary
            .failure_summary
            .as_deref()
            .unwrap_or("Background task was cancelled without a reason."),
        "running" => "Background task is still running.",
        "queued" => "Background task is queued and has not started yet.",
        "not_found" => "No events were found for this background request.",
        _ => "Background task has events but no terminal result yet.",
    };

    format!(
        "Task Result\n\nTask ID: {task_id}\nRequest ID: {}\nStatus: {}{}{}\n\n---\n\n{}",
        summary.request_id, summary.status, duration, timeout_note, body
    )
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
    let BatchCallOutcome {
        index,
        tool_id,
        canonical_tool_id,
        parameters,
        result,
    } = outcome;

    match result {
        Ok(value) => {
            let result = json!({
                "success": true,
                "status": "succeeded",
                "summary": value.display_text,
                "structured_output": value.structured_json,
                "artifacts": value.artifacts,
            });

            json!({
                "index": index,
                "tool_id": &tool_id,
                "canonical_tool_id": &canonical_tool_id,
                "request": batch_request_json(&tool_id, &canonical_tool_id, &parameters),
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
                "index": index,
                "tool_id": &tool_id,
                "canonical_tool_id": &canonical_tool_id,
                "request": batch_request_json(&tool_id, &canonical_tool_id, &parameters),
                "success": false,
                "status": "failed",
                "error": result.get("error").cloned(),
                "result": result,
            })
        }
    }
}

fn batch_request_json(
    tool_id: &str,
    canonical_tool_id: &Option<String>,
    parameters: &Value,
) -> Value {
    json!({
        "tool_id": tool_id,
        "canonical_tool_id": canonical_tool_id,
        "parameter_shape": parameter_shape(parameters),
        "parameter_keys": parameter_keys(parameters),
        "parameters_redacted": true,
    })
}

fn parameter_shape(parameters: &Value) -> &'static str {
    match parameters {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn parameter_keys(parameters: &Value) -> Vec<String> {
    parameters
        .as_object()
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        map_request_agent_turn_error, map_spawn_agent_error, select_profile_name, AgentSpawnRequest,
    };
    use harness_core::coord::CoordinatorError;
    use harness_core::tool::ToolError;

    #[test]
    fn select_profile_name_accepts_matching_category_and_subagent_type() {
        assert_eq!(
            select_profile_name(Some("child"), Some("child")).expect("matching selectors"),
            "child"
        );
    }

    #[test]
    fn select_profile_name_prefers_subagent_type_over_category_hint() {
        assert_eq!(
            select_profile_name(Some("quick"), Some("child"))
                .expect("direct subagent selector should win"),
            "child"
        );
    }

    #[test]
    fn select_profile_name_ignores_blank_selectors() {
        assert_eq!(
            select_profile_name(Some("  "), Some("child")).expect("blank category is ignored"),
            "child"
        );
    }

    #[test]
    fn unknown_existing_session_returns_guidance() {
        let err = map_request_agent_turn_error(
            CoordinatorError::UnknownAgent("missing-session".to_string()),
            &AgentSpawnRequest {
                description: "resume child".to_string(),
                profile_name: "deep".to_string(),
                category_selector: None,
                prompt: "resume".to_string(),
                task_id: Some("missing-session".to_string()),
                session_id: None,
                run_in_background: false,
                load_skills: Vec::new(),
                command: None,
            },
        );
        assert!(
            matches!(err, ToolError::InvalidArguments(message) if message.contains("Unknown child session `missing-session`") && message.contains("start a new child session"))
        );
    }

    #[test]
    fn unknown_child_profile_returns_guidance() {
        let err = map_spawn_agent_error(
            CoordinatorError::UnknownAgent("missing_profile".to_string()),
            &AgentSpawnRequest {
                description: "spawn child".to_string(),
                profile_name: "missing_profile".to_string(),
                category_selector: None,
                prompt: "inspect".to_string(),
                task_id: None,
                session_id: None,
                run_in_background: false,
                load_skills: Vec::new(),
                command: None,
            },
        );
        assert!(
            matches!(err, ToolError::InvalidArguments(message) if message.contains("Unknown child profile `missing_profile`") && message.contains("category/subagent_type `missing_profile`"))
        );
    }
}
