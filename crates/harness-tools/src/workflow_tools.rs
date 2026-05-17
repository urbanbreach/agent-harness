use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_core::event::EventEnvelopeV1;
use harness_core::persistent_task::{project_persistent_tasks, PersistentTaskProjection};
use harness_core::run_dossier::build_run_dossier_with_tasks_and_closeout_policy;
use harness_core::tool::{ArtifactRef, Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use harness_core::workflow::{
    project_workflows, WorkflowEvidenceRequest, WorkflowProjection, WorkflowQuestionProjection,
    WorkflowSignoffPolicy, WORKFLOW_QUESTION_EVIDENCE_CATEGORY,
    WORKFLOW_QUESTION_METADATA_ANSWER_REF, WORKFLOW_QUESTION_METADATA_ID,
    WORKFLOW_QUESTION_METADATA_PROMPT_REF, WORKFLOW_QUESTION_METADATA_REASON_CODE,
    WORKFLOW_QUESTION_METADATA_STATUS, WORKFLOW_QUESTION_STATUS_ANSWERED,
    WORKFLOW_QUESTION_STATUS_ASKED, WORKFLOW_QUESTION_STATUS_CLOSED,
    WORKFLOW_QUESTION_STATUS_ERROR, WORKFLOW_QUESTION_STATUS_TIMED_OUT,
};
use harness_core::workflow_closeout::{
    WorkflowCatalogHealthReport, WorkflowCloseoutPolicy, WorkflowCloseoutPolicyConfig,
    WorkflowCloseoutReadiness, WorkflowSignoffDecision, WorkflowSignoffReport,
    WorkflowStatusCloseoutReport, WORKFLOW_CLOSEOUT_DEFAULT_POLICY_ID,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_stream::StreamExt;

use crate::{parse_tool_args, text_json_artifacts_tool_result, text_json_tool_result};

pub(crate) struct WorkflowStatusTool;
pub(crate) struct WorkflowSignoffTool;
pub(crate) struct WorkflowDossierExportTool;
pub(crate) struct WorkflowQuestionRecordTool;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WorkflowStatusArgs {
    #[serde(default)]
    workflow_id: Option<String>,
    #[serde(default)]
    policy_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WorkflowSignoffArgs {
    workflow_id: String,
    decision: WorkflowSignoffDecision,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default = "default_operator")]
    operator: String,
    #[serde(default)]
    policy_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum WorkflowDossierFormat {
    Json,
    Markdown,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WorkflowDossierExportArgs {
    #[serde(default)]
    workflow_id: Option<String>,
    #[serde(default)]
    policy_id: Option<String>,
    #[serde(default = "default_dossier_format")]
    format: WorkflowDossierFormat,
    #[serde(default)]
    output_artifact: bool,
    #[serde(default)]
    slug: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum WorkflowQuestionRecordStatus {
    Asked,
    Answered,
    Closed,
    TimedOut,
    Error,
}

impl WorkflowQuestionRecordStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Asked => WORKFLOW_QUESTION_STATUS_ASKED,
            Self::Answered => WORKFLOW_QUESTION_STATUS_ANSWERED,
            Self::Closed => WORKFLOW_QUESTION_STATUS_CLOSED,
            Self::TimedOut => WORKFLOW_QUESTION_STATUS_TIMED_OUT,
            Self::Error => WORKFLOW_QUESTION_STATUS_ERROR,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WorkflowQuestionRecordArgs {
    workflow_id: String,
    question_id: String,
    status: WorkflowQuestionRecordStatus,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    reason_code: Option<String>,
    #[serde(default)]
    prompt_ref: Option<String>,
    #[serde(default)]
    answer_ref: Option<String>,
}

#[derive(Debug, Serialize)]
struct WorkflowQuestionRecordReport {
    workflow_id: String,
    question_id: String,
    status: String,
    accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    question: Option<WorkflowQuestionProjection>,
    closeout: WorkflowCloseoutReadiness,
}

#[derive(Debug, Serialize)]
struct WorkflowToolStatusReport {
    run_id: String,
    workflow_count: usize,
    active_count: usize,
    projection: WorkflowProjection,
    persistent_tasks: PersistentTaskProjection,
    closeout: BTreeMap<String, WorkflowStatusCloseoutReport>,
    catalog_health: WorkflowCatalogHealthReport,
}

fn default_operator() -> String {
    "agent".to_string()
}

fn default_dossier_format() -> WorkflowDossierFormat {
    WorkflowDossierFormat::Json
}

#[async_trait]
impl Tool for WorkflowStatusTool {
    fn id(&self) -> &str {
        "workflow_status"
    }

    fn description(&self) -> &str {
        "Reads replay-derived workflow status for the current run, including shared closeout readiness and legal next actions. This tool is projection-only and appends no events."
    }

    fn parameters_json_schema(&self) -> Value {
        crate::json_schema_for::<WorkflowStatusArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: WorkflowStatusArgs = parse_tool_args(args_json)?;
        let events = replay_events(&ctx).await?;
        let policy = closeout_policy(args.policy_id.as_deref())?;
        let catalog_health = crate::workflow_catalog_health_report(ctx.workspace_root.as_path())?;
        let report = workflow_status_report(
            &ctx.run_id,
            &events,
            args.workflow_id,
            &policy,
            catalog_health,
        )?;
        Ok(text_json_tool_result(
            format!(
                "workflow_status: {} workflow(s), {} active",
                report.workflow_count, report.active_count
            ),
            serde_json::to_value(report).map_err(|err| {
                ToolError::Execution(format!("failed to serialize workflow status: {err}"))
            })?,
        ))
    }
}

#[async_trait]
impl Tool for WorkflowSignoffTool {
    fn id(&self) -> &str {
        "workflow_signoff"
    }

    fn description(&self) -> &str {
        "Records a coordinator-owned workflow signoff decision against a workflow in the current run. Non-approve decisions require a reason; waive/redirect require scope. Approval is denied until closeout readiness allows it."
    }

    fn parameters_json_schema(&self) -> Value {
        crate::json_schema_for::<WorkflowSignoffArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::EditFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: WorkflowSignoffArgs = parse_tool_args(args_json)?;
        validate_signoff_args(&args)?;
        let policy = closeout_policy(args.policy_id.as_deref())?;
        let signoff_policy = WorkflowSignoffPolicy::simulator_default();
        let before = replay_events(&ctx).await?;
        let projection = project_workflows(before.iter().map(|event| &event.payload));
        let workflow = projection.workflows.get(&args.workflow_id).ok_or_else(|| {
            ToolError::Execution(format!(
                "workflow `{}` was not found in current run {}",
                args.workflow_id, ctx.run_id
            ))
        })?;
        if workflow.terminal {
            if args.decision == WorkflowSignoffDecision::Approve
                && workflow.status == "outcome.finished"
            {
                let closeout = closeout_readiness_for_report(
                    &ctx.run_id,
                    &projection,
                    &project_persistent_tasks(&before),
                    &signoff_policy,
                    &policy,
                    &args.workflow_id,
                );
                return Ok(text_json_tool_result(
                    format!("workflow_signoff: {} already approved", args.workflow_id),
                    serde_json::to_value(WorkflowSignoffReport {
                        workflow_id: args.workflow_id,
                        decision: args.decision,
                        audit_only: false,
                        accepted: true,
                        closeout,
                        reason: args.reason,
                    })
                    .map_err(|err| {
                        ToolError::Execution(format!("failed to serialize signoff report: {err}"))
                    })?,
                ));
            }
            return Err(ToolError::Execution(format!(
                "workflow `{}` is already terminal with status `{}`",
                args.workflow_id, workflow.status
            )));
        }

        if args.decision == WorkflowSignoffDecision::ApproveLive && !policy.allow_live_approval {
            return Err(ToolError::Execution(format!(
                "workflow closeout policy `{}` does not allow approve_live",
                policy.policy_id
            )));
        }

        let operator_decision = workflow_operator_decision(&args.decision, args.scope.as_deref())?;
        if args.decision == WorkflowSignoffDecision::Approve {
            ctx.coordinator
                .complete_workflow_with_closeout_policy(
                    ctx.actor.clone(),
                    args.workflow_id.clone(),
                    "outcome.finished",
                    args.reason.clone().unwrap_or_else(|| {
                        "workflow signoff decision: signoff-approved".to_string()
                    }),
                    args.operator.clone(),
                    signoff_policy.clone(),
                    policy.clone(),
                )
                .await
                .map_err(|err| ToolError::Execution(err.to_string()))?;
        } else {
            ctx.coordinator
                .record_workflow_operator_decision(
                    ctx.actor.clone(),
                    args.workflow_id.clone(),
                    operator_decision.clone(),
                    args.operator.clone(),
                    args.reason.clone(),
                    None,
                )
                .await
                .map_err(|err| ToolError::Execution(err.to_string()))?;
            if let Some(outcome) = terminal_outcome_for_decision(&args.decision) {
                ctx.coordinator
                    .complete_workflow(
                        ctx.actor.clone(),
                        args.workflow_id.clone(),
                        outcome,
                        args.reason.clone().unwrap_or_else(|| {
                            format!("workflow signoff decision: {operator_decision}")
                        }),
                        args.operator.clone(),
                    )
                    .await
                    .map_err(|err| ToolError::Execution(err.to_string()))?;
            }
        }

        let after = replay_events(&ctx).await?;
        let refreshed_projection = project_workflows(after.iter().map(|event| &event.payload));
        let refreshed_tasks = project_persistent_tasks(&after);
        let closeout = closeout_readiness_for_report(
            &ctx.run_id,
            &refreshed_projection,
            &refreshed_tasks,
            &signoff_policy,
            &policy,
            &args.workflow_id,
        );
        let report = WorkflowSignoffReport {
            workflow_id: args.workflow_id,
            decision: args.decision,
            audit_only: false,
            accepted: true,
            closeout,
            reason: args.reason,
        };
        Ok(text_json_tool_result(
            format!(
                "workflow_signoff: {} decision={} accepted",
                report.workflow_id, operator_decision
            ),
            serde_json::to_value(report).map_err(|err| {
                ToolError::Execution(format!("failed to serialize signoff report: {err}"))
            })?,
        ))
    }
}

#[async_trait]
impl Tool for WorkflowDossierExportTool {
    fn id(&self) -> &str {
        "workflow_dossier_export"
    }

    fn description(&self) -> &str {
        "Exports a replay-derived Run Dossier for the current run. It can return JSON/markdown and optionally write a tool artifact; it does not mark workflow closeout complete."
    }

    fn parameters_json_schema(&self) -> Value {
        crate::json_schema_for::<WorkflowDossierExportArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: WorkflowDossierExportArgs = parse_tool_args(args_json)?;
        let events = replay_events(&ctx).await?;
        let policy = closeout_policy(args.policy_id.as_deref())?;
        let catalog_health = crate::workflow_catalog_health_report(ctx.workspace_root.as_path())?;
        let status = workflow_status_report(
            &ctx.run_id,
            &events,
            args.workflow_id.clone(),
            &policy,
            catalog_health,
        )?;
        let dossier = build_run_dossier_with_tasks_and_closeout_policy(
            &status.projection,
            &status.persistent_tasks,
            &WorkflowSignoffPolicy::simulator_default(),
            &policy,
        );
        let body = match args.format {
            WorkflowDossierFormat::Json => {
                serde_json::to_string_pretty(&dossier).map_err(|err| {
                    ToolError::Execution(format!("failed to render dossier JSON: {err}"))
                })?
            }
            WorkflowDossierFormat::Markdown => render_dossier_markdown(&dossier),
        };
        let artifacts = if args.output_artifact {
            let slug = args
                .slug
                .as_deref()
                .unwrap_or("workflow-dossier")
                .trim()
                .trim_matches('/');
            if slug.is_empty() || slug.contains("..") {
                return Err(ToolError::InvalidArguments(
                    "slug must be non-empty and must not contain `..`".to_string(),
                ));
            }
            let extension = match args.format {
                WorkflowDossierFormat::Json => "json",
                WorkflowDossierFormat::Markdown => "md",
            };
            vec![ctx
                .artifact_store()
                .map_err(|err| ToolError::Execution(err.to_string()))?
                .write_text(&format!("workflow_dossiers/{slug}.{extension}"), &body)
                .map_err(|err| ToolError::Execution(err.to_string()))?]
        } else {
            Vec::<ArtifactRef>::new()
        };
        Ok(text_json_artifacts_tool_result(
            format!(
                "workflow_dossier_export: {} workflow(s), format={:?}, artifact_written={}",
                dossier.workflows.len(),
                args.format,
                !artifacts.is_empty()
            ),
            json!({
                "run_id": ctx.run_id,
                "format": match args.format {
                    WorkflowDossierFormat::Json => "json",
                    WorkflowDossierFormat::Markdown => "markdown",
                },
                "dossier": dossier,
                "body": body,
            }),
            artifacts,
        ))
    }
}

#[async_trait]
impl Tool for WorkflowQuestionRecordTool {
    fn id(&self) -> &str {
        "workflow_question_record"
    }

    fn description(&self) -> &str {
        "Records replay-visible workflow question lifecycle state (asked, answered, closed, timed_out, error) through the coordinator. Unknown, closed, or malformed question transitions fail with stable reason codes."
    }

    fn parameters_json_schema(&self) -> Value {
        crate::json_schema_for::<WorkflowQuestionRecordArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::EditFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: WorkflowQuestionRecordArgs = parse_tool_args(args_json)?;
        let before = replay_events(&ctx).await?;
        let projection = project_workflows(before.iter().map(|event| &event.payload));
        let existing = validate_question_record_args(&args, &projection)?;
        let mut metadata = BTreeMap::from([
            (
                WORKFLOW_QUESTION_METADATA_ID.to_string(),
                args.question_id.clone(),
            ),
            (
                WORKFLOW_QUESTION_METADATA_STATUS.to_string(),
                args.status.as_str().to_string(),
            ),
        ]);
        let reason_code = args
            .reason_code
            .clone()
            .or_else(|| default_question_reason_code(args.status, existing));
        if let Some(reason_code) = reason_code.as_ref() {
            metadata.insert(
                WORKFLOW_QUESTION_METADATA_REASON_CODE.to_string(),
                reason_code.clone(),
            );
        }
        insert_optional_metadata(
            &mut metadata,
            WORKFLOW_QUESTION_METADATA_PROMPT_REF,
            args.prompt_ref.as_ref(),
        );
        insert_optional_metadata(
            &mut metadata,
            WORKFLOW_QUESTION_METADATA_ANSWER_REF,
            args.answer_ref.as_ref(),
        );

        let status = args.status.as_str().to_string();
        ctx.coordinator
            .record_workflow_evidence(
                ctx.actor.clone(),
                WorkflowEvidenceRequest {
                    workflow_id: args.workflow_id.clone(),
                    category: WORKFLOW_QUESTION_EVIDENCE_CATEGORY.to_string(),
                    summary: args.summary.clone().unwrap_or_else(|| {
                        format!(
                            "workflow question `{}` lifecycle status `{status}`",
                            args.question_id
                        )
                    }),
                    artifact_path: None,
                    artifact_digest: None,
                    acceptance_ref: Some(args.question_id.clone()),
                    metadata,
                },
            )
            .await
            .map_err(|err| ToolError::Execution(err.to_string()))?;

        let after = replay_events(&ctx).await?;
        let refreshed_projection = project_workflows(after.iter().map(|event| &event.payload));
        let persistent_tasks = project_persistent_tasks(&after);
        let closeout_policy = closeout_policy(None)?;
        let signoff_policy = WorkflowSignoffPolicy::simulator_default();
        let closeout = closeout_readiness_for_report(
            &ctx.run_id,
            &refreshed_projection,
            &persistent_tasks,
            &signoff_policy,
            &closeout_policy,
            &args.workflow_id,
        );
        let question = refreshed_projection
            .questions
            .get(&args.question_id)
            .cloned();
        let report = WorkflowQuestionRecordReport {
            workflow_id: args.workflow_id,
            question_id: args.question_id,
            status,
            accepted: true,
            reason_code,
            question,
            closeout,
        };
        Ok(text_json_tool_result(
            format!(
                "workflow_question_record: {} status={} accepted",
                report.question_id, report.status
            ),
            serde_json::to_value(report).map_err(|err| {
                ToolError::Execution(format!("failed to serialize question report: {err}"))
            })?,
        ))
    }
}

async fn replay_events(ctx: &ToolContext) -> Result<Vec<EventEnvelopeV1>, ToolError> {
    let store = ctx
        .coordinator
        .event_store()
        .await
        .map_err(|err| ToolError::Execution(format!("failed to access event store: {err}")))?;
    let mut stream = store
        .replay(1)
        .map_err(|err| ToolError::Execution(format!("failed to replay events: {err}")))?;
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(
            event.map_err(|err| ToolError::Execution(format!("failed to read event: {err}")))?,
        );
    }
    Ok(events)
}

fn workflow_status_report(
    run_id: &str,
    events: &[EventEnvelopeV1],
    workflow_id: Option<String>,
    closeout_policy: &WorkflowCloseoutPolicy,
    catalog_health: WorkflowCatalogHealthReport,
) -> Result<WorkflowToolStatusReport, ToolError> {
    let mut projection = project_workflows(events.iter().map(|event| &event.payload));
    let mut persistent_tasks = project_persistent_tasks(events);
    if let Some(workflow_id) = workflow_id {
        projection
            .workflows
            .retain(|id, _| id.as_str() == workflow_id.as_str());
        projection.evidence.retain(|id, _| id == &workflow_id);
        projection
            .continuations
            .retain(|_, continuation| continuation.workflow_id == workflow_id);
        projection
            .questions
            .retain(|_, question| question.workflow_id == workflow_id);
        projection
            .teams
            .retain(|_, team| team.workflow_id == workflow_id);
        projection
            .plan_consensus
            .retain(|_, plan| plan.workflow_id == workflow_id);
        projection
            .goal_ledger
            .goals
            .retain(|_, goal| goal.workflow_id == workflow_id);
        projection
            .research_missions
            .missions
            .retain(|_, mission| mission.workflow_id == workflow_id);
        persistent_tasks.tasks.retain(|_, task| {
            task.metadata
                .get(harness_core::workflow::WORKFLOW_TASK_METADATA_KEY)
                == Some(&workflow_id)
        });
    }
    let signoff_policy = WorkflowSignoffPolicy::simulator_default();
    let closeout = projection
        .workflows
        .keys()
        .map(|workflow_id| {
            let readiness = closeout_readiness_for_report(
                run_id,
                &projection,
                &persistent_tasks,
                &signoff_policy,
                closeout_policy,
                workflow_id,
            );
            (
                workflow_id.clone(),
                WorkflowStatusCloseoutReport {
                    closeout: readiness,
                },
            )
        })
        .collect();
    let workflow_count = projection.workflows.len();
    let active_count = projection
        .workflows
        .values()
        .filter(|workflow| !workflow.terminal)
        .count();
    Ok(WorkflowToolStatusReport {
        run_id: run_id.to_string(),
        workflow_count,
        active_count,
        projection,
        persistent_tasks,
        closeout,
        catalog_health,
    })
}

fn validate_question_record_args<'a>(
    args: &WorkflowQuestionRecordArgs,
    projection: &'a WorkflowProjection,
) -> Result<Option<&'a WorkflowQuestionProjection>, ToolError> {
    if args.workflow_id.trim().is_empty() {
        return Err(question_record_error("missing_workflow_id"));
    }
    if args.question_id.trim().is_empty() {
        return Err(question_record_error("missing_question_id"));
    }
    if !projection.workflows.contains_key(&args.workflow_id) {
        return Err(question_record_error("unknown_workflow"));
    }
    let existing = projection.questions.get(&args.question_id);
    if existing.is_some_and(|question| question.workflow_id != args.workflow_id) {
        return Err(question_record_error("question_workflow_mismatch"));
    }
    if existing.is_some_and(|question| question.status == WORKFLOW_QUESTION_STATUS_CLOSED)
        && args.status != WorkflowQuestionRecordStatus::Closed
    {
        return Err(question_record_error("question_closed"));
    }
    match args.status {
        WorkflowQuestionRecordStatus::Asked => Ok(existing),
        WorkflowQuestionRecordStatus::Answered => {
            let Some(existing) = existing else {
                return Err(question_record_error("unknown_question"));
            };
            if args
                .answer_ref
                .as_deref()
                .is_none_or(|answer_ref| answer_ref.trim().is_empty())
            {
                return Err(question_record_error("malformed_answer"));
            }
            Ok(Some(existing))
        }
        WorkflowQuestionRecordStatus::Closed
        | WorkflowQuestionRecordStatus::TimedOut
        | WorkflowQuestionRecordStatus::Error => {
            let Some(existing) = existing else {
                return Err(question_record_error("unknown_question"));
            };
            Ok(Some(existing))
        }
    }
}

fn question_record_error(reason_code: &str) -> ToolError {
    ToolError::InvalidArguments(format!(
        "workflow_question_record rejected: reason_code={reason_code}"
    ))
}

fn default_question_reason_code(
    status: WorkflowQuestionRecordStatus,
    existing: Option<&WorkflowQuestionProjection>,
) -> Option<String> {
    match status {
        WorkflowQuestionRecordStatus::TimedOut => Some("question_timed_out".to_string()),
        WorkflowQuestionRecordStatus::Error => Some("question_error".to_string()),
        WorkflowQuestionRecordStatus::Closed => Some("question_closed".to_string()),
        WorkflowQuestionRecordStatus::Asked | WorkflowQuestionRecordStatus::Answered => existing
            .and_then(|question| question.reason_code.clone())
            .filter(|reason| !reason.trim().is_empty()),
    }
}

fn insert_optional_metadata(
    metadata: &mut BTreeMap<String, String>,
    key: &str,
    value: Option<&String>,
) {
    if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
        metadata.insert(key.to_string(), value.clone());
    }
}

