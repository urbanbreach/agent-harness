use std::time::Duration;

use harness_core::agent_catalog::{
    agent_catalog_category_binding, agent_catalog_display_order, agent_catalog_role,
};
use harness_core::coord::{AgentRuntimeInfo, ChildTaskRequestMetadata, CoordinatorError};
use harness_core::event::{
    ActorKind, EventActor, EventV1, TaskCancelledEvent, TaskCompletedEvent, TaskRouteMetadata,
    TaskScheduleState, TaskTerminalScope, ToolCallStatus,
};
use harness_core::proj::{BackgroundRequestProjection, BackgroundToolCallCounts};
use harness_core::store::{EventStoreError, EventStream};
use harness_core::tool::{canonical_tool_id_for, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::task::JoinSet;
use tokio::time::{sleep, Instant};
use tokio_stream::StreamExt;

use crate::control_plane::{
    render_task_skill_context, resolve_task_skill_context, TaskSkillContext,
};
use crate::text_json_tool_result;

const DEFAULT_TASK_WAIT_TIMEOUT_MS: u64 = 300_000;
const MAX_BACKGROUND_OUTPUT_TIMEOUT_MS: u64 = 300_000;
const MAX_BATCH_CALLS: usize = 25;
const BATCH_NESTED_ERROR: &str = "batch cannot be nested inside batch";
const BATCH_MAX_CALLS_ERROR: &str = "Maximum of 25 tools allowed in batch";

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
        let loaded_skills = resolve_task_skill_context(ctx, &request.load_skills).await?;

        let supervisor = EventActor::new(ActorKind::Supervisor, None);
        let existing_session_id = request.session_id.clone().or(request.task_id.clone());
        let resumed_existing_session = existing_session_id.is_some();
        let (agent_id, runtime) = if let Some(session_id) = existing_session_id {
            let target_info = ctx
                .coordinator
                .agent_runtime_info(session_id.clone())
                .await
                .map_err(|err| map_request_agent_turn_error(err, &request))?;
            authorize_existing_child_session(ctx, &request, &target_info)?;
            (session_id, target_info)
        } else {
            let agent_id = spawn_new_child_agent(ctx, &mut request, supervisor.clone()).await?;
            let runtime = ctx
                .coordinator
                .agent_runtime_info(agent_id.clone())
                .await
                .map_err(|err| map_request_agent_turn_error(err, &request))?;
            (agent_id, runtime)
        };
        let model_override = inherited_model_override(ctx, &runtime);
        let request_id = ctx
            .coordinator
            .request_child_agent_turn_with_model(
                supervisor,
                agent_id.clone(),
                build_child_prompt(&request, &loaded_skills),
                model_override
                    .as_ref()
                    .map(|(model_ref, _)| model_ref.clone()),
                model_override.map(|(_, settings)| settings),
                ChildTaskRequestMetadata {
                    parent_tool_call_id: ctx.tool_call_id.clone(),
                    parent_session_id: ctx.run_id.clone(),
                    parent_agent_id: ctx.actor.agent_id.clone(),
                    child_session_id: agent_id.clone(),
                    task_id: agent_id.clone(),
                    description: request.description.clone(),
                    run_in_background: request.run_in_background,
                    route: Some(child_task_route_metadata(
                        &request,
                        &child_runtime_metadata(&runtime),
                    )),
                },
            )
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
                child_runtime_metadata(&runtime),
                ChildRequestObservability {
                    status: "scheduled",
                    duration_ms: None,
                    result_summary: None,
                    failure_summary: None,
                    tool_calls: ChildToolCallCounts::default(),
                },
            );
            return Ok(text_json_tool_result(
                format!(
                    "task_id: {agent_id} (for resuming to continue this task if needed)\nrequest_id: {request_id}\n\n<task_result>Background task scheduled.</task_result>"
                ),
                spawn_result_json(
                    &request,
                    &loaded_skills,
                    &agent_id,
                    &request_id,
                    lineage.clone(),
                    &child_session,
                ),
            ));
        }

        let child_observability = wait_for_request_completion(ctx, &request_id).await?;
        let child_session = child_session_observability(
            &agent_id,
            &request_id,
            &request,
            resumed_existing_session,
            &permissions,
            child_runtime_metadata(&runtime),
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

        Ok(text_json_tool_result(
            format!(
                "task_id: {agent_id} (for resuming to continue this task if needed)\nrequest_id: {request_id}\n\n<task_result>\n{}\n</task_result>",
                task_result
            ),
            spawn_result_json(
                &request,
                &loaded_skills,
                &agent_id,
                &request_id,
                lineage,
                &child_session,
            ),
        ))
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

        Ok(text_json_tool_result(
            if failed == 0 {
                format!("All {successful} tools executed successfully.")
            } else {
                format!("Executed {successful} tools successfully. {failed} failed.")
            },
            json!({
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
            }),
        ))
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
        let request_id = trimmed_selector(request.request_id.as_deref()).map(str::to_string);
        let selector_hint = trimmed_selector(request.session_id.as_deref())
            .or_else(|| trimmed_selector(request.task_id.as_deref()))
            .map(str::to_string);
        let mut summary = background_summary_from_projection(
            ctx.coordinator
                .background_request_projection(
                    ctx.actor.clone(),
                    request_id.clone(),
                    selector_hint.clone(),
                )
                .await
                .map_err(map_background_request_error)?,
        );
        let mut cancel_requested_before_terminal = false;
        if request.cancel && !summary.terminal {
            let reason = request
                .reason
                .as_deref()
                .and_then(|reason| trimmed_selector(Some(reason)))
                .unwrap_or("cancelled by background_output")
                .to_string();
            summary = background_summary_from_projection(
                ctx.coordinator
                    .cancel_background_request(
                        ctx.actor.clone(),
                        request_id.clone(),
                        selector_hint.clone(),
                        reason.clone(),
                    )
                    .await
                    .map_err(map_background_request_error)?,
            );
            cancel_requested_before_terminal = true;
            if summary.status == "cancelled" {
                summary.cancel_reason = summary.failure_summary.clone().or(Some(reason));
            }
        }
        summary.cancel_requested = request.cancel;
        summary.cancel_performed =
            cancel_requested_before_terminal && summary.status == "cancelled";
        let mut timed_out = false;
        if request.block && !summary.terminal {
            let scheduler_task_id = summary.scheduler_task_id.clone().ok_or_else(|| {
                ToolError::InvalidArguments(format!(
                    "cannot wait for background request `{}` because no scheduler task id was observed yet",
                    summary.request_id
                ))
            })?;
            let observed_terminal = ctx
                .coordinator
                .wait_background_request_terminal(
                    summary.request_id.clone(),
                    scheduler_task_id,
                    request.timeout_ms,
                )
                .await
                .map_err(map_background_request_error)?;
            summary = background_summary_from_projection(
                ctx.coordinator
                    .background_request_projection(
                        ctx.actor.clone(),
                        request_id.clone(),
                        selector_hint.clone(),
                    )
                    .await
                    .map_err(map_background_request_error)?,
            );
            timed_out = !observed_terminal && !summary.terminal;
        }
        let child_runtime = background_child_runtime_metadata(ctx, &summary).await?;

        Ok(text_json_tool_result(
            format_background_output(&summary, timed_out),
            json!({
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
                "cancel_requested": summary.cancel_requested,
                "cancel_performed": summary.cancel_performed,
                "cancel_reason": summary.cancel_reason,
                "runtime": child_runtime,
                "child_runtime": child_runtime,
                "next_actions": background_next_actions(&summary),
                "source": "event_replay",
            }),
        ))
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentSelection {
    pub(crate) profile_name: String,
    pub(crate) category_selector: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct BackgroundOutputRequest {
    pub(crate) task_id: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) request_id: Option<String>,
    pub(crate) block: bool,
    pub(crate) timeout_ms: u64,
    pub(crate) cancel: bool,
    pub(crate) reason: Option<String>,
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
struct ChildRuntimeMetadata {
    profile: String,
    category: String,
    catalog_role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    category_binding: Option<String>,
    display_order: usize,
    model_ref: String,
    toolset: Vec<String>,
    can_redelegate: bool,
    has_background_output: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ChildRouteMetadata {
    requested_profile: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    requested_category: Option<String>,
    resolved_profile: String,
    resolved_category: String,
    resolved_catalog_role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_category_binding: Option<String>,
    resolved_display_order: usize,
    model_ref: String,
    can_redelegate: bool,
    category_fallback_applied: bool,
    explicit_subagent: bool,
    loaded_skills: Vec<String>,
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
    runtime: ChildRuntimeMetadata,
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
    cancel_requested: bool,
    cancel_performed: bool,
    cancel_reason: Option<String>,
}

fn background_summary_from_projection(
    projection: BackgroundRequestProjection,
) -> BackgroundRequestSummary {
    BackgroundRequestSummary {
        request_id: projection.request_id,
        session_id: projection.session_id,
        scheduler_task_id: projection.scheduler_task_id,
        status: projection.status,
        terminal: projection.terminal,
        duration_ms: projection.duration_ms,
        result_summary: projection.result_summary,
        failure_summary: projection.failure_summary,
        tool_calls: child_tool_call_counts_from_projection(projection.tool_calls),
        late_result: projection.late_result,
        cancel_requested: false,
        cancel_performed: false,
        cancel_reason: projection.cancel_reason,
    }
}

fn child_tool_call_counts_from_projection(counts: BackgroundToolCallCounts) -> ChildToolCallCounts {
    ChildToolCallCounts {
        requested: counts.requested,
        succeeded: counts.succeeded,
        failed: counts.failed,
    }
}

#[derive(Debug, Clone, Copy)]
enum ChildTerminalState {
    Completed,
    Failed,
    TimedOut,
}

fn build_child_prompt(request: &AgentSpawnRequest, loaded_skills: &[TaskSkillContext]) -> String {
    if loaded_skills.is_empty() && request.command.is_none() {
        return request.prompt.clone();
    }

    let mut prompt = String::from("Delegation context from parent:\n");
    if !loaded_skills.is_empty() {
        prompt.push_str("- Loaded skills: ");
        prompt.push_str(
            &loaded_skills
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        );
        prompt.push_str("\n\n");
        for skill in loaded_skills {
            prompt.push_str(&render_task_skill_context(skill));
            prompt.push_str("\n\n");
        }
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
    spawn_child_agent_once(ctx, request, supervisor)
        .await
        .map_err(|err| map_spawn_agent_error(err, request))
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

fn inherited_model_override(
    ctx: &ToolContext,
    runtime: &AgentRuntimeInfo,
) -> Option<(String, harness_core::agent::AgentModelSettings)> {
    if runtime.model_ref_explicit {
        return None;
    }

    Some((
        ctx.current_model_ref.clone()?,
        ctx.current_model_settings.clone().unwrap_or_default(),
    ))
}

pub(crate) fn select_agent_selection(
    category: Option<&str>,
    subagent_type: Option<&str>,
) -> Result<AgentSelection, ToolError> {
    let category = normalize_profile_selector(category).map(str::to_string);
    let subagent_type = normalize_subagent_selector(subagent_type);
    let category_selector = category_selector_for(&category, &subagent_type);
    let profile_name = match (category, subagent_type) {
        (Some(category), Some(subagent_type)) if category == subagent_type => Ok(category),
        (Some(_), Some(subagent_type)) => Ok(subagent_type),
        (Some(category), None) => Ok(category),
        (None, Some(subagent_type)) => Ok(subagent_type),
        (None, None) => Err(ToolError::InvalidArguments(
            "category or subagent_type is required".to_string(),
        )),
    }?;
    Ok(AgentSelection {
        profile_name,
        category_selector,
    })
}

fn category_selector_for(
    category: &Option<String>,
    subagent_type: &Option<String>,
) -> Option<String> {
    subagent_type.is_none().then(|| category.clone()).flatten()
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
        CoordinatorError::UnknownAgent(profile_name) => {
            if request.category_selector.is_some() {
                ToolError::InvalidArguments(format!(
                    "Unknown category `{profile_name}`. Configure that category profile before using task(category=...). Harness no longer applies an implicit general fallback."
                ))
            } else {
                ToolError::InvalidArguments(format!(
                    "Unknown child profile `{profile_name}`. Configure that agent profile before using task with category/subagent_type `{}`.",
                    request.profile_name
                ))
            }
        }
        other => ToolError::Execution(format!("failed to spawn agent: {other}")),
    }
}

fn map_background_request_error(err: CoordinatorError) -> ToolError {
    match err {
        CoordinatorError::UnknownTask(message)
        | CoordinatorError::PermissionDenied(message)
        | CoordinatorError::PolicyViolation(message) => ToolError::InvalidArguments(message),
        other => ToolError::Execution(format!("failed to inspect background request: {other}")),
    }
}

async fn wait_for_request_completion(
    ctx: &ToolContext,
    request_id: &str,
) -> Result<ChildRequestObservability, ToolError> {
    let mut stream = subscribe_events(ctx).await?;
    let deadline = Instant::now() + Duration::from_millis(DEFAULT_TASK_WAIT_TIMEOUT_MS);
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let next =
            tokio::time::timeout(remaining.min(Duration::from_millis(250)), stream.next()).await;
        match next {
            Ok(Some(Ok(event))) => match &event.payload {
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(request_id)
                        && task_completed_marks_child_agent_turn(data) =>
                {
                    return summarize_child_request(ctx, request_id, ChildTerminalState::Completed)
                        .await;
                }
                EventV1::TaskCancelled(data)
                    if event.correlation_id.as_deref() == Some(request_id)
                        && task_cancelled_marks_child_agent_turn(data) =>
                {
                    return summarize_child_request(ctx, request_id, ChildTerminalState::Failed)
                        .await;
                }
                _ => {}
            },
            Ok(Some(Err(err))) => {
                return Err(map_event_stream_error(err));
            }
            Ok(None) | Err(_) => {
                sleep(Duration::from_millis(10)).await;
            }
        }
    }
    summarize_child_request(ctx, request_id, ChildTerminalState::TimedOut).await
}

async fn background_child_runtime_metadata(
    ctx: &ToolContext,
    summary: &BackgroundRequestSummary,
) -> Result<Option<ChildRuntimeMetadata>, ToolError> {
    let Some(session_id) = summary.session_id.as_ref() else {
        return Ok(None);
    };
    let runtime = ctx
        .coordinator
        .agent_runtime_info(session_id.clone())
        .await
        .map_err(|err| ToolError::Execution(format!("failed to inspect child runtime: {err}")))?;
    Ok(Some(child_runtime_metadata(&runtime)))
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
    let cancel_note = if summary.cancel_performed {
        "\nCancellation requested through the coordinator."
    } else if summary.cancel_requested && summary.terminal {
        "\nCancellation was requested after the child task was already terminal."
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
        "Task Result\n\nTask ID: {task_id}\nRequest ID: {}\nStatus: {}{}{}{}\n\n---\n\n{}",
        summary.request_id, summary.status, duration, timeout_note, cancel_note, body
    )
}

async fn summarize_child_request(
    ctx: &ToolContext,
    request_id: &str,
    terminal_state: ChildTerminalState,
) -> Result<ChildRequestObservability, ToolError> {
    let mut replay = replay_events(ctx).await?;

    let mut tool_calls = ChildToolCallCounts::default();
    let mut started_mono_ms = None;
    let mut observed_result_summary = None;
    let mut observed_failure_summary = None;
    let mut observed_duration_ms = None;
    let mut observed_status = None;

    while let Some(next) = replay.next().await {
        let event = next.map_err(map_replay_stream_error)?;
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
            EventV1::TaskCompleted(data) if task_completed_marks_child_agent_turn(data) => {
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
            EventV1::TaskCancelled(data) if task_cancelled_marks_child_agent_turn(data) => {
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

async fn subscribe_events(ctx: &ToolContext) -> Result<EventStream, ToolError> {
    let store = ctx
        .coordinator
        .event_store()
        .await
        .map_err(|err| ToolError::Execution(format!("failed to access event store: {err}")))?;
    store
        .subscribe(1)
        .map_err(|err| ToolError::Execution(format!("failed to subscribe to events: {err}")))
}

async fn replay_events(ctx: &ToolContext) -> Result<EventStream, ToolError> {
    let store = ctx
        .coordinator
        .event_store()
        .await
        .map_err(|err| ToolError::Execution(format!("failed to access event store: {err}")))?;
    store
        .replay(1)
        .map_err(|err| ToolError::Execution(format!("failed to replay events: {err}")))
}

fn map_event_stream_error(err: EventStoreError) -> ToolError {
    ToolError::Execution(format!("failed to consume event stream: {err}"))
}

fn map_replay_stream_error(err: EventStoreError) -> ToolError {
    ToolError::Execution(format!("failed to replay event stream: {err}"))
}

fn elapsed_ms_from_events(started_mono_ms: Option<u64>, finished_mono_ms: u64) -> Option<u64> {
    started_mono_ms.map(|started| finished_mono_ms.saturating_sub(started))
}

fn task_completed_marks_child_agent_turn(data: &TaskCompletedEvent) -> bool {
    data.metadata
        .as_ref()
        .and_then(|metadata| metadata.task_scope)
        .is_some_and(|scope| matches!(scope, TaskTerminalScope::AgentTurn))
}

fn task_cancelled_marks_child_agent_turn(data: &TaskCancelledEvent) -> bool {
    data.task_scope
        .is_some_and(|scope| matches!(scope, TaskTerminalScope::AgentTurn))
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

fn child_runtime_metadata(runtime: &AgentRuntimeInfo) -> ChildRuntimeMetadata {
    ChildRuntimeMetadata {
        profile: runtime.profile_name.clone(),
        category: runtime.profile_category.clone(),
        catalog_role: agent_catalog_role(&runtime.profile_name).to_string(),
        category_binding: agent_catalog_category_binding(&runtime.profile_name),
        display_order: agent_catalog_display_order(&runtime.profile_name, 0),
        model_ref: runtime.model_ref.clone(),
        can_redelegate: runtime.toolset.iter().any(|tool| tool == "task"),
        has_background_output: runtime
            .toolset
            .iter()
            .any(|tool| tool == "background_output"),
        toolset: runtime.toolset.clone(),
        parent_agent_id: runtime.parent_agent_id.clone(),
    }
}

fn child_task_route_metadata(
    request: &AgentSpawnRequest,
    runtime: &ChildRuntimeMetadata,
) -> TaskRouteMetadata {
    let route = child_route_metadata(request, runtime);
    TaskRouteMetadata {
        requested_profile: Some(route.requested_profile),
        requested_category: route.requested_category,
        resolved_profile: Some(route.resolved_profile),
        resolved_category: Some(route.resolved_category),
        resolved_catalog_role: Some(runtime.catalog_role.clone()),
        resolved_category_binding: runtime.category_binding.clone(),
        resolved_display_order: Some(runtime.display_order),
        model_ref: Some(runtime.model_ref.clone()),
        can_redelegate: Some(runtime.can_redelegate),
        category_fallback_applied: Some(route.category_fallback_applied),
        explicit_subagent: Some(route.explicit_subagent),
        loaded_skills: route.loaded_skills,
    }
}

fn child_route_metadata(
    request: &AgentSpawnRequest,
    runtime: &ChildRuntimeMetadata,
) -> ChildRouteMetadata {
    let requested_profile = request
        .category_selector
        .clone()
        .unwrap_or_else(|| request.profile_name.clone());
    ChildRouteMetadata {
        requested_profile,
        requested_category: request.category_selector.clone(),
        resolved_profile: runtime.profile.clone(),
        resolved_category: runtime.category.clone(),
        resolved_catalog_role: runtime.catalog_role.clone(),
        resolved_category_binding: runtime.category_binding.clone(),
        resolved_display_order: runtime.display_order,
        model_ref: runtime.model_ref.clone(),
        can_redelegate: runtime.can_redelegate,
        category_fallback_applied: request
            .category_selector
            .as_ref()
            .is_some_and(|category| category != &runtime.profile),
        explicit_subagent: request.category_selector.is_none(),
        loaded_skills: request.load_skills.clone(),
    }
}

fn background_next_actions(summary: &BackgroundRequestSummary) -> Value {
    let mut actions = Vec::new();
    actions.push(json!({
        "action": "check_status",
        "tool": "background_output",
        "parameters": { "request_id": summary.request_id, "block": false },
    }));
    if !summary.terminal {
        actions.push(json!({
            "action": "wait_for_result",
            "tool": "background_output",
            "parameters": { "request_id": summary.request_id, "block": true },
        }));
        actions.push(json!({
            "action": "cancel",
            "tool": "background_output",
            "parameters": {
                "request_id": summary.request_id,
                "cancel": true,
                "reason": "cancelled by parent request"
            },
        }));
    }
    Value::Array(actions)
}

fn child_next_actions(request: &AgentSpawnRequest, agent_id: &str, request_id: &str) -> Value {
    let mut actions = Vec::new();
    if request.run_in_background {
        actions.push(json!({
            "action": "check_status",
            "tool": "background_output",
            "parameters": { "request_id": request_id, "block": false },
        }));
        actions.push(json!({
            "action": "wait_for_result",
            "tool": "background_output",
            "parameters": { "request_id": request_id, "block": true },
        }));
        actions.push(json!({
            "action": "cancel",
            "tool": "background_output",
            "parameters": {
                "request_id": request_id,
                "cancel": true,
                "reason": "cancelled by parent request"
            },
        }));
    }
    actions.push(json!({
        "action": "continue_task",
        "tool": "task",
        "parameters": {
            "session_id": agent_id,
            "subagent_type": request.profile_name,
            "description": format!("Continue {}", request.description),
            "prompt": "Continue this child task with additional instructions.",
            "run_in_background": false,
            "load_skills": []
        },
    }));
    Value::Array(actions)
}

fn child_session_observability(
    session_id: &str,
    request_id: &str,
    request: &AgentSpawnRequest,
    resumed_existing_session: bool,
    permissions: &ChildPermissionMetadata,
    runtime: ChildRuntimeMetadata,
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
        runtime,
    }
}

fn spawn_result_json(
    request: &AgentSpawnRequest,
    loaded_skills: &[TaskSkillContext],
    agent_id: &str,
    request_id: &str,
    lineage: Value,
    child_session: &ChildSessionObservability,
) -> Value {
    let route = child_route_metadata(request, &child_session.runtime);
    json!({
        "description": request.description,
        "profile": request.profile_name,
        "category": request.category_selector.clone(),
        "route": route.clone(),
        "resolved_route": route,
        "task_id": agent_id,
        "session_id": agent_id,
        "request_id": request_id,
        "child_session_id": agent_id,
        "child_request_id": request_id,
        "lineage": lineage,
        "load_skills": request.load_skills,
        "skills": request.load_skills,
        "loaded_skills": loaded_skill_metadata(loaded_skills),
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
        "runtime": child_session.runtime,
        "child_runtime": child_session.runtime,
        "effective_model_ref": child_session.runtime.model_ref,
        "child_toolset": child_session.runtime.toolset,
        "can_redelegate": child_session.runtime.can_redelegate,
        "has_background_output": child_session.runtime.has_background_output,
        "next_actions": child_next_actions(request, agent_id, request_id),
        "child_session": child_session,
    })
}

fn loaded_skill_metadata(loaded_skills: &[TaskSkillContext]) -> Value {
    Value::Array(
        loaded_skills
            .iter()
            .map(|skill| {
                json!({
                    "name": skill.name,
                    "description": skill.description,
                    "location": skill.location.display().to_string(),
                    "policy": skill.policy,
                })
            })
            .collect(),
    )
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
        map_request_agent_turn_error, map_spawn_agent_error, select_agent_selection,
        AgentSelection, AgentSpawnRequest,
    };
    use harness_core::coord::CoordinatorError;
    use harness_core::tool::ToolError;

    #[test]
    fn select_agent_selection_accepts_matching_category_and_subagent_type() {
        assert_eq!(
            select_agent_selection(Some("child"), Some("child"))
                .expect("matching selectors")
                .profile_name,
            "child"
        );
    }

    #[test]
    fn select_agent_selection_prefers_subagent_type_over_category_hint() {
        assert_eq!(
            select_agent_selection(Some("quick"), Some("child"))
                .expect("direct subagent selector should win"),
            AgentSelection {
                profile_name: "child".to_string(),
                category_selector: None,
            }
        );
    }

    #[test]
    fn select_agent_selection_uses_category_as_fallback_hint_for_category_only_request() {
        assert_eq!(
            select_agent_selection(Some("quick"), None).expect("category-only selector should win"),
            AgentSelection {
                profile_name: "quick".to_string(),
                category_selector: Some("quick".to_string()),
            }
        );
    }

    #[test]
    fn select_agent_selection_ignores_blank_selectors() {
        assert_eq!(
            select_agent_selection(Some("  "), Some("child"))
                .expect("blank category is ignored")
                .profile_name,
            "child"
        );
    }

    #[test]
    fn select_agent_selection_preserves_category_fallback_only_for_category_requests() {
        let category_only = select_agent_selection(Some("quick"), None)
            .expect("category-only selection should be accepted");
        assert_eq!(category_only.profile_name, "quick");
        assert_eq!(category_only.category_selector.as_deref(), Some("quick"));

        let explicit_subagent = select_agent_selection(Some("quick"), Some("child"))
            .expect("explicit subagent should be accepted");
        assert_eq!(explicit_subagent.profile_name, "child");
        assert_eq!(explicit_subagent.category_selector, None);
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
