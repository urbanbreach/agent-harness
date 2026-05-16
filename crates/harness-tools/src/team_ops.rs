use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use harness_core::event::{
    TeamBounds, TeamMemberRole, TeamMemberSelector, TeamMemberSpec, TeamMessage, TeamMessageKind,
    TeamReference, TeamSpec, TeamTask, TeamTaskStatus,
};
use harness_core::proj::{
    TEAM_METADATA_ABORT_REASON, TEAM_METADATA_EVIDENCE_REF, TEAM_METADATA_SYNTHESIS_REF,
};
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{parse_tool_args, text_json_tool_result};

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
    #[serde(default)]
    metadata: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TeamListArgs {
    #[serde(default)]
    include_deleted: bool,
    #[serde(default = "default_include_declared_teams")]
    include_declared: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DeclaredTeamSpec {
    version: u16,
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    lead: Option<TeamMemberSelector>,
    members: Vec<TeamMemberSpec>,
    #[serde(default)]
    bounds: TeamBounds,
    #[serde(default)]
    metadata: BTreeMap<String, String>,
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
    #[serde(default, rename = "abortReason", alias = "abort_reason")]
    abort_reason: Option<String>,
    #[serde(default)]
    metadata: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TeamDeleteArgs {
    #[serde(rename = "teamRunId", alias = "team_run_id")]
    team_run_id: String,
    #[serde(default, rename = "abortReason", alias = "abort_reason")]
    abort_reason: Option<String>,
    #[serde(default, rename = "evidenceRefs", alias = "evidence_refs")]
    evidence_refs: Vec<String>,
    #[serde(default, rename = "synthesisRefs", alias = "synthesis_refs")]
    synthesis_refs: Vec<String>,
    #[serde(default)]
    metadata: BTreeMap<String, String>,
}

fn default_team_message_kind() -> TeamMessageKind {
    TeamMessageKind::Message
}

fn default_include_declared_teams() -> bool {
    true
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

fn insert_optional_metadata(
    metadata: &mut BTreeMap<String, String>,
    key: &str,
    value: Option<String>,
) {
    if let Some(value) = value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    }) {
        metadata.entry(key.to_string()).or_insert(value);
    }
}

fn insert_joined_metadata(metadata: &mut BTreeMap<String, String>, key: &str, values: Vec<String>) {
    let refs = values
        .into_iter()
        .filter_map(|value| {
            let trimmed = value.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        })
        .collect::<Vec<_>>();
    if !refs.is_empty() {
        metadata.entry(key.to_string()).or_insert(refs.join(","));
    }
}

fn team_result(label: &str, team: harness_core::proj::TeamRunProjection) -> ToolResult {
    text_json_tool_result(
        format!("{label}: {} ({})", team.name, team.team_run_id),
        serde_json::to_value(team).unwrap_or_else(|_| json!({ "error": "serialization failed" })),
    )
}

fn declared_team_roots(workspace_root: &Path) -> Vec<PathBuf> {
    let mut roots = vec![workspace_root.join(".agent-harness/teams")];
    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        roots.push(PathBuf::from(config_home).join("harness/teams"));
    } else if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join(".config/harness/teams"));
    }
    roots
}

fn load_declared_teams(workspace_root: &Path) -> Vec<serde_json::Value> {
    let mut declared = Vec::new();
    for root in declared_team_roots(workspace_root) {
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            declared.push(load_declared_team_file(&root, &path));
        }
    }
    declared.sort_by(|left, right| {
        left.get("name")
            .and_then(serde_json::Value::as_str)
            .cmp(&right.get("name").and_then(serde_json::Value::as_str))
    });
    declared
}

fn load_declared_team_file(root: &Path, path: &Path) -> serde_json::Value {
    let source = path.display().to_string();
    let root_display = root.display().to_string();
    let name_from_path = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string();
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            return json!({
                "name": name_from_path,
                "source": source,
                "root": root_display,
                "status": "invalid",
                "errors": [format!("failed to read declared team spec: {err}")],
            });
        }
    };
    match serde_json::from_str::<DeclaredTeamSpec>(&text) {
        Ok(spec) => declared_team_json(spec, &name_from_path, &source, &root_display),
        Err(err) => json!({
            "name": name_from_path,
            "source": source,
            "root": root_display,
            "status": "invalid",
            "errors": [format!("failed to parse declared team spec: {err}")],
        }),
    }
}

fn declared_team_json(
    spec: DeclaredTeamSpec,
    name_from_path: &str,
    source: &str,
    root: &str,
) -> serde_json::Value {
    let errors = validate_declared_team_spec(&spec, name_from_path);
    let metadata = spec.metadata;
    let worktree_path = metadata
        .get("worktree.path")
        .or_else(|| metadata.get("worktree_path"))
        .cloned();
    let worktree_status = metadata
        .get("worktree.status")
        .or_else(|| metadata.get("worktree_status"))
        .cloned();
    let tmux_pane = metadata
        .get("tmux.pane")
        .or_else(|| metadata.get("tmux_pane"))
        .cloned();
    let tmux_status = metadata
        .get("tmux.status")
        .or_else(|| metadata.get("tmux_status"))
        .cloned();
    json!({
        "name": spec.name,
        "description": spec.description,
        "source": source,
        "root": root,
        "status": if errors.is_empty() { "valid" } else { "invalid" },
        "errors": errors,
        "lead": spec.lead,
        "members": spec.members,
        "member_count": spec.members.len(),
        "bounds": spec.bounds,
        "metadata": metadata,
        "runtime": {
            "worktree_path": worktree_path,
            "worktree_status": worktree_status,
            "tmux_pane": tmux_pane,
            "tmux_status": tmux_status
        }
    })
}

