use std::collections::BTreeMap;

use async_trait::async_trait;
use harness_core::event::{
    TeamBounds, TeamMemberRole, TeamMemberSelector, TeamMemberSpec, TeamMessage, TeamMessageKind,
    TeamReference, TeamSpec, TeamTask, TeamTaskStatus,
};
use harness_core::proj::{TeamRunProjection, TeamRunStatus};
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::{parse_tool_args, text_json_tool_result};

const DEFAULT_TEAM_LIST_LIMIT: usize = 50;
const MAX_TEAM_LIST_LIMIT: usize = 200;

pub(crate) struct TeamCreateTool;
pub(crate) struct TeamListTool;
pub(crate) struct TeamStatusTool;
pub(crate) struct TeamSendMessageTool;
pub(crate) struct TeamTaskCreateTool;
pub(crate) struct TeamTaskListTool;
pub(crate) struct TeamTaskGetTool;
pub(crate) struct TeamTaskUpdateTool;
pub(crate) struct TeamShutdownRequestTool;
pub(crate) struct TeamShutdownApproveTool;
pub(crate) struct TeamShutdownRejectTool;
pub(crate) struct TeamDeleteTool;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TeamCreateArgs {
    #[serde(default, rename = "teamRunId", alias = "team_run_id")]
    team_run_id: Option<String>,
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    lead: Option<TeamMemberArgs>,
    members: Vec<TeamMemberArgs>,
    #[serde(default)]
    bounds: Option<TeamBoundsArgs>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TeamMemberArgs {
    name: String,
    #[serde(default)]
    role: TeamMemberRole,
    kind: TeamMemberKindArg,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    subagent_type: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum TeamMemberKindArg {
    Category,
    SubagentType,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TeamBoundsArgs {
    #[serde(default, rename = "maxMembers", alias = "max_members")]
    max_members: Option<u32>,
    #[serde(default, rename = "maxParallelMembers", alias = "max_parallel_members")]
    max_parallel_members: Option<u32>,
    #[serde(default, rename = "maxMessagesPerRun", alias = "max_messages_per_run")]
    max_messages_per_run: Option<u32>,
    #[serde(
        default,
        rename = "maxWallClockMinutes",
        alias = "max_wall_clock_minutes"
    )]
    max_wall_clock_minutes: Option<u32>,
    #[serde(default, rename = "maxMemberTurns", alias = "max_member_turns")]
    max_member_turns: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TeamStatusArgs {
    #[serde(default, rename = "teamRunId", alias = "team_run_id")]
    team_run_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TeamListArgs {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TeamSendMessageArgs {
    #[serde(rename = "teamRunId", alias = "team_run_id")]
    team_run_id: String,
    from: String,
    to: String,
    #[serde(default = "default_team_message_kind")]
    kind: TeamMessageKind,
    body: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    references: Vec<TeamReference>,
    #[serde(default, rename = "correlationId", alias = "correlation_id")]
    correlation_id: Option<String>,
    #[serde(default, rename = "messageId", alias = "message_id")]
    message_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TeamTaskCreateArgs {
    #[serde(rename = "teamRunId", alias = "team_run_id")]
    team_run_id: String,
    #[serde(default, rename = "taskId", alias = "task_id")]
    task_id: Option<String>,
    subject: String,
    description: String,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default, rename = "blockedBy", alias = "blocked_by")]
    blocked_by: Vec<String>,
    #[serde(default)]
    metadata: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TeamTaskSelectorArgs {
    #[serde(rename = "teamRunId", alias = "team_run_id")]
    team_run_id: String,
    #[serde(default)]
    status: Option<TeamTaskStatus>,
    #[serde(default)]
    owner: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TeamTaskGetArgs {
    #[serde(rename = "teamRunId", alias = "team_run_id")]
    team_run_id: String,
    #[serde(rename = "taskId", alias = "task_id")]
    task_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TeamTaskUpdateArgs {
    #[serde(rename = "teamRunId", alias = "team_run_id")]
    team_run_id: String,
    #[serde(rename = "taskId", alias = "task_id")]
    task_id: String,
    status: TeamTaskStatus,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    metadata: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TeamShutdownRequestArgs {
    #[serde(rename = "teamRunId", alias = "team_run_id")]
    team_run_id: String,
    #[serde(rename = "memberName", alias = "member_name")]
    member_name: String,
    requester: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TeamShutdownDecisionArgs {
    #[serde(rename = "teamRunId", alias = "team_run_id")]
    team_run_id: String,
    #[serde(rename = "memberName", alias = "member_name")]
    member_name: String,
    #[serde(rename = "actorName", alias = "actor_name")]
    actor_name: String,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TeamDeleteArgs {
    #[serde(rename = "teamRunId", alias = "team_run_id")]
    team_run_id: String,
}

fn default_team_message_kind() -> TeamMessageKind {
    TeamMessageKind::Message
}

impl TryFrom<TeamMemberArgs> for TeamMemberSpec {
    type Error = ToolError;

    fn try_from(value: TeamMemberArgs) -> Result<Self, Self::Error> {
        let selector = match value.kind {
            TeamMemberKindArg::Category => TeamMemberSelector::Category {
                category: value.category.ok_or_else(|| {
                    ToolError::InvalidArguments("category member requires category".to_string())
                })?,
            },
            TeamMemberKindArg::SubagentType => TeamMemberSelector::SubagentType {
                subagent_type: value.subagent_type.ok_or_else(|| {
                    ToolError::InvalidArguments(
                        "subagent_type member requires subagent_type".to_string(),
                    )
                })?,
            },
        };
        Ok(Self {
            name: value.name,
            role: value.role,
            selector,
            prompt: value.prompt,
        })
    }
}

fn bounds_from_args(args: Option<TeamBoundsArgs>) -> TeamBounds {
    let defaults = TeamBounds::default();
    let Some(args) = args else {
        return defaults;
    };
    TeamBounds {
        max_members: args.max_members.unwrap_or(defaults.max_members),
        max_parallel_members: args
            .max_parallel_members
            .unwrap_or(defaults.max_parallel_members),
        max_messages_per_run: args
            .max_messages_per_run
            .unwrap_or(defaults.max_messages_per_run),
        max_wall_clock_minutes: args
            .max_wall_clock_minutes
            .unwrap_or(defaults.max_wall_clock_minutes),
        max_member_turns: args.max_member_turns.unwrap_or(defaults.max_member_turns),
    }
}

fn map_coord_err(err: harness_core::coord::CoordinatorError) -> ToolError {
    ToolError::Execution(err.to_string())
}

fn team_result(label: &str, team: harness_core::proj::TeamRunProjection) -> ToolResult {
    text_json_tool_result(
        format!("{label}: {} ({})", team.name, team.team_run_id),
        serde_json::to_value(team).unwrap_or_else(|_| json!({ "error": "serialization failed" })),
    )
}

fn parse_team_run_status(status: &str) -> Result<TeamRunStatus, ToolError> {
    match status.trim().to_ascii_lowercase().as_str() {
        "active" => Ok(TeamRunStatus::Active),
        "shutdown_requested" | "shutdown-requested" => Ok(TeamRunStatus::ShutdownRequested),
        "deleted" => Ok(TeamRunStatus::Deleted),
        other => Err(ToolError::InvalidArguments(format!(
            "unsupported team status `{other}`; expected active, shutdown_requested, or deleted"
        ))),
    }
}

fn team_list_entry_json(team: &TeamRunProjection) -> serde_json::Value {
    json!({
        "team_run_id": team.team_run_id,
        "name": team.name,
        "description": team.description,
        "status": team.status,
        "lead": team.lead.as_ref().map(|lead| {
            json!({
                "status": lead.status,
                "profile": lead.profile,
                "agent_id": lead.agent_id,
                "selector": lead.selector,
            })
        }),
        "lead_summary": team.lead.as_ref().map(|lead| {
            let profile = lead.profile.as_deref().unwrap_or("<pending>");
            format!("{profile} ({:?})", lead.status)
        }),
        "member_count": team.members.len(),
        "member_counts": {
            "pending": team.members.values().filter(|member| matches!(member.status, harness_core::proj::TeamMemberStatus::Pending)).count(),
            "running": team.members.values().filter(|member| matches!(member.status, harness_core::proj::TeamMemberStatus::Running)).count(),
            "shutdown_requested": team.members.values().filter(|member| matches!(member.status, harness_core::proj::TeamMemberStatus::ShutdownRequested)).count(),
            "shutdown_approved": team.members.values().filter(|member| matches!(member.status, harness_core::proj::TeamMemberStatus::ShutdownApproved)).count(),
        },
        "task_count": team.tasks.len(),
        "task_counts": {
            "pending": team.tasks.values().filter(|task| matches!(task.status, TeamTaskStatus::Pending)).count(),
            "claimed": team.tasks.values().filter(|task| matches!(task.status, TeamTaskStatus::Claimed)).count(),
            "in_progress": team.tasks.values().filter(|task| matches!(task.status, TeamTaskStatus::InProgress)).count(),
            "completed": team.tasks.values().filter(|task| matches!(task.status, TeamTaskStatus::Completed)).count(),
            "deleted": team.tasks.values().filter(|task| matches!(task.status, TeamTaskStatus::Deleted)).count(),
        },
        "message_count": team.messages.len(),
        "bounds": team.bounds,
        "bounds_consumption": team.bounds_consumption,
        "created_mono_ms": team.created_mono_ms,
        "last_mono_ms": team.last_mono_ms,
        "shutdown_request_count": team.shutdown_requests.len(),
        "deleted": team.status == TeamRunStatus::Deleted,
        "shutdown_state": if team.status == TeamRunStatus::Deleted {
            "deleted"
        } else if team.status == TeamRunStatus::ShutdownRequested {
            "shutdown_requested"
        } else {
            "active"
        },
    })
}

#[async_trait]
impl Tool for TeamCreateTool {
    fn id(&self) -> &str {
        "team_create"
    }

    fn description(&self) -> &str {
        "Creates an event-sourced team run, resolves an optional write-capable lead, and activates members within coordinator-enforced bounds. Member role defaults to member; use role=research only for read-only profiles."
    }

    fn parameters_json_schema(&self) -> serde_json::Value {
        crate::json_schema_for::<TeamCreateArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }

    async fn call(
        &self,
        ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let args: TeamCreateArgs = parse_tool_args(args_json)?;
        let lead = args.lead.map(TeamMemberSpec::try_from).transpose()?;
        let members = args
            .members
            .into_iter()
            .map(TeamMemberSpec::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        let spec = TeamSpec {
            version: 1,
            name: args.name,
            description: args.description,
            lead: lead.map(|member| member.selector),
            members,
            bounds: bounds_from_args(args.bounds),
        };
        let team = ctx
            .coordinator
            .create_team(ctx.actor, spec, args.team_run_id)
            .await
            .map_err(map_coord_err)?;
        Ok(team_result("team created", team))
    }
}

#[async_trait]
impl Tool for TeamListTool {
    fn id(&self) -> &str {
        "team_list"
    }

    fn description(&self) -> &str {
        "Lists event-sourced team runs from the coordinator projection. This is read-only and does not create worktrees, tmux panes, mailboxes, file claims, or declared team registries."
    }

    fn parameters_json_schema(&self) -> serde_json::Value {
        crate::json_schema_for::<TeamListArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }

    async fn call(
        &self,
        ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let args: TeamListArgs = parse_tool_args(args_json)?;
        let status_filter = args
            .status
            .as_deref()
            .map(parse_team_run_status)
            .transpose()?;
        let requested_limit = args.limit.map(|limit| limit as usize);
        let raw_limit = requested_limit.unwrap_or(DEFAULT_TEAM_LIST_LIMIT);
        let limit = raw_limit.clamp(1, MAX_TEAM_LIST_LIMIT);
        let limit_clamped = raw_limit != limit;
        let projection = ctx
            .coordinator
            .team_projection()
            .await
            .map_err(map_coord_err)?;
        let total_count = projection.teams.len();
        let mut teams = projection
            .teams
            .values()
            .filter(|team| status_filter.is_none_or(|status| team.status == status))
            .map(team_list_entry_json)
            .collect::<Vec<_>>();
        teams.sort_by(|left, right| {
            left.get("team_run_id")
                .and_then(serde_json::Value::as_str)
                .cmp(&right.get("team_run_id").and_then(serde_json::Value::as_str))
        });
        let filtered_count = teams.len();
        let truncated_count = filtered_count.saturating_sub(limit);
        teams.truncate(limit);

        Ok(text_json_tool_result(
            format!("{} team run(s)", teams.len()),
            json!({
                "source": "event_replay",
                "scope": "primitive_projection_reader",
                "mutates": false,
                "excludes": [
                    "declared_team_registries",
                    "worktrees",
                    "tmux_visualization",
                    "mailbox_artifacts",
                    "team_file_claims",
                    "spawn_resume_shutdown_mutation"
                ],
                "status": args.status,
                "limit": limit,
                "requested_limit": requested_limit,
                "effective_limit": limit,
                "max_limit": MAX_TEAM_LIST_LIMIT,
                "limit_clamped": limit_clamped,
                "total_count": total_count,
                "filtered_count": filtered_count,
                "returned_count": teams.len(),
                "truncated_count": truncated_count,
                "truncated": truncated_count > 0,
                "teams": teams,
            }),
        ))
    }
}

#[async_trait]
impl Tool for TeamStatusTool {
    fn id(&self) -> &str {
        "team_status"
    }

    fn description(&self) -> &str {
        "Returns replay-derived team state for one team or all teams, including lead/member roles, runtime status, bounds consumption, tasks, messages, and shutdown state."
    }

    fn parameters_json_schema(&self) -> serde_json::Value {
        crate::json_schema_for::<TeamStatusArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }

    async fn call(
        &self,
        ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let args: TeamStatusArgs = parse_tool_args(args_json)?;
        let mut projection = ctx
            .coordinator
            .team_projection()
            .await
            .map_err(map_coord_err)?;
        if let Some(team_run_id) = args.team_run_id {
            let team = projection.teams.remove(&team_run_id).ok_or_else(|| {
                ToolError::InvalidArguments(format!("unknown team `{team_run_id}`"))
            })?;
            return Ok(team_result("team status", team));
        }
        Ok(text_json_tool_result(
            format!("{} team(s)", projection.teams.len()),
            serde_json::to_value(projection)
                .unwrap_or_else(|_| json!({ "error": "serialization failed" })),
        ))
    }
}

#[async_trait]
impl Tool for TeamSendMessageTool {
    fn id(&self) -> &str {
        "team_send_message"
    }

    fn description(&self) -> &str {
        "Appends a replayable team message after coordinator validation of role, shutdown status, duplicate message id, and runtime bounds."
    }

    fn parameters_json_schema(&self) -> serde_json::Value {
        crate::json_schema_for::<TeamSendMessageArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }

    async fn call(
        &self,
        ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let args: TeamSendMessageArgs = parse_tool_args(args_json)?;
        let team_run_id = args.team_run_id.clone();
        let message = TeamMessage {
            version: 1,
            message_id: args
                .message_id
                .unwrap_or_else(|| format!("msg_{}", ctx.tool_call_id)),
            from: args.from,
            to: args.to,
            kind: args.kind,
            body: args.body,
            summary: args.summary,
            references: args.references,
            correlation_id: args.correlation_id,
        };
        let team = ctx
            .coordinator
            .send_team_message(ctx.actor, team_run_id, message)
            .await
            .map_err(map_coord_err)?;
        Ok(team_result("team message sent", team))
    }
}

#[async_trait]
impl Tool for TeamTaskCreateTool {
    fn id(&self) -> &str {
        "team_task_create"
    }

    fn description(&self) -> &str {
        "Creates a replayable shared team task after coordinator validation of role, blockers, shutdown status, and runtime bounds. Provide blocked_by; blocks is projected."
    }

    fn parameters_json_schema(&self) -> serde_json::Value {
        crate::json_schema_for::<TeamTaskCreateArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }

    async fn call(
        &self,
        ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let args: TeamTaskCreateArgs = parse_tool_args(args_json)?;
        let task_id = args
            .task_id
            .unwrap_or_else(|| format!("teamtask_{}", ctx.tool_call_id));
        let task = TeamTask {
            version: 1,
            task_id,
            subject: args.subject,
            description: args.description,
            status: TeamTaskStatus::Pending,
            owner: args.owner,
            blocks: Vec::new(),
            blocked_by: args.blocked_by,
            metadata: args.metadata,
        };
        let team = ctx
            .coordinator
            .create_team_task(ctx.actor, args.team_run_id, task)
            .await
            .map_err(map_coord_err)?;
        Ok(team_result("team task created", team))
    }
}

#[async_trait]
impl Tool for TeamTaskListTool {
    fn id(&self) -> &str {
        "team_task_list"
    }

    fn description(&self) -> &str {
        "Lists replay-derived shared team tasks with optional status/owner filters."
    }

    fn parameters_json_schema(&self) -> serde_json::Value {
        crate::json_schema_for::<TeamTaskSelectorArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }

    async fn call(
        &self,
        ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let args: TeamTaskSelectorArgs = parse_tool_args(args_json)?;
        let projection = ctx
            .coordinator
            .team_projection()
            .await
            .map_err(map_coord_err)?;
        let team = projection.teams.get(&args.team_run_id).ok_or_else(|| {
            ToolError::InvalidArguments(format!("unknown team `{}`", args.team_run_id))
        })?;
        let tasks = team
            .tasks
            .values()
            .filter(|task| args.status.is_none_or(|status| task.status == status))
            .filter(|task| {
                args.owner
                    .as_deref()
                    .is_none_or(|owner| task.owner.as_deref() == Some(owner))
            })
            .cloned()
            .collect::<Vec<_>>();
        Ok(text_json_tool_result(
            format!("{} task(s)", tasks.len()),
            json!({ "team_run_id": args.team_run_id, "tasks": tasks }),
        ))
    }
}

#[async_trait]
impl Tool for TeamTaskGetTool {
    fn id(&self) -> &str {
        "team_task_get"
    }

    fn description(&self) -> &str {
        "Returns one replay-derived shared team task."
    }

    fn parameters_json_schema(&self) -> serde_json::Value {
        crate::json_schema_for::<TeamTaskGetArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }

    async fn call(
        &self,
        ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let args: TeamTaskGetArgs = parse_tool_args(args_json)?;
        let projection = ctx
            .coordinator
            .team_projection()
            .await
            .map_err(map_coord_err)?;
        let task = projection
            .teams
            .get(&args.team_run_id)
            .and_then(|team| team.tasks.get(&args.task_id))
            .ok_or_else(|| {
                ToolError::InvalidArguments(format!("unknown team task `{}`", args.task_id))
            })?;
        Ok(text_json_tool_result(
            format!("team task: {}", task.task_id),
            serde_json::to_value(task)
                .unwrap_or_else(|_| json!({ "error": "serialization failed" })),
        ))
    }
}

#[async_trait]
impl Tool for TeamTaskUpdateTool {
    fn id(&self) -> &str {
        "team_task_update"
    }

    fn description(&self) -> &str {
        "Updates a replayable shared team task status, owner, or metadata after coordinator validation of role, blockers, shutdown status, and runtime bounds."
    }

    fn parameters_json_schema(&self) -> serde_json::Value {
        crate::json_schema_for::<TeamTaskUpdateArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }

    async fn call(
        &self,
        ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let args: TeamTaskUpdateArgs = parse_tool_args(args_json)?;
        let team = ctx
            .coordinator
            .update_team_task(
                ctx.actor,
                args.team_run_id,
                args.task_id,
                args.status,
                args.owner,
                args.metadata,
            )
            .await
            .map_err(map_coord_err)?;
        Ok(team_result("team task updated", team))
    }
}

#[async_trait]
impl Tool for TeamShutdownRequestTool {
    fn id(&self) -> &str {
        "team_shutdown_request"
    }

    fn description(&self) -> &str {
        "Requests replayable shutdown approval for a team member; this lifecycle acknowledgement remains allowed for research members."
    }

    fn parameters_json_schema(&self) -> serde_json::Value {
        crate::json_schema_for::<TeamShutdownRequestArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }

    async fn call(
        &self,
        ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let args: TeamShutdownRequestArgs = parse_tool_args(args_json)?;
        let team = ctx
            .coordinator
            .request_team_shutdown(
                ctx.actor,
                args.team_run_id,
                args.member_name,
                args.requester,
            )
            .await
            .map_err(map_coord_err)?;
        Ok(team_result("team shutdown requested", team))
    }
}

#[async_trait]
impl Tool for TeamShutdownApproveTool {
    fn id(&self) -> &str {
        "team_shutdown_approve"
    }

    fn description(&self) -> &str {
        "Approves a replayable team member shutdown request."
    }

    fn parameters_json_schema(&self) -> serde_json::Value {
        crate::json_schema_for::<TeamShutdownDecisionArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }

    async fn call(
        &self,
        ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let args: TeamShutdownDecisionArgs = parse_tool_args(args_json)?;
        let team = ctx
            .coordinator
            .approve_team_shutdown(
                ctx.actor,
                args.team_run_id,
                args.member_name,
                args.actor_name,
            )
            .await
            .map_err(map_coord_err)?;
        Ok(team_result("team shutdown approved", team))
    }
}

#[async_trait]
impl Tool for TeamShutdownRejectTool {
    fn id(&self) -> &str {
        "team_shutdown_reject"
    }

    fn description(&self) -> &str {
        "Rejects a replayable team member shutdown request."
    }

    fn parameters_json_schema(&self) -> serde_json::Value {
        crate::json_schema_for::<TeamShutdownDecisionArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }

    async fn call(
        &self,
        ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let args: TeamShutdownDecisionArgs = parse_tool_args(args_json)?;
        let reason = args.reason.ok_or_else(|| {
            ToolError::InvalidArguments("shutdown rejection requires reason".to_string())
        })?;
        let team = ctx
            .coordinator
            .reject_team_shutdown(
                ctx.actor,
                args.team_run_id,
                args.member_name,
                args.actor_name,
                reason,
            )
            .await
            .map_err(map_coord_err)?;
        Ok(team_result("team shutdown rejected", team))
    }
}

#[async_trait]
impl Tool for TeamDeleteTool {
    fn id(&self) -> &str {
        "team_delete"
    }

    fn description(&self) -> &str {
        "Deletes a team after all non-lead members have approved shutdown; deletion does not cancel unrelated provider or tool tasks."
    }

    fn parameters_json_schema(&self) -> serde_json::Value {
        crate::json_schema_for::<TeamDeleteArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }

    async fn call(
        &self,
        ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let args: TeamDeleteArgs = parse_tool_args(args_json)?;
        let team = ctx
            .coordinator
            .delete_team(ctx.actor, args.team_run_id)
            .await
            .map_err(map_coord_err)?;
        Ok(team_result("team deleted", team))
    }
}