fn closeout_readiness_for_report(
    run_id: &str,
    projection: &WorkflowProjection,
    persistent_tasks: &PersistentTaskProjection,
    signoff_policy: &WorkflowSignoffPolicy,
    closeout_policy: &WorkflowCloseoutPolicy,
    workflow_id: &str,
) -> WorkflowCloseoutReadiness {
    let mut readiness = projection.closeout_readiness(
        workflow_id.to_string(),
        persistent_tasks,
        signoff_policy,
        closeout_policy,
    );
    readiness.run_id = Some(run_id.to_string());
    readiness
}

fn closeout_policy(policy_id: Option<&str>) -> Result<WorkflowCloseoutPolicy, ToolError> {
    let policy_id = policy_id.unwrap_or(WORKFLOW_CLOSEOUT_DEFAULT_POLICY_ID);
    WorkflowCloseoutPolicy::from_config(
        policy_id.to_string(),
        WorkflowCloseoutPolicyConfig::default(),
    )
    .map_err(|err| {
        ToolError::Execution(format!(
            "invalid workflow closeout policy `{policy_id}`: {err:?}"
        ))
    })
}

fn validate_signoff_args(args: &WorkflowSignoffArgs) -> Result<(), ToolError> {
    if args.decision.requires_reason()
        && args
            .reason
            .as_deref()
            .is_none_or(|reason| reason.trim().is_empty())
    {
        return Err(ToolError::InvalidArguments(format!(
            "reason is required for workflow_signoff decision `{:?}`",
            args.decision
        )));
    }
    if args.decision.requires_scope()
        && args
            .scope
            .as_deref()
            .is_none_or(|scope| scope.trim().is_empty())
    {
        return Err(ToolError::InvalidArguments(format!(
            "scope is required for workflow_signoff decision `{:?}`",
            args.decision
        )));
    }
    Ok(())
}