fn validate_declared_team_spec(spec: &DeclaredTeamSpec, name_from_path: &str) -> Vec<String> {
    let mut errors = Vec::new();
    if spec.version != 1 {
        errors.push(format!("unsupported version {}; expected 1", spec.version));
    }
    if spec.name.trim().is_empty() {
        errors.push("name cannot be empty".to_string());
    }
    if !name_from_path.is_empty() && spec.name != name_from_path {
        errors.push(format!(
            "file name `{name_from_path}` must match declared team name `{}`",
            spec.name
        ));
    }
    if spec.members.is_empty() {
        errors.push("members cannot be empty".to_string());
    }
    if spec.members.len() as u32 > spec.bounds.max_members {
        errors.push(format!(
            "member count {} exceeds max_members {}",
            spec.members.len(),
            spec.bounds.max_members
        ));
    }
    if let Some(lead) = spec.lead.as_ref() {
        let lead_profile = selector_profile_name(lead);
        if is_read_only_team_profile(lead_profile) {
            errors.push(format!(
                "lead selector `{lead_profile}` is read-only or planning-only"
            ));
        }
    }
    for member in &spec.members {
        let profile = selector_profile_name(&member.selector);
        let read_only = is_read_only_team_profile(profile);
        match member.role {
            TeamMemberRole::Member if read_only => errors.push(format!(
                "member `{}` uses read-only selector `{profile}`; mark role as research",
                member.name
            )),
            TeamMemberRole::Research if !read_only => errors.push(format!(
                "research member `{}` uses write-capable selector `{profile}`",
                member.name
            )),
            _ => {}
        }
    }
    errors
}

fn selector_profile_name(selector: &TeamMemberSelector) -> &str {
    match selector {
        TeamMemberSelector::Category { category } => category,
        TeamMemberSelector::SubagentType { subagent_type } => subagent_type,
    }
}

fn is_read_only_team_profile(profile: &str) -> bool {
    matches!(
        profile,
        "oracle"
            | "librarian"
            | "explore"
            | "metis"
            | "momus"
            | "multimodal-looker"
            | "prometheus"
            | "plan"
    )
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
            metadata: args.metadata,
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
        "Lists replay-derived active team runs and declared team specs from project/user Harness team roots. Worktrees, file claims, and tmux visualization are reported as not-started parity seams."
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
        let projection = ctx
            .coordinator
            .team_projection()
            .await
            .map_err(map_coord_err)?;
        let teams = projection
            .teams
            .values()
            .filter(|team| {
                args.include_deleted || team.status != harness_core::proj::TeamRunStatus::Deleted
            })
            .map(|team| {
                json!({
                    "team_run_id": &team.team_run_id,
                    "name": &team.name,
                    "status": &team.status,
                    "members": team.members.len(),
                    "tasks": team.tasks.len(),
                    "task_status_counts": &team.task_status_counts,
                    "shutdown_proof": &team.shutdown_proof,
                    "messages": team.messages.len(),
                    "shutdown_requests": team.shutdown_requests.len(),
                    "workflow_id": &team.workflow_id,
                })
            })
            .collect::<Vec<_>>();
        let declared_teams = if args.include_declared {
            load_declared_teams(&ctx.workspace_root)
        } else {
            Vec::new()
        };
        let invalid_declared = declared_teams
            .iter()
            .filter(|team| {
                team.get("status").and_then(serde_json::Value::as_str) == Some("invalid")
            })
            .count();
        Ok(text_json_tool_result(
            format!(
                "{} active team(s), {} declared team spec(s)",
                teams.len(),
                declared_teams.len()
            ),
            json!({
                "teams": teams,
                "declared_teams": declared_teams,
                "declared_team_roots": declared_team_roots(&ctx.workspace_root)
                    .into_iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>(),
                "declared_team_validation": {
                    "invalid": invalid_declared,
                    "status": if invalid_declared == 0 { "ok" } else { "invalid" }
                },
                "policy": "completion requires shutdown approval plus no pending/claimed/in-progress tasks and verification evidence, unless abort_reason metadata is present",
                "source": "event_replay"
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
        let mut metadata = args.metadata;
        insert_optional_metadata(
            &mut metadata,
            TEAM_METADATA_ABORT_REASON,
            args.abort_reason.or(args.reason),
        );
        let team = ctx
            .coordinator
            .approve_team_shutdown_with_metadata(
                ctx.actor,
                args.team_run_id,
                args.member_name,
                args.actor_name,
                metadata,
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
        let mut metadata = args.metadata;
        insert_optional_metadata(&mut metadata, TEAM_METADATA_ABORT_REASON, args.abort_reason);
        insert_joined_metadata(
            &mut metadata,
            TEAM_METADATA_EVIDENCE_REF,
            args.evidence_refs,
        );
        insert_joined_metadata(
            &mut metadata,
            TEAM_METADATA_SYNTHESIS_REF,
            args.synthesis_refs,
        );
        let team = ctx
            .coordinator
            .delete_team_with_metadata(ctx.actor, args.team_run_id, metadata)
            .await
            .map_err(map_coord_err)?;
        Ok(team_result("team deleted", team))
    }
}