fn workflow_operator_decision(
    decision: &WorkflowSignoffDecision,
    scope: Option<&str>,
) -> Result<String, ToolError> {
    match decision {
        WorkflowSignoffDecision::Approve => Ok("signoff-approved".to_string()),
        WorkflowSignoffDecision::Fail => Ok("signoff-failed".to_string()),
        WorkflowSignoffDecision::RequestEvidence => Ok("request-evidence".to_string()),
        WorkflowSignoffDecision::Waive => {
            let scope = scope.ok_or_else(|| {
                ToolError::InvalidArguments(
                    "scope is required for workflow_signoff decision `Waive`".to_string(),
                )
            })?;
            Ok(format!("waive:{scope}"))
        }
        WorkflowSignoffDecision::Abort => Ok("abort".to_string()),
        WorkflowSignoffDecision::Redirect => {
            let scope = scope.ok_or_else(|| {
                ToolError::InvalidArguments(
                    "scope is required for workflow_signoff decision `Redirect`".to_string(),
                )
            })?;
            Ok(format!("redirect:{scope}"))
        }
        WorkflowSignoffDecision::ApproveLive => Ok("approve-live".to_string()),
    }
}

fn terminal_outcome_for_decision(decision: &WorkflowSignoffDecision) -> Option<&'static str> {
    match decision {
        WorkflowSignoffDecision::Approve | WorkflowSignoffDecision::ApproveLive => {
            Some("outcome.finished")
        }
        WorkflowSignoffDecision::Fail => Some("outcome.failed"),
        WorkflowSignoffDecision::Abort => Some("outcome.cancelled"),
        WorkflowSignoffDecision::RequestEvidence
        | WorkflowSignoffDecision::Waive
        | WorkflowSignoffDecision::Redirect => None,
    }
}

fn render_dossier_markdown(dossier: &harness_core::run_dossier::RunDossier) -> String {
    let mut body = "# Workflow Run Dossier\n\n".to_string();
    for workflow in &dossier.workflows {
        body.push_str(&format!(
            "## `{}`\n\n- Status: `{}`\n- Owner: `{}`\n- Closeout policy: `{}` v{}\n- Closeout allowed: `{}`\n\n",
            workflow.workflow_id,
            workflow.status,
            workflow.owner,
            workflow.closeout.policy_id,
            workflow.closeout.policy_version,
            workflow.closeout.overall_allowed
        ));
    }
    body
}
