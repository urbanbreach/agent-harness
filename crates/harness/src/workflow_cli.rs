use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{ArgGroup, Args, Subcommand, ValueEnum};
use harness_core::agent_catalog::{resolve_agent_catalog, AgentCatalog};
use harness_core::clock::{Clock, RealClock};
use harness_core::config::{load_resolved_config, ShellAllowlist, WorkflowRuntimeConfig};
use harness_core::context_snapshot::{
    ContextSnapshotAmbiguity, ContextSnapshotInput, ContextSnapshotOptions,
    ContextSnapshotWriteResult,
};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle, RunInfo};
use harness_core::event::{ActorKind, EventActor};
use harness_core::goal_ledger::{
    goal_checkpoint_artifact_name, goal_checkpoint_metadata, goal_ledger_artifact_name,
    goal_ledger_metadata, project_goal_ledger, validate_goal_checkpoint_artifact,
    validate_goal_ledger_artifact, GoalCheckpointArtifact, GoalLedgerArtifact,
    GoalLedgerProjection, GoalQualityGate, GoalStoryArtifact, GOAL_LEDGER_EVIDENCE_CATEGORY,
    GOAL_LEDGER_MODE, GOAL_LEDGER_SCHEMA_VERSION,
};
use harness_core::perm::{PermissionKind, PermissionPolicy, PermissionRuleRequest, PolicyDecision};
use harness_core::persistent_task::{project_persistent_tasks, PersistentTaskProjection};
use harness_core::plan_consensus::{
    plan_consensus_artifact_name, plan_consensus_metadata, resolve_plan_consensus_lanes,
    validate_plan_consensus_artifact, PlanConsensusArtifact, PlanConsensusLane,
    PlanConsensusOption, PLAN_CONSENSUS_EVIDENCE_CATEGORY, PLAN_CONSENSUS_MODE,
    PLAN_CONSENSUS_SCHEMA_VERSION,
};
use harness_core::proj::SessionModeSource;
use harness_core::redact::DefaultRedactor;
use harness_core::research_mission::{
    project_research_missions, research_mission_artifact_name, research_mission_metadata,
    research_result_artifact_name, research_result_metadata, research_validator_artifact_name,
    validate_research_mission_artifact, validate_research_result_artifact, ResearchMissionArtifact,
    ResearchMissionProjection, ResearchResultArtifact, ResearchSandboxArtifact,
    ResearchValidatorArtifact, ResearchValidatorCommand, ResearchValidatorMode,
    RESEARCH_MISSION_EVIDENCE_CATEGORY, RESEARCH_MISSION_MODE, RESEARCH_MISSION_SCHEMA_VERSION,
};
use harness_core::run_dossier::{build_run_dossier_with_tasks_and_closeout_policy, RunDossier};
use harness_core::tool::ArtifactStore;
use harness_core::wiki::{
    parse_wiki_page, render_wiki_page, wiki_digest, wiki_evidence_metadata, wiki_lint,
    wiki_matches, wiki_page_path, wiki_summary, WikiLintFinding, WikiPage, WikiPageSummary,
    WIKI_EVIDENCE_CATEGORY, WIKI_MODE,
};
use harness_core::workflow::{
    project_workflows, WorkflowEvidenceRequest, WorkflowProjection, WorkflowSignoffPolicy,
    WorkflowStartRequest, WorkflowStartResult,
};
use harness_core::workflow_closeout::{
    WorkflowCloseoutPolicy, WorkflowCloseoutReadiness, WorkflowSignoffDecision,
    WorkflowSignoffReport, WorkflowStatusCloseoutReport,
};
use harness_core::workflow_registry::{evidence_category_ids, is_evidence_category};
use serde::Serialize;
use serde_json::json;

use crate::cli_io::{load_events_from_run_dir, EVENTS_FILE_NAME};
use crate::defaults::DEFAULT_SESSION_DIR;

const PERMISSION_DECISION_EVIDENCE_CATEGORY: &str = "evidence.permission_decision";
const PERMISSION_DENIED_STATUS: &str = "denied";

#[derive(Debug, Args, Clone)]
pub struct WorkflowCommand {
    #[command(subcommand)]
    command: WorkflowCommands,
}

#[derive(Debug, Subcommand, Clone)]
enum WorkflowCommands {
    /// Start a coordinator-owned workflow run.
    Run(WorkflowRunCommand),
    /// Inspect replay-derived workflow status without appending events.
    Status(WorkflowStatusCommand),
    /// Record an operator signoff decision against a target workflow.
    Signoff(WorkflowSignoffCommand),
    /// Record a workflow cancellation outcome in a coordinator-owned audit run.
    Cancel(WorkflowCancelCommand),
    /// Export a replay-derived Run Dossier.
    Dossier(WorkflowDossierCommand),
    /// Capture or inspect workflow context snapshots.
    Snapshot(WorkflowSnapshotCommand),
    /// Create a reviewed planner/architect/critic consensus plan artifact.
    #[command(name = "plan-consensus", alias = "ralplan", alias = "consensus-plan")]
    PlanConsensus(Box<WorkflowPlanConsensusCommand>),
    /// Create, checkpoint, or inspect replay-derived workflow goal ledgers.
    #[command(name = "goal", alias = "goal-ledger", alias = "ultragoal")]
    Goal(Box<WorkflowGoalCommand>),
    /// Create or inspect validator-gated research missions.
    #[command(name = "mission", alias = "research-loop", alias = "autoresearch")]
    Mission(Box<WorkflowMissionCommand>),
    /// Read, query, or update the markdown workflow wiki.
    Wiki(Box<WorkflowWikiCommand>),
    /// Record generic workflow-family evidence with status metadata.
    Evidence(Box<WorkflowEvidenceCommand>),
    /// Check or apply local workflow bootstrap files.
    Init(WorkflowInitCommand),
}

#[derive(Debug, Args, Clone)]
struct WorkflowRunCommand {
    /// Stable workflow id. Defaults to a value derived from the harness run id.
    #[arg(long)]
    workflow_id: Option<String>,

    /// Human-readable workflow title.
    #[arg(long)]
    title: Option<String>,

    /// Workflow owner recorded on lifecycle events.
    #[arg(long, default_value = "workflow-cli")]
    owner: String,

    /// Workflow lane. Defaults to runtime.workflow.run.default_lane.
    #[arg(long)]
    lane: Option<String>,

    /// Optional idempotency key for duplicate start detection.
    #[arg(long)]
    idempotency_key: Option<String>,

    /// Emit machine-readable JSON.
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct WorkflowStatusCommand {
    #[command(flatten)]
    target: WorkflowReadTargetArgs,

    /// Limit the status report to one workflow id.
    #[arg(long)]
    workflow_id: Option<String>,

    /// Emit machine-readable JSON.
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
#[command(group(
    ArgGroup::new("decision")
        .required(true)
        .args(["approve", "fail", "request_evidence", "waive", "abort", "redirect", "approve_live"])
))]
struct WorkflowSignoffCommand {
    #[command(flatten)]
    target: WorkflowReadTargetArgs,

    /// Workflow id receiving the operator decision.
    #[arg(long)]
    workflow_id: String,

    /// Record signoff approval and terminal success.
    #[arg(long, default_value_t = false)]
    approve: bool,

    /// Record signoff failure and terminal failure.
    #[arg(long, default_value_t = false)]
    fail: bool,

    /// Request more evidence without terminalizing the workflow.
    #[arg(long = "request-evidence", default_value_t = false)]
    request_evidence: bool,

    /// Waive a closeout blocker scope without terminalizing the workflow.
    #[arg(long, default_value_t = false)]
    waive: bool,

    /// Abort and terminally cancel the workflow.
    #[arg(long, default_value_t = false)]
    abort: bool,

    /// Record a redirect decision for a closeout scope without terminalizing the workflow.
    #[arg(long, default_value_t = false)]
    redirect: bool,

    /// Approve terminal success through an explicit live-approval closeout policy.
    #[arg(long = "approve-live", default_value_t = false)]
    approve_live: bool,

    /// Closeout dimension/category/domain scope for waive or redirect decisions.
    #[arg(long)]
    scope: Option<String>,

    /// Closeout policy id. Defaults to runtime.workflow.closeout.default_policy.
    #[arg(long)]
    policy_id: Option<String>,

    /// Operator id recorded with the decision.
    #[arg(long, default_value = "operator")]
    operator: String,

    /// Decision reason.
    #[arg(long)]
    reason: Option<String>,

    /// Use the legacy detached audit-run behavior instead of target workflow closeout.
    #[arg(long, default_value_t = false)]
    audit_only: bool,

    /// Emit machine-readable JSON.
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct WorkflowCancelCommand {
    /// Workflow id to cancel.
    #[arg(long)]
    workflow_id: String,

    /// Owner recorded on the terminal cancellation event.
    #[arg(long, default_value = "workflow-cli")]
    owner: String,

    /// Cancellation reason.
    #[arg(long, default_value = "operator cancelled workflow")]
    reason: String,

    /// Compatibility selector for future multi-workflow cancellation surfaces.
    #[arg(long, default_value = "workflow")]
    mode: String,

    /// Emit machine-readable JSON.
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct WorkflowDossierCommand {
    #[command(subcommand)]
    command: WorkflowDossierCommands,
}

#[derive(Debug, Subcommand, Clone)]
enum WorkflowDossierCommands {
    /// Export a replay-derived dossier from a run directory.
    Export(WorkflowDossierExportCommand),
}

#[derive(Debug, Args, Clone)]
struct WorkflowDossierExportCommand {
    #[command(flatten)]
    target: WorkflowReadTargetArgs,

    /// Limit the dossier to one workflow id.
    #[arg(long)]
    workflow_id: Option<String>,

    /// Dossier output format.
    #[arg(long, value_enum, default_value_t = DossierFormat::Json)]
    format: DossierFormat,

    /// Optional output file. Omitted means print to stdout only.
    #[arg(long)]
    output: Option<PathBuf>,

    /// Emit machine-readable export metadata.
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum DossierFormat {
    Json,
    Markdown,
}

#[derive(Debug, Args, Clone)]
struct WorkflowSnapshotCommand {
    #[command(subcommand)]
    command: WorkflowSnapshotCommands,
}

#[derive(Debug, Subcommand, Clone)]
enum WorkflowSnapshotCommands {
    Write(WorkflowSnapshotWriteCommand),
    List(WorkflowSnapshotListCommand),
    Read(WorkflowSnapshotReadCommand),
    Export(WorkflowSnapshotReadCommand),
}

#[derive(Debug, Args, Clone)]
struct WorkflowSnapshotListCommand {
    #[command(flatten)]
    target: WorkflowReadTargetArgs,

    /// Emit machine-readable JSON.
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct WorkflowSnapshotReadCommand {
    #[command(flatten)]
    target: WorkflowReadTargetArgs,

    /// Snapshot id to read from projection metadata.
    #[arg(long)]
    snapshot_id: String,

    /// Optional output file for export.
    #[arg(long)]
    output: Option<PathBuf>,

    /// Emit machine-readable JSON.
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct WorkflowSnapshotWriteCommand {
    /// Workflow id to attach the snapshot evidence to.
    #[arg(long)]
    workflow_id: Option<String>,

    /// Source command that initiated intake, such as `/interview` or `/workflow run`.
    #[arg(long, default_value = "/workflow run")]
    source_command: String,

    /// Task or problem statement to capture in the snapshot.
    #[arg(long)]
    task: String,

    /// Desired outcome for the workflow or interview handoff.
    #[arg(long, default_value = "")]
    desired_outcome: String,

    /// Inferred or operator-provided intent summary.
    #[arg(long, default_value = "")]
    probable_intent: String,

    /// Constraint to persist in the snapshot; repeat for multiple constraints.
    #[arg(long = "constraint")]
    constraints: Vec<String>,

    /// Known unknown to persist in the snapshot; repeat for multiple unknowns.
    #[arg(long = "unknown")]
    unknowns: Vec<String>,

    /// Likely file/module/system touchpoint; repeat for multiple touchpoints.
    #[arg(long = "touchpoint")]
    likely_touchpoints: Vec<String>,

    /// Ambiguity score to expose in workflow status projections.
    #[arg(long, default_value_t = 0.0)]
    ambiguity_score: f32,

    /// Ambiguity threshold used by intake gating.
    #[arg(long, default_value_t = 0.2)]
    ambiguity_threshold: f32,

    /// Mark the snapshot as ready for downstream workflow handoff.
    #[arg(long, default_value_t = false)]
    handoff_ready: bool,

    /// Maximum characters stored per free-text field before capping.
    #[arg(long, default_value_t = 4_000)]
    max_text_chars: usize,

    /// Maximum items stored per list field before capping.
    #[arg(long, default_value_t = 64)]
    max_list_items: usize,

    /// Emit machine-readable JSON containing run and artifact references.
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct WorkflowPlanConsensusCommand {
    /// Stable workflow id for the planning workflow.
    #[arg(long)]
    workflow_id: Option<String>,

    /// Stable plan id. Defaults to a value derived from the run id.
    #[arg(long)]
    plan_id: Option<String>,

    /// Task or decision being planned.
    #[arg(long)]
    task: String,

    /// Optional context snapshot id/ref used as planning input.
    #[arg(long)]
    snapshot_ref: Option<String>,

    /// Workflow owner recorded on lifecycle events.
    #[arg(long, default_value = "workflow-cli")]
    owner: String,

    /// Workflow lane. Defaults to runtime.workflow.run.default_lane.
    #[arg(long)]
    lane: Option<String>,

    /// Planning principle. Repeat for multiple principles.
    #[arg(long = "principle")]
    principles: Vec<String>,

    /// Decision driver. Repeat for multiple drivers.
    #[arg(long = "decision-driver")]
    decision_drivers: Vec<String>,

    /// Viable option as id=summary. Repeat for multiple options.
    #[arg(long = "option")]
    options: Vec<String>,

    /// Chosen option id. Defaults to the first --option id.
    #[arg(long)]
    chosen_option: Option<String>,

    /// Rejected alternative. Repeat for multiple alternatives.
    #[arg(long = "reject")]
    rejected_alternatives: Vec<String>,

    /// Architecture decision record text.
    #[arg(long)]
    adr: String,

    /// Work-breakdown item. Repeat for multiple items.
    #[arg(long = "work")]
    work_breakdown: Vec<String>,

    /// Risk or pre-mortem item. Repeat for multiple risks.
    #[arg(long = "risk")]
    risks: Vec<String>,

    /// Test-plan item. Repeat for multiple checks.
    #[arg(long = "test-plan")]
    test_plan: Vec<String>,

    /// Manual QA-plan item. Repeat for multiple checks.
    #[arg(long = "manual-qa")]
    manual_qa_plan: Vec<String>,

    /// Agent/team staffing guidance. Repeat for multiple items.
    #[arg(long = "staffing")]
    staffing: Vec<String>,

    /// Execution handoff option. Repeat for multiple handoffs.
    #[arg(long = "handoff")]
    handoff_options: Vec<String>,

    /// Acceptance criterion. Repeat for multiple criteria.
    #[arg(long = "acceptance")]
    acceptance_criteria: Vec<String>,

    /// Evidence ref used to justify the plan. Repeat for multiple refs.
    #[arg(long = "evidence-ref")]
    evidence_refs: Vec<String>,

    /// Final critic verdict.
    #[arg(long, default_value = "approved")]
    critic_verdict: String,

    /// Number of critic iterations that occurred.
    #[arg(long, default_value_t = 1)]
    critic_iterations: u32,

    /// Override runtime.workflow.planConsensus.maxIterations.
    #[arg(long)]
    max_iterations: Option<u32>,

    /// Emit machine-readable JSON.
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct WorkflowGoalCommand {
    #[command(subcommand)]
    command: WorkflowGoalCommands,
}

#[derive(Debug, Subcommand, Clone)]
enum WorkflowGoalCommands {
    /// Create a goal ledger artifact and workflow evidence.
    Create(WorkflowGoalCreateCommand),
    /// Checkpoint a story with evidence refs and optional final quality gate.
    Checkpoint(WorkflowGoalCheckpointCommand),
    /// Inspect replay-derived goal status.
    Status(WorkflowGoalStatusCommand),
    /// List replay-derived goals.
    List(WorkflowGoalStatusCommand),
    /// Read one replay-derived goal.
    Read(WorkflowGoalReadCommand),
}

#[derive(Debug, Args, Clone)]
struct WorkflowGoalCreateCommand {
    /// Stable workflow id for the goal ledger workflow.
    #[arg(long)]
    workflow_id: Option<String>,

    /// Stable goal id.
    #[arg(long)]
    goal_id: String,

    /// Aggregate goal objective.
    #[arg(long)]
    objective: String,

    /// Story definition as id=objective. Repeat for multiple stories.
    #[arg(long = "story")]
    stories: Vec<String>,

    /// Acceptance criterion applied to each story. Repeat for multiple criteria.
    #[arg(long = "acceptance")]
    acceptance: Vec<String>,

    /// Initial evidence ref for the goal ledger. Repeat for multiple refs.
    #[arg(long = "evidence-ref")]
    evidence_refs: Vec<String>,

    /// Optional owner workflow id recorded on each story.
    #[arg(long)]
    owner_workflow_id: Option<String>,

    /// Workflow owner recorded on lifecycle events.
    #[arg(long, default_value = "workflow-cli")]
    owner: String,

    /// Workflow lane. Defaults to runtime.workflow.run.default_lane.
    #[arg(long)]
    lane: Option<String>,

    /// Emit machine-readable JSON.
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct WorkflowGoalCheckpointCommand {
    /// Stable workflow id for the goal ledger workflow.
    #[arg(long)]
    workflow_id: Option<String>,

    /// Goal id receiving the checkpoint.
    #[arg(long)]
    goal_id: String,

    /// Story id receiving the checkpoint.
    #[arg(long)]
    story_id: Option<String>,

    /// New story/checkpoint status.
    #[arg(long, value_enum)]
    status: GoalStatusArg,

    /// Checkpoint summary.
    #[arg(long, default_value = "goal checkpoint")]
    summary: String,

    /// Evidence ref proving this checkpoint. Repeat for multiple refs.
    #[arg(long = "evidence-ref")]
    evidence_refs: Vec<String>,

    /// Marks this checkpoint as the aggregate final completion checkpoint.
    #[arg(long, default_value_t = false)]
    final_goal: bool,

    /// Verification evidence ref for the final quality gate.
    #[arg(long = "verification-ref")]
    verification_refs: Vec<String>,

    /// Review evidence ref for the final quality gate.
    #[arg(long = "review-ref")]
    review_refs: Vec<String>,

    /// Cleanup evidence ref for the final quality gate.
    #[arg(long = "cleanup-ref")]
    cleanup_refs: Vec<String>,

    /// Additional quality-gate evidence ref.
    #[arg(long = "quality-gate-ref")]
    quality_gate_refs: Vec<String>,

    /// Workflow owner recorded on lifecycle events.
    #[arg(long, default_value = "workflow-cli")]
    owner: String,

    /// Workflow lane. Defaults to runtime.workflow.run.default_lane.
    #[arg(long)]
    lane: Option<String>,

    /// Emit machine-readable JSON.
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct WorkflowGoalStatusCommand {
    #[command(flatten)]
    target: WorkflowReadTargetArgs,

    /// Limit the status report to one goal id.
    #[arg(long)]
    goal_id: Option<String>,

    /// Emit machine-readable JSON.
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct WorkflowGoalReadCommand {
    #[command(flatten)]
    target: WorkflowReadTargetArgs,

    /// Goal id to read from the replay-derived projection.
    #[arg(long)]
    goal_id: String,

    /// Optional output file for export.
    #[arg(long)]
    output: Option<PathBuf>,

    /// Emit machine-readable JSON.
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum GoalStatusArg {
    Pending,
    Active,
    Complete,
    Blocked,
    Failed,
}

impl GoalStatusArg {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Complete => "complete",
            Self::Blocked => "blocked",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Args, Clone)]
struct WorkflowMissionCommand {
    #[command(subcommand)]
    command: WorkflowMissionCommands,
}

#[derive(Debug, Subcommand, Clone)]
enum WorkflowMissionCommands {
    /// Create mission and sandbox artifacts.
    Init(WorkflowMissionInitCommand),
    /// Record an iteration result and validator/review artifact refs.
    Run(WorkflowMissionRunCommand),
    /// Inspect replay-derived mission status.
    Status(WorkflowMissionStatusCommand),
    /// Read one replay-derived mission.
    Read(WorkflowMissionReadCommand),
}

#[derive(Debug, Args, Clone)]
struct WorkflowMissionInitCommand {
    #[arg(long)]
    workflow_id: Option<String>,
    #[arg(long)]
    mission_id: String,
    #[arg(long)]
    objective: String,
    #[arg(long)]
    question: String,
    #[arg(long, value_enum, default_value_t = ValidatorModeArg::PromptArchitectArtifact)]
    validator_mode: ValidatorModeArg,
    #[arg(long, default_value = "isolated research sandbox")]
    sandbox: String,
    #[arg(long = "allowed-command")]
    allowed_commands: Vec<String>,
    #[arg(long = "constraint")]
    constraints: Vec<String>,
    #[arg(long = "evidence-ref")]
    evidence_refs: Vec<String>,
    #[arg(long, default_value = "workflow-cli")]
    owner: String,
    #[arg(long)]
    lane: Option<String>,
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct WorkflowMissionRunCommand {
    #[arg(long)]
    workflow_id: Option<String>,
    #[arg(long)]
    mission_id: String,
    #[arg(long, default_value_t = 1)]
    iteration: u32,
    #[arg(long, value_enum)]
    status: MissionStatusArg,
    #[arg(long)]
    summary: String,
    #[arg(long)]
    candidate_ref: Option<String>,
    #[arg(long, value_enum, default_value_t = ValidatorModeArg::PromptArchitectArtifact)]
    validator_mode: ValidatorModeArg,
    #[arg(long, default_value = "passed")]
    validator_status: String,
    #[arg(long)]
    validator_command: Option<String>,
    #[arg(long)]
    validator_result_ref: Option<String>,
    #[arg(long)]
    review_ref: Option<String>,
    #[arg(long = "evidence-ref")]
    evidence_refs: Vec<String>,
    #[arg(long, default_value = "workflow-cli")]
    owner: String,
    #[arg(long)]
    lane: Option<String>,
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct WorkflowMissionStatusCommand {
    #[command(flatten)]
    target: WorkflowReadTargetArgs,
    #[arg(long)]
    mission_id: Option<String>,
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct WorkflowMissionReadCommand {
    #[command(flatten)]
    target: WorkflowReadTargetArgs,
    #[arg(long)]
    mission_id: String,
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ValidatorModeArg {
    MissionValidatorScript,
    PromptArchitectArtifact,
}

impl ValidatorModeArg {
    fn to_core(self) -> ResearchValidatorMode {
        match self {
            Self::MissionValidatorScript => ResearchValidatorMode::MissionValidatorScript,
            Self::PromptArchitectArtifact => ResearchValidatorMode::PromptArchitectArtifact,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum MissionStatusArg {
    Complete,
    Blocked,
    Failed,
}

impl MissionStatusArg {
    fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Blocked => "blocked",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Args, Clone)]
struct WorkflowWikiCommand {
    #[command(subcommand)]
    command: WorkflowWikiCommands,
}

#[derive(Debug, Subcommand, Clone)]
enum WorkflowWikiCommands {
    Add(WorkflowWikiAddCommand),
    Read(WorkflowWikiReadCommand),
    List(WorkflowWikiListCommand),
    Query(WorkflowWikiQueryCommand),
    Lint(WorkflowWikiListCommand),
    Refresh(WorkflowWikiListCommand),
    Delete(WorkflowWikiDeleteCommand),
}

#[derive(Debug, Args, Clone)]
struct WorkflowWikiAddCommand {
    #[arg(long)]
    workflow_id: Option<String>,
    #[arg(long)]
    slug: String,
    #[arg(long)]
    title: String,
    #[arg(long)]
    category: String,
    #[arg(long = "tag")]
    tags: Vec<String>,
    #[arg(long)]
    body: Option<String>,
    #[arg(long)]
    body_file: Option<PathBuf>,
    #[arg(long, default_value = "workflow-cli")]
    owner: String,
    #[arg(long)]
    lane: Option<String>,
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct WorkflowWikiReadCommand {
    #[arg(long)]
    slug: String,
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct WorkflowWikiListCommand {
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct WorkflowWikiQueryCommand {
    #[arg(long)]
    term: Option<String>,
    #[arg(long)]
    tag: Option<String>,
    #[arg(long)]
    category: Option<String>,
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct WorkflowWikiDeleteCommand {
    #[arg(long)]
    workflow_id: Option<String>,
    #[arg(long)]
    slug: String,
    #[arg(long, default_value = "workflow-cli")]
    owner: String,
    #[arg(long)]
    lane: Option<String>,
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct WorkflowEvidenceCommand {
    #[command(subcommand)]
    command: WorkflowEvidenceCommands,
}

#[derive(Debug, Subcommand, Clone)]
enum WorkflowEvidenceCommands {
    Record(WorkflowEvidenceRecordCommand),
}

#[derive(Debug, Args, Clone)]
struct WorkflowEvidenceRecordCommand {
    /// Stable workflow id receiving the evidence.
    #[arg(long)]
    workflow_id: String,

    /// Workflow mode to start for this evidence run.
    #[arg(long, default_value = "workflow.operator_utility")]
    mode: String,

    /// Evidence category registered in harness-core::workflow_registry.
    #[arg(long)]
    category: String,

    /// Human-readable evidence summary.
    #[arg(long)]
    summary: String,

    /// Optional acceptance criterion or artifact id this evidence satisfies.
    #[arg(long)]
    acceptance_ref: Option<String>,

    /// Optional redacted artifact path under the run/artifact contract.
    #[arg(long)]
    artifact_path: Option<String>,

    /// Optional digest for the artifact path.
    #[arg(long)]
    artifact_digest: Option<String>,

    /// Metadata as key=value; may be repeated.
    #[arg(long = "metadata")]
    metadata: Vec<String>,

    /// Status metadata shorthand. Blocking statuses (failed/blocked/etc.) block closeout.
    #[arg(long)]
    status: Option<String>,

    /// Metadata key to receive --status. Defaults to status.
    #[arg(long, default_value = "status")]
    status_key: String,

    /// Human-readable workflow title.
    #[arg(long)]
    title: Option<String>,

    /// Workflow owner recorded on lifecycle events.
    #[arg(long, default_value = "workflow-cli")]
    owner: String,

    /// Workflow lane. Defaults to runtime.workflow.run.default_lane.
    #[arg(long)]
    lane: Option<String>,

    /// Emit machine-readable JSON.
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
#[command(group(
    ArgGroup::new("mode")
        .required(true)
        .args(["check", "apply"])
))]
struct WorkflowInitCommand {
    /// Check bootstrap status without writing files.
    #[arg(long, default_value_t = false)]
    check: bool,

    /// Apply safe generated workflow files under .agent-harness/.
    #[arg(long, default_value_t = false)]
    apply: bool,

    /// Emit machine-readable JSON.
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Args, Clone, Default)]
struct WorkflowReadTargetArgs {
    /// Run directory containing events.jsonl.
    #[arg(long)]
    run_dir: Option<PathBuf>,

    /// Run id under the configured session directory.
    #[arg(long)]
    run_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct WorkflowRunReport {
    run_id: String,
    run_dir: String,
    events_path: String,
    workflow_id: String,
    result: WorkflowStartResult,
}

#[derive(Debug, Serialize)]
struct WorkflowStatusReport {
    run_dir: String,
    events_path: String,
    workflow_count: usize,
    active_count: usize,
    projection: WorkflowProjection,
    persistent_tasks: PersistentTaskProjection,
    closeout: BTreeMap<String, WorkflowStatusCloseoutReport>,
}

#[derive(Debug, Serialize)]
struct WorkflowMutationReport {
    run_id: String,
    run_dir: String,
    events_path: String,
    workflow_id: String,
    decision: String,
    terminal_outcome: Option<String>,
    audit_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    signoff: Option<WorkflowSignoffReport>,
}

#[derive(Debug, Serialize)]
struct WorkflowEvidenceRecordReport {
    run_id: String,
    run_dir: String,
    events_path: String,
    workflow_id: String,
    mode: String,
    category: String,
    summary: String,
    artifact_path: Option<String>,
    artifact_digest: Option<String>,
    acceptance_ref: Option<String>,
    metadata: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
struct SnapshotWriteReport {
    run_id: String,
    run_dir: String,
    events_path: String,
    snapshot: ContextSnapshotWriteResult,
}

#[derive(Debug, Serialize)]
struct PlanConsensusReport {
    run_id: String,
    run_dir: String,
    events_path: String,
    workflow_id: String,
    plan_id: String,
    status: String,
    critic_verdict: String,
    critic_iterations: u32,
    max_iterations: u32,
    lanes: Vec<PlanConsensusLane>,
    artifact_path: String,
    artifact_digest: String,
    artifact_bytes: u64,
}

#[derive(Debug, Serialize)]
struct GoalMutationReport {
    run_id: String,
    run_dir: String,
    events_path: String,
    workflow_id: String,
    goal_id: String,
    status: String,
    artifact_path: String,
    artifact_digest: String,
    artifact_bytes: u64,
}

#[derive(Debug, Serialize)]
struct GoalStatusReport {
    run_dir: String,
    events_path: String,
    projection: GoalLedgerProjection,
}

#[derive(Debug, Serialize)]
struct MissionMutationReport {
    run_id: String,
    run_dir: String,
    events_path: String,
    workflow_id: String,
    mission_id: String,
    status: String,
    artifact_path: String,
    artifact_digest: String,
    artifact_bytes: u64,
}

#[derive(Debug, Serialize)]
struct MissionStatusReport {
    run_dir: String,
    events_path: String,
    projection: ResearchMissionProjection,
}

#[derive(Debug, Serialize)]
struct WikiMutationReport {
    run_id: String,
    run_dir: String,
    events_path: String,
    workflow_id: String,
    action: String,
    page: WikiPageSummary,
}

struct WikiMutationAudit {
    coordinator: CoordinatorHandle,
    run: RunInfo,
    workflow_id: String,
    action: String,
    page: WikiPageSummary,
}

#[derive(Debug, Serialize)]
struct WikiListReport {
    root: String,
    pages: Vec<WikiPageSummary>,
}

#[derive(Debug, Serialize)]
struct WikiQueryReport {
    root: String,
    matches: Vec<WikiPageSummary>,
}

#[derive(Debug, Serialize)]
struct WikiLintReport {
    root: String,
    findings: Vec<WikiLintFinding>,
}

#[derive(Debug, Serialize)]
struct InitReport {
    mode: String,
    applied: bool,
    files: Vec<InitFileReport>,
}

#[derive(Debug, Serialize)]
struct InitFileReport {
    path: String,
    exists: bool,
    action: String,
}

#[derive(Debug, Serialize)]
struct DossierExportReport {
    run_dir: String,
    events_path: String,
    format: String,
    output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dossier: Option<RunDossier>,
    body: String,
}

pub fn execute(
    cmd: WorkflowCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("workflow command failed: failed to build async runtime: {err}");
            return ExitCode::from(1);
        }
    };

    let result = match cmd.command {
        WorkflowCommands::Run(run) => {
            runtime.block_on(execute_run(run, config_path, global_session_dir))
        }
        WorkflowCommands::Status(status) => execute_status(status, config_path, global_session_dir),
        WorkflowCommands::Signoff(signoff) => {
            runtime.block_on(execute_signoff(signoff, config_path, global_session_dir))
        }
        WorkflowCommands::Cancel(cancel) => {
            runtime.block_on(execute_cancel(cancel, config_path, global_session_dir))
        }
        WorkflowCommands::Dossier(dossier) => match dossier.command {
            WorkflowDossierCommands::Export(export) => {
                execute_dossier_export(export, config_path, global_session_dir)
            }
        },
        WorkflowCommands::Snapshot(snapshot) => match snapshot.command {
            WorkflowSnapshotCommands::Write(write) => runtime.block_on(execute_snapshot_write(
                write,
                config_path,
                global_session_dir,
            )),
            WorkflowSnapshotCommands::List(list) => {
                execute_snapshot_list(list, config_path, global_session_dir)
            }
            WorkflowSnapshotCommands::Read(read) | WorkflowSnapshotCommands::Export(read) => {
                execute_snapshot_read(read, config_path, global_session_dir)
            }
        },
        WorkflowCommands::PlanConsensus(plan) => runtime.block_on(execute_plan_consensus(
            *plan,
            config_path,
            global_session_dir,
        )),
        WorkflowCommands::Goal(goal) => {
            let goal = *goal;
            match goal.command {
                WorkflowGoalCommands::Create(create) => {
                    runtime.block_on(execute_goal_create(create, config_path, global_session_dir))
                }
                WorkflowGoalCommands::Checkpoint(checkpoint) => runtime.block_on(
                    execute_goal_checkpoint(checkpoint, config_path, global_session_dir),
                ),
                WorkflowGoalCommands::Status(status) | WorkflowGoalCommands::List(status) => {
                    execute_goal_status(status, config_path, global_session_dir)
                }
                WorkflowGoalCommands::Read(read) => {
                    execute_goal_read(read, config_path, global_session_dir)
                }
            }
        }
        WorkflowCommands::Mission(mission) => {
            let mission = *mission;
            match mission.command {
                WorkflowMissionCommands::Init(init) => {
                    runtime.block_on(execute_mission_init(init, config_path, global_session_dir))
                }
                WorkflowMissionCommands::Run(run) => {
                    runtime.block_on(execute_mission_run(run, config_path, global_session_dir))
                }
                WorkflowMissionCommands::Status(status) => {
                    execute_mission_status(status, config_path, global_session_dir)
                }
                WorkflowMissionCommands::Read(read) => {
                    execute_mission_read(read, config_path, global_session_dir)
                }
            }
        }
        WorkflowCommands::Wiki(wiki) => {
            let wiki = *wiki;
            match wiki.command {
                WorkflowWikiCommands::Add(add) => {
                    runtime.block_on(execute_wiki_add(add, config_path, global_session_dir))
                }
                WorkflowWikiCommands::Delete(delete) => {
                    runtime.block_on(execute_wiki_delete(delete, config_path, global_session_dir))
                }
                WorkflowWikiCommands::Read(read) => {
                    execute_wiki_read(read, config_path, global_session_dir)
                }
                WorkflowWikiCommands::List(list) | WorkflowWikiCommands::Refresh(list) => {
                    execute_wiki_list(list, config_path, global_session_dir)
                }
                WorkflowWikiCommands::Query(query) => {
                    execute_wiki_query(query, config_path, global_session_dir)
                }
                WorkflowWikiCommands::Lint(lint) => {
                    execute_wiki_lint(lint, config_path, global_session_dir)
                }
            }
        }
        WorkflowCommands::Evidence(evidence) => {
            let evidence = *evidence;
            match evidence.command {
                WorkflowEvidenceCommands::Record(record) => runtime.block_on(
                    execute_evidence_record(record, config_path, global_session_dir),
                ),
            }
        }
        WorkflowCommands::Init(init) => {
            runtime.block_on(execute_init(init, config_path, global_session_dir))
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("workflow command failed: {err}");
            ExitCode::from(1)
        }
    }
}

async fn execute_run(
    cmd: WorkflowRunCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    let context = resolve_workflow_context(config_path, global_session_dir)?;
    fs::create_dir_all(&context.session_dir).map_err(|err| {
        format!(
            "failed to create session dir {}: {err}",
            context.session_dir.display()
        )
    })?;

    let coordinator_config = coordinator_config_for_workflow(&context);
    let clock: Arc<dyn Clock + Send + Sync> = Arc::new(RealClock::new());
    let coordinator = spawn_coordinator(
        coordinator_config,
        Arc::clone(&clock),
        Arc::new(DefaultRedactor::default()),
    );
    let workspace = std::env::current_dir()
        .map_err(|err| format!("failed to resolve current working directory: {err}"))?;
    let run_name = cmd
        .title
        .clone()
        .unwrap_or_else(|| "workflow run".to_string());
    let run = coordinator
        .start_run(run_name, workspace)
        .await
        .map_err(|err| err.to_string())?;
    let workflow_id = cmd
        .workflow_id
        .clone()
        .unwrap_or_else(|| format!("wf_{}", run.run_id));
    let lane = cmd
        .lane
        .clone()
        .or_else(|| Some(context.workflow.run.default_lane.clone()));
    let result = coordinator
        .start_workflow(
            supervisor_actor(),
            WorkflowStartRequest {
                workflow_id: workflow_id.clone(),
                mode: "workflow.run".to_string(),
                owner: cmd.owner.clone(),
                lane,
                title: cmd.title.clone(),
                idempotency_key: cmd.idempotency_key.clone(),
            },
        )
        .await
        .map_err(|err| err.to_string())?;
    coordinator
        .stop_run()
        .await
        .map_err(|err| err.to_string())?;

    let report = WorkflowRunReport {
        run_id: run.run_id.clone(),
        run_dir: run.run_dir.display().to_string(),
        events_path: run.events_path.display().to_string(),
        workflow_id,
        result,
    };
    if cmd.json {
        print_json(&report, "workflow run JSON")?;
    } else {
        println!(
            "workflow run started: {} run={} events={}",
            report.workflow_id, report.run_id, report.events_path
        );
    }
    Ok(())
}

async fn execute_evidence_record(
    cmd: WorkflowEvidenceRecordCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    if !is_evidence_category(&cmd.category) {
        return Err(format!(
            "unknown workflow evidence category `{}`; known categories: {}",
            cmd.category,
            evidence_category_ids().join(", ")
        ));
    }

    let context = resolve_workflow_context(config_path, global_session_dir)?;
    fs::create_dir_all(&context.session_dir).map_err(|err| {
        format!(
            "failed to create session dir {}: {err}",
            context.session_dir.display()
        )
    })?;

    let mut metadata = parse_metadata_pairs(&cmd.metadata)?;
    if let Some(status) = cmd.status.as_ref() {
        let key = cmd.status_key.trim();
        if key.is_empty() {
            return Err("--status-key cannot be empty".to_string());
        }
        metadata.insert(key.to_string(), status.trim().to_string());
    }

    let coordinator_config = coordinator_config_for_workflow(&context);
    let clock: Arc<dyn Clock + Send + Sync> = Arc::new(RealClock::new());
    let coordinator = spawn_coordinator(
        coordinator_config,
        Arc::clone(&clock),
        Arc::new(DefaultRedactor::default()),
    );
    let workspace = std::env::current_dir()
        .map_err(|err| format!("failed to resolve current working directory: {err}"))?;
    let run = coordinator
        .start_run(
            cmd.title
                .clone()
                .unwrap_or_else(|| "workflow evidence record".to_string()),
            workspace,
        )
        .await
        .map_err(|err| err.to_string())?;
    let lane = cmd
        .lane
        .clone()
        .or_else(|| Some(context.workflow.run.default_lane.clone()));
    coordinator
        .start_workflow(
            supervisor_actor(),
            WorkflowStartRequest {
                workflow_id: cmd.workflow_id.clone(),
                mode: cmd.mode.clone(),
                owner: cmd.owner.clone(),
                lane,
                title: cmd.title.clone(),
                idempotency_key: None,
            },
        )
        .await
        .map_err(|err| err.to_string())?;
    coordinator
        .record_workflow_evidence(
            supervisor_actor(),
            WorkflowEvidenceRequest {
                workflow_id: cmd.workflow_id.clone(),
                category: cmd.category.clone(),
                summary: cmd.summary.clone(),
                artifact_path: cmd.artifact_path.clone(),
                artifact_digest: cmd.artifact_digest.clone(),
                acceptance_ref: cmd.acceptance_ref.clone(),
                metadata: metadata.clone(),
            },
        )
        .await
        .map_err(|err| err.to_string())?;
    coordinator
        .stop_run()
        .await
        .map_err(|err| err.to_string())?;

    let report = WorkflowEvidenceRecordReport {
        run_id: run.run_id,
        run_dir: run.run_dir.display().to_string(),
        events_path: run.events_path.display().to_string(),
        workflow_id: cmd.workflow_id,
        mode: cmd.mode,
        category: cmd.category,
        summary: cmd.summary,
        artifact_path: cmd.artifact_path,
        artifact_digest: cmd.artifact_digest,
        acceptance_ref: cmd.acceptance_ref,
        metadata,
    };
    if cmd.json {
        print_json(&report, "workflow evidence record JSON")?;
    } else {
        println!(
            "workflow evidence recorded: {} category={} run={}",
            report.workflow_id, report.category, report.run_dir
        );
    }
    Ok(())
}

fn execute_status(
    cmd: WorkflowStatusCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    let report = status_report(cmd.target, cmd.workflow_id, config_path, global_session_dir)?;
    if cmd.json {
        print_json(&report, "workflow status JSON")?;
    } else {
        println!(
            "workflow status: {} workflow(s), {} active, run={}",
            report.workflow_count, report.active_count, report.run_dir
        );
        for workflow in report.projection.workflows.values() {
            let closeout = report
                .closeout
                .get(&workflow.workflow_id)
                .map(|report| report.closeout.overall_allowed)
                .unwrap_or(false);
            println!(
                "- {} mode={} status={} owner={} closeout_allowed={}",
                workflow.workflow_id, workflow.mode, workflow.status, workflow.owner, closeout
            );
        }
    }
    Ok(())
}

async fn execute_signoff(
    cmd: WorkflowSignoffCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    let signoff_decision = signoff_decision_from_command(&cmd);
    let scope = closeout_decision_scope(&cmd, &signoff_decision)?;
    let decision = workflow_operator_decision(&signoff_decision, scope.as_deref())?;
    let terminal_outcome = terminal_outcome_for_decision(&signoff_decision);
    if signoff_decision.requires_reason() && cmd.reason.is_none() {
        return Err(format!(
            "`--reason` is required for workflow signoff decision `{decision}`"
        ));
    }
    let reason = cmd
        .reason
        .clone()
        .unwrap_or_else(|| format!("workflow signoff decision: {decision}"));
    let report = if cmd.audit_only {
        execute_workflow_audit_mutation(WorkflowAuditMutationRequest {
            config_path,
            global_session_dir,
            run_name: "workflow signoff",
            workflow_id: &cmd.workflow_id,
            decision: &decision,
            owner: &cmd.operator,
            reason: &reason,
            terminal_outcome,
            audit_only: true,
        })
        .await?
    } else {
        execute_workflow_target_signoff(
            &cmd,
            &decision,
            signoff_decision,
            terminal_outcome,
            &reason,
            config_path,
            global_session_dir,
        )
        .await?
    };

    if cmd.json {
        print_json(&report, "workflow signoff JSON")?;
    } else {
        println!(
            "workflow signoff recorded: {} decision={} run={} audit_only={}",
            report.workflow_id, report.decision, report.run_id, report.audit_only
        );
        if report.audit_only {
            println!("audit-only signoff recorded a detached audit run and did not close a target workflow");
        }
    }
    Ok(())
}

fn signoff_decision_from_command(cmd: &WorkflowSignoffCommand) -> WorkflowSignoffDecision {
    if cmd.approve {
        WorkflowSignoffDecision::Approve
    } else if cmd.fail {
        WorkflowSignoffDecision::Fail
    } else if cmd.waive {
        WorkflowSignoffDecision::Waive
    } else if cmd.abort {
        WorkflowSignoffDecision::Abort
    } else if cmd.redirect {
        WorkflowSignoffDecision::Redirect
    } else if cmd.approve_live {
        WorkflowSignoffDecision::ApproveLive
    } else {
        WorkflowSignoffDecision::RequestEvidence
    }
}

fn closeout_decision_scope(
    cmd: &WorkflowSignoffCommand,
    decision: &WorkflowSignoffDecision,
) -> Result<Option<String>, String> {
    if decision.requires_scope() {
        let scope = cmd
            .scope
            .as_deref()
            .map(str::trim)
            .filter(|scope| !scope.is_empty());
        return scope.map(|scope| Some(scope.to_string())).ok_or_else(|| {
            format!("`--scope` is required for workflow signoff decision `{decision:?}`")
        });
    }
    Ok(cmd.scope.clone())
}

fn workflow_operator_decision(
    decision: &WorkflowSignoffDecision,
    scope: Option<&str>,
) -> Result<String, String> {
    match decision {
        WorkflowSignoffDecision::Approve => Ok("signoff-approved".to_string()),
        WorkflowSignoffDecision::Fail => Ok("signoff-failed".to_string()),
        WorkflowSignoffDecision::RequestEvidence => Ok("request-evidence".to_string()),
        WorkflowSignoffDecision::Waive => {
            let scope = scope.ok_or_else(|| {
                "`--scope` is required for workflow signoff decision `Waive`".to_string()
            })?;
            Ok(format!("waive:{scope}"))
        }
        WorkflowSignoffDecision::Abort => Ok("abort".to_string()),
        WorkflowSignoffDecision::Redirect => {
            let scope = scope.ok_or_else(|| {
                "`--scope` is required for workflow signoff decision `Redirect`".to_string()
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

async fn execute_workflow_target_signoff(
    cmd: &WorkflowSignoffCommand,
    decision: &str,
    signoff_decision: WorkflowSignoffDecision,
    terminal_outcome: Option<&str>,
    reason: &str,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<WorkflowMutationReport, String> {
    let context = resolve_workflow_context(config_path.clone(), global_session_dir.clone())?;
    let run_dir = resolve_read_run_dir(cmd.target.clone(), config_path, global_session_dir)?;
    let (session_dir, run_id) = session_dir_and_run_id_from_run_dir(&run_dir)?;
    let events = load_events_from_run_dir(&run_dir)?;
    let projection = project_workflows(events.iter().map(|event| &event.payload));
    let persistent_tasks = project_persistent_tasks(&events);
    let workflow = projection
        .workflows
        .get(&cmd.workflow_id)
        .ok_or_else(|| {
            format!(
                "workflow `{}` was not found in target run {}; pass --audit-only for detached compatibility",
                cmd.workflow_id,
                run_dir.display()
            )
        })?;
    if workflow.terminal {
        if cmd.approve && workflow.status == "outcome.finished" {
            return Ok(WorkflowMutationReport {
                run_id,
                run_dir: run_dir.display().to_string(),
                events_path: run_dir.join(EVENTS_FILE_NAME).display().to_string(),
                workflow_id: cmd.workflow_id.clone(),
                decision: decision.to_string(),
                terminal_outcome: terminal_outcome.map(str::to_string),
                audit_only: false,
                signoff: None,
            });
        }
        return Err(format!(
            "workflow `{}` is already terminal with status `{}`",
            cmd.workflow_id, workflow.status
        ));
    }

    let signoff_policy = WorkflowSignoffPolicy::simulator_default();
    let closeout_policy = effective_closeout_policy(&context, cmd.policy_id.as_deref())?;
    if cmd.approve {
        let readiness = projection.closeout_readiness(
            cmd.workflow_id.clone(),
            &persistent_tasks,
            &signoff_policy,
            &closeout_policy,
        );
        if !readiness.overall_allowed {
            let coordinator = workflow_mutation_coordinator(&context, session_dir.clone());
            coordinator
                .attach_workflow_mutation_run(run_id.clone(), "workflow signoff")
                .await
                .map_err(|err| err.to_string())?;
            let premature_result = coordinator
                .complete_workflow_with_closeout_policy(
                    supervisor_actor(),
                    cmd.workflow_id.clone(),
                    "outcome.finished".to_string(),
                    reason.to_string(),
                    cmd.operator.clone(),
                    signoff_policy,
                    closeout_policy,
                )
                .await;
            return match premature_result {
                Ok(_) => Err(format!(
                    "workflow `{}` premature approval unexpectedly passed closeout policy",
                    cmd.workflow_id
                )),
                Err(err) => Err(err.to_string()),
            };
        }
    }
    if cmd.approve_live && !closeout_policy.allow_live_approval {
        return Err(format!(
            "workflow closeout policy `{}` does not allow approve-live",
            closeout_policy.policy_id
        ));
    }

    let coordinator = workflow_mutation_coordinator(&context, session_dir);
    let run = coordinator
        .attach_workflow_mutation_run(run_id.clone(), "workflow signoff")
        .await
        .map_err(|err| err.to_string())?;
    coordinator
        .record_workflow_operator_decision(
            supervisor_actor(),
            cmd.workflow_id.clone(),
            decision.to_string(),
            cmd.operator.clone(),
            Some(reason.to_string()),
            None,
        )
        .await
        .map_err(|err| err.to_string())?;
    if let Some(outcome) = terminal_outcome {
        if outcome == "outcome.finished" && cmd.approve {
            coordinator
                .complete_workflow_with_closeout_policy(
                    supervisor_actor(),
                    cmd.workflow_id.clone(),
                    outcome.to_string(),
                    reason.to_string(),
                    cmd.operator.clone(),
                    signoff_policy.clone(),
                    closeout_policy.clone(),
                )
                .await
                .map_err(|err| err.to_string())?;
        } else {
            coordinator
                .complete_workflow(
                    supervisor_actor(),
                    cmd.workflow_id.clone(),
                    outcome.to_string(),
                    reason.to_string(),
                    cmd.operator.clone(),
                )
                .await
                .map_err(|err| err.to_string())?;
        }
    }
    let refreshed_events = load_events_from_run_dir(&run.run_dir)?;
    let refreshed_projection =
        project_workflows(refreshed_events.iter().map(|event| &event.payload));
    let refreshed_tasks = project_persistent_tasks(&refreshed_events);
    let closeout = closeout_readiness_for_report(
        &refreshed_projection,
        &refreshed_tasks,
        &signoff_policy,
        &closeout_policy,
        &cmd.workflow_id,
        Some(run.run_id.clone()),
    );

    Ok(WorkflowMutationReport {
        run_id: run.run_id,
        run_dir: run.run_dir.display().to_string(),
        events_path: run.events_path.display().to_string(),
        workflow_id: cmd.workflow_id.clone(),
        decision: decision.to_string(),
        terminal_outcome: terminal_outcome.map(str::to_string),
        audit_only: false,
        signoff: Some(WorkflowSignoffReport {
            workflow_id: cmd.workflow_id.clone(),
            decision: signoff_decision,
            audit_only: false,
            accepted: true,
            closeout,
            reason: Some(reason.to_string()),
        }),
    })
}

async fn execute_cancel(
    cmd: WorkflowCancelCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    let reason = format!("{} (mode={})", cmd.reason, cmd.mode);
    let report = execute_workflow_audit_mutation(WorkflowAuditMutationRequest {
        config_path,
        global_session_dir,
        run_name: "workflow cancel",
        workflow_id: &cmd.workflow_id,
        decision: "abort",
        owner: &cmd.owner,
        reason: &reason,
        terminal_outcome: Some("outcome.cancelled"),
        audit_only: false,
    })
    .await?;

    if cmd.json {
        print_json(&report, "workflow cancel JSON")?;
    } else {
        println!(
            "workflow cancel recorded: {} outcome=outcome.cancelled audit_run={}",
            report.workflow_id, report.run_id
        );
    }
    Ok(())
}

struct WorkflowAuditMutationRequest<'a> {
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
    run_name: &'a str,
    workflow_id: &'a str,
    decision: &'a str,
    owner: &'a str,
    reason: &'a str,
    terminal_outcome: Option<&'a str>,
    audit_only: bool,
}

async fn execute_workflow_audit_mutation(
    request: WorkflowAuditMutationRequest<'_>,
) -> Result<WorkflowMutationReport, String> {
    let WorkflowAuditMutationRequest {
        config_path,
        global_session_dir,
        run_name,
        workflow_id,
        decision,
        owner,
        reason,
        terminal_outcome,
        audit_only,
    } = request;
    let context = resolve_workflow_context(config_path, global_session_dir)?;
    if audit_only && !context.workflow.closeout.allow_audit_only {
        return Err(
            "runtime.workflow.closeout.allow_audit_only=false disables detached workflow signoff"
                .to_string(),
        );
    }
    fs::create_dir_all(&context.session_dir).map_err(|err| {
        format!(
            "failed to create session dir {}: {err}",
            context.session_dir.display()
        )
    })?;
    let audit_lane = Some(context.workflow.run.default_lane.clone());

    let mut coordinator_config = CoordinatorConfig::new(context.session_dir.clone());
    coordinator_config.session_mode_source = Some(SessionModeSource::Prompt);
    let clock: Arc<dyn Clock + Send + Sync> = Arc::new(RealClock::new());
    let coordinator = spawn_coordinator(
        coordinator_config,
        Arc::clone(&clock),
        Arc::new(DefaultRedactor::default()),
    );
    let workspace = std::env::current_dir()
        .map_err(|err| format!("failed to resolve current working directory: {err}"))?;
    let run = coordinator
        .start_run(run_name.to_string(), workspace)
        .await
        .map_err(|err| err.to_string())?;
    coordinator
        .start_workflow(
            supervisor_actor(),
            WorkflowStartRequest {
                workflow_id: workflow_id.to_string(),
                mode: run_name.replace(' ', "."),
                owner: owner.to_string(),
                lane: audit_lane,
                title: Some(run_name.to_string()),
                idempotency_key: None,
            },
        )
        .await
        .map_err(|err| err.to_string())?;
    coordinator
        .record_workflow_operator_decision(
            supervisor_actor(),
            workflow_id.to_string(),
            decision.to_string(),
            owner.to_string(),
            Some(reason.to_string()),
            None,
        )
        .await
        .map_err(|err| err.to_string())?;
    if let Some(outcome) = terminal_outcome {
        coordinator
            .complete_workflow(
                supervisor_actor(),
                workflow_id.to_string(),
                outcome.to_string(),
                reason.to_string(),
                owner.to_string(),
            )
            .await
            .map_err(|err| err.to_string())?;
    }
    coordinator
        .stop_run()
        .await
        .map_err(|err| err.to_string())?;

    Ok(WorkflowMutationReport {
        run_id: run.run_id,
        run_dir: run.run_dir.display().to_string(),
        events_path: run.events_path.display().to_string(),
        workflow_id: workflow_id.to_string(),
        decision: decision.to_string(),
        terminal_outcome: terminal_outcome.map(str::to_string),
        audit_only,
        signoff: None,
    })
}

fn execute_dossier_export(
    cmd: WorkflowDossierExportCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    let context = resolve_workflow_context(config_path.clone(), global_session_dir.clone())?;
    let closeout_policy = effective_closeout_policy(&context, None)?;
    let report = status_report(cmd.target, cmd.workflow_id, config_path, global_session_dir)?;
    let dossier = build_run_dossier_with_tasks_and_closeout_policy(
        &report.projection,
        &report.persistent_tasks,
        &WorkflowSignoffPolicy::simulator_default(),
        &closeout_policy,
    );
    let body = match cmd.format {
        DossierFormat::Json => serde_json::to_string_pretty(&dossier)
            .map_err(|err| format!("failed to render dossier JSON: {err}"))?,
        DossierFormat::Markdown => render_dossier_markdown(&report, &dossier),
    };
    if let Some(output) = cmd.output.as_ref() {
        write_explicit_output(output, &body)?;
    }
    if cmd.json {
        let export = DossierExportReport {
            run_dir: report.run_dir,
            events_path: report.events_path,
            format: match cmd.format {
                DossierFormat::Json => "json".to_string(),
                DossierFormat::Markdown => "markdown".to_string(),
            },
            output: cmd.output.map(|path| path.display().to_string()),
            dossier: matches!(cmd.format, DossierFormat::Json).then_some(dossier),
            body,
        };
        print_json(&export, "workflow dossier export JSON")?;
    } else {
        println!("{body}");
    }
    Ok(())
}

async fn execute_snapshot_write(
    cmd: WorkflowSnapshotWriteCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    let context = resolve_workflow_context(config_path, global_session_dir)?;
    fs::create_dir_all(&context.session_dir).map_err(|err| {
        format!(
            "failed to create session dir {}: {err}",
            context.session_dir.display()
        )
    })?;

    let coordinator_config = coordinator_config_for_workflow(&context);
    let clock: Arc<dyn Clock + Send + Sync> = Arc::new(RealClock::new());
    let coordinator = spawn_coordinator(
        coordinator_config,
        Arc::clone(&clock),
        Arc::new(DefaultRedactor::default()),
    );
    let workspace = std::env::current_dir()
        .map_err(|err| format!("failed to resolve current working directory: {err}"))?;
    let run = coordinator
        .start_run("workflow context snapshot", workspace)
        .await
        .map_err(|err| err.to_string())?;

    let input = snapshot_input_from_command(&cmd);
    let options = ContextSnapshotOptions {
        max_text_chars: cmd.max_text_chars,
        max_list_items: cmd.max_list_items,
    };
    let result = coordinator
        .write_context_snapshot(supervisor_actor(), cmd.workflow_id.clone(), input, options)
        .await;

    let stop_result = coordinator.stop_run().await;
    let result = result.map_err(|err| err.to_string())?;
    stop_result.map_err(|err| err.to_string())?;

    print_snapshot_write_report(&cmd, &run, result)
}

fn execute_snapshot_list(
    cmd: WorkflowSnapshotListCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    let report = status_report(cmd.target, None, config_path, global_session_dir)?;
    if cmd.json {
        print_json(
            &report.projection.context_snapshots,
            "workflow snapshot list JSON",
        )?;
    } else {
        for snapshot in report.projection.context_snapshots.values() {
            println!(
                "{} slug={} artifact={}",
                snapshot.snapshot_id, snapshot.slug, snapshot.artifact_path
            );
        }
    }
    Ok(())
}

fn execute_snapshot_read(
    cmd: WorkflowSnapshotReadCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    let report = status_report(cmd.target, None, config_path, global_session_dir)?;
    let snapshot = report
        .projection
        .context_snapshots
        .get(&cmd.snapshot_id)
        .ok_or_else(|| format!("snapshot `{}` not found in projection", cmd.snapshot_id))?;
    let body = serde_json::to_string_pretty(snapshot)
        .map_err(|err| format!("failed to render snapshot JSON: {err}"))?;
    if let Some(output) = cmd.output.as_ref() {
        write_explicit_output(output, &body)?;
    }
    if cmd.json {
        println!("{body}");
    } else {
        println!(
            "workflow snapshot: {} slug={} artifact={}",
            snapshot.snapshot_id, snapshot.slug, snapshot.artifact_path
        );
    }
    Ok(())
}

async fn execute_plan_consensus(
    cmd: WorkflowPlanConsensusCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    let context = resolve_workflow_context(config_path, global_session_dir)?;
    fs::create_dir_all(&context.session_dir).map_err(|err| {
        format!(
            "failed to create session dir {}: {err}",
            context.session_dir.display()
        )
    })?;

    let coordinator_config = coordinator_config_for_workflow(&context);
    let clock: Arc<dyn Clock + Send + Sync> = Arc::new(RealClock::new());
    let coordinator = spawn_coordinator(
        coordinator_config,
        Arc::clone(&clock),
        Arc::new(DefaultRedactor::default()),
    );
    let workspace = std::env::current_dir()
        .map_err(|err| format!("failed to resolve current working directory: {err}"))?;
    let run = coordinator
        .start_run("workflow plan consensus".to_string(), workspace)
        .await
        .map_err(|err| err.to_string())?;
    let workflow_id = cmd
        .workflow_id
        .clone()
        .unwrap_or_else(|| format!("wf_plan_{}", run.run_id));
    let plan_id = cmd
        .plan_id
        .clone()
        .unwrap_or_else(|| format!("plan_{}", run.run_id));
    let lane = cmd
        .lane
        .clone()
        .or_else(|| Some(context.workflow.run.default_lane.clone()));
    let max_iterations = cmd
        .max_iterations
        .unwrap_or(context.workflow.plan_consensus.max_iterations);
    let options = parse_plan_options(&cmd.options)?;
    let chosen_option = cmd
        .chosen_option
        .clone()
        .or_else(|| options.first().map(|option| option.id.clone()))
        .unwrap_or_default();
    let artifact = PlanConsensusArtifact {
        schema_version: PLAN_CONSENSUS_SCHEMA_VERSION,
        workflow_id: workflow_id.clone(),
        plan_id: plan_id.clone(),
        task: cmd.task.clone(),
        snapshot_ref: cmd.snapshot_ref.clone(),
        lanes: resolve_plan_consensus_lanes(context.agent_catalog.as_ref()),
        max_iterations,
        critic_iterations: cmd.critic_iterations,
        critic_verdict: cmd.critic_verdict.clone(),
        principles: cmd.principles.clone(),
        decision_drivers: cmd.decision_drivers.clone(),
        options,
        chosen_option,
        rejected_alternatives: cmd.rejected_alternatives.clone(),
        adr: cmd.adr.clone(),
        work_breakdown: cmd.work_breakdown.clone(),
        risks: cmd.risks.clone(),
        test_plan: cmd.test_plan.clone(),
        manual_qa_plan: cmd.manual_qa_plan.clone(),
        staffing: cmd.staffing.clone(),
        handoff_options: cmd.handoff_options.clone(),
        acceptance_criteria: cmd.acceptance_criteria.clone(),
        evidence_refs: cmd.evidence_refs.clone(),
    };
    validate_plan_consensus_artifact(&artifact)
        .map_err(|errors| format!("invalid plan consensus artifact: {}", errors.join("; ")))?;
    let (artifact_path, artifact_digest, artifact_bytes) =
        write_json_artifact(&run, &plan_consensus_artifact_name(&plan_id), &artifact)?;
    let metadata = plan_consensus_metadata(&artifact);
    coordinator
        .start_workflow(
            supervisor_actor(),
            WorkflowStartRequest {
                workflow_id: workflow_id.clone(),
                mode: PLAN_CONSENSUS_MODE.to_string(),
                owner: cmd.owner.clone(),
                lane,
                title: Some(format!("plan consensus: {}", cmd.task)),
                idempotency_key: Some(format!("plan-consensus:{plan_id}")),
            },
        )
        .await
        .map_err(|err| err.to_string())?;
    coordinator
        .record_workflow_evidence(
            supervisor_actor(),
            WorkflowEvidenceRequest {
                workflow_id: workflow_id.clone(),
                category: PLAN_CONSENSUS_EVIDENCE_CATEGORY.to_string(),
                summary: format!(
                    "plan consensus `{plan_id}` verdict={}",
                    artifact.critic_verdict
                ),
                artifact_path: Some(artifact_path.clone()),
                artifact_digest: Some(artifact_digest.clone()),
                acceptance_ref: Some(plan_id.clone()),
                metadata,
            },
        )
        .await
        .map_err(|err| err.to_string())?;
    coordinator
        .stop_run()
        .await
        .map_err(|err| err.to_string())?;

    let report = PlanConsensusReport {
        run_id: run.run_id,
        run_dir: run.run_dir.display().to_string(),
        events_path: run.events_path.display().to_string(),
        workflow_id,
        plan_id,
        status: artifact.critic_verdict.clone(),
        critic_verdict: artifact.critic_verdict,
        critic_iterations: artifact.critic_iterations,
        max_iterations: artifact.max_iterations,
        lanes: artifact.lanes,
        artifact_path,
        artifact_digest,
        artifact_bytes,
    };
    if cmd.json {
        print_json(&report, "workflow plan consensus JSON")?;
    } else {
        println!(
            "workflow plan consensus written: {} verdict={} artifact={} run={}",
            report.plan_id, report.critic_verdict, report.artifact_path, report.run_dir
        );
    }
    Ok(())
}

async fn execute_goal_create(
    cmd: WorkflowGoalCreateCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    let context = resolve_workflow_context(config_path, global_session_dir)?;
    fs::create_dir_all(&context.session_dir).map_err(|err| {
        format!(
            "failed to create session dir {}: {err}",
            context.session_dir.display()
        )
    })?;
    let mut coordinator_config = CoordinatorConfig::new(context.session_dir);
    coordinator_config.session_mode_source = Some(SessionModeSource::Prompt);
    let clock: Arc<dyn Clock + Send + Sync> = Arc::new(RealClock::new());
    let coordinator = spawn_coordinator(
        coordinator_config,
        Arc::clone(&clock),
        Arc::new(DefaultRedactor::default()),
    );
    let workspace = std::env::current_dir()
        .map_err(|err| format!("failed to resolve current working directory: {err}"))?;
    let run = coordinator
        .start_run("workflow goal create".to_string(), workspace)
        .await
        .map_err(|err| err.to_string())?;
    let workflow_id = cmd
        .workflow_id
        .clone()
        .unwrap_or_else(|| format!("wf_goal_{}", run.run_id));
    let lane = cmd
        .lane
        .clone()
        .or_else(|| Some(context.workflow.run.default_lane.clone()));
    let stories = parse_goal_stories(
        &cmd.stories,
        cmd.owner_workflow_id.as_deref(),
        &cmd.acceptance,
    )?;
    let artifact = GoalLedgerArtifact {
        schema_version: GOAL_LEDGER_SCHEMA_VERSION,
        workflow_id: workflow_id.clone(),
        goal_id: cmd.goal_id.clone(),
        objective: cmd.objective.clone(),
        status: "active".to_string(),
        stories,
        evidence_refs: cmd.evidence_refs.clone(),
        quality_gate: None,
    };
    validate_goal_ledger_artifact(&artifact, context.workflow.goal.require_final_quality_gate)
        .map_err(|errors| format!("invalid goal ledger artifact: {}", errors.join("; ")))?;
    let (artifact_path, artifact_digest, artifact_bytes) =
        write_json_artifact(&run, &goal_ledger_artifact_name(&cmd.goal_id), &artifact)?;
    let metadata = goal_ledger_metadata(&artifact);
    coordinator
        .start_workflow(
            supervisor_actor(),
            WorkflowStartRequest {
                workflow_id: workflow_id.clone(),
                mode: GOAL_LEDGER_MODE.to_string(),
                owner: cmd.owner.clone(),
                lane,
                title: Some(format!("goal ledger: {}", cmd.objective)),
                idempotency_key: Some(format!("goal-ledger:{}", cmd.goal_id)),
            },
        )
        .await
        .map_err(|err| err.to_string())?;
    coordinator
        .record_workflow_evidence(
            supervisor_actor(),
            WorkflowEvidenceRequest {
                workflow_id: workflow_id.clone(),
                category: GOAL_LEDGER_EVIDENCE_CATEGORY.to_string(),
                summary: format!("goal ledger `{}` created", cmd.goal_id),
                artifact_path: Some(artifact_path.clone()),
                artifact_digest: Some(artifact_digest.clone()),
                acceptance_ref: Some(cmd.goal_id.clone()),
                metadata,
            },
        )
        .await
        .map_err(|err| err.to_string())?;
    coordinator
        .stop_run()
        .await
        .map_err(|err| err.to_string())?;

    let story_count = artifact.stories.len();
    let report = GoalMutationReport {
        run_id: run.run_id,
        run_dir: run.run_dir.display().to_string(),
        events_path: run.events_path.display().to_string(),
        workflow_id,
        goal_id: cmd.goal_id,
        status: artifact.status.clone(),
        artifact_path,
        artifact_digest,
        artifact_bytes,
    };
    if cmd.json {
        print_json(&report, "workflow goal create JSON")?;
    } else {
        println!(
            "workflow goal created: {} stories={} artifact={} run={}",
            report.goal_id, story_count, report.artifact_path, report.run_dir
        );
    }
    Ok(())
}

async fn execute_goal_checkpoint(
    cmd: WorkflowGoalCheckpointCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    let context = resolve_workflow_context(config_path, global_session_dir)?;
    fs::create_dir_all(&context.session_dir).map_err(|err| {
        format!(
            "failed to create session dir {}: {err}",
            context.session_dir.display()
        )
    })?;
    let mut coordinator_config = CoordinatorConfig::new(context.session_dir);
    coordinator_config.session_mode_source = Some(SessionModeSource::Prompt);
    let clock: Arc<dyn Clock + Send + Sync> = Arc::new(RealClock::new());
    let coordinator = spawn_coordinator(
        coordinator_config,
        Arc::clone(&clock),
        Arc::new(DefaultRedactor::default()),
    );
    let workspace = std::env::current_dir()
        .map_err(|err| format!("failed to resolve current working directory: {err}"))?;
    let run = coordinator
        .start_run("workflow goal checkpoint".to_string(), workspace)
        .await
        .map_err(|err| err.to_string())?;
    let workflow_id = cmd
        .workflow_id
        .clone()
        .unwrap_or_else(|| format!("wf_goal_{}", run.run_id));
    let lane = cmd
        .lane
        .clone()
        .or_else(|| Some(context.workflow.run.default_lane.clone()));
    let quality_gate = build_quality_gate(&cmd);
    let checkpoint = GoalCheckpointArtifact {
        schema_version: GOAL_LEDGER_SCHEMA_VERSION,
        workflow_id: workflow_id.clone(),
        goal_id: cmd.goal_id.clone(),
        story_id: cmd.story_id.clone(),
        status: cmd.status.as_str().to_string(),
        summary: cmd.summary.clone(),
        evidence_refs: cmd.evidence_refs.clone(),
        final_checkpoint: cmd.final_goal,
        quality_gate,
    };
    validate_goal_checkpoint_artifact(
        &checkpoint,
        context.workflow.goal.require_final_quality_gate,
    )
    .map_err(|errors| format!("invalid goal checkpoint artifact: {}", errors.join("; ")))?;
    let (artifact_path, artifact_digest, artifact_bytes) = write_json_artifact(
        &run,
        &goal_checkpoint_artifact_name(&cmd.goal_id, cmd.story_id.as_deref()),
        &checkpoint,
    )?;
    let metadata = goal_checkpoint_metadata(&checkpoint);
    coordinator
        .start_workflow(
            supervisor_actor(),
            WorkflowStartRequest {
                workflow_id: workflow_id.clone(),
                mode: GOAL_LEDGER_MODE.to_string(),
                owner: cmd.owner.clone(),
                lane,
                title: Some(format!("goal checkpoint: {}", cmd.goal_id)),
                idempotency_key: Some(format!(
                    "goal-checkpoint:{}:{}:{}",
                    cmd.goal_id,
                    cmd.story_id.as_deref().unwrap_or("goal"),
                    run.run_id
                )),
            },
        )
        .await
        .map_err(|err| err.to_string())?;
    coordinator
        .record_workflow_evidence(
            supervisor_actor(),
            WorkflowEvidenceRequest {
                workflow_id: workflow_id.clone(),
                category: GOAL_LEDGER_EVIDENCE_CATEGORY.to_string(),
                summary: cmd.summary.clone(),
                artifact_path: Some(artifact_path.clone()),
                artifact_digest: Some(artifact_digest.clone()),
                acceptance_ref: Some(cmd.goal_id.clone()),
                metadata,
            },
        )
        .await
        .map_err(|err| err.to_string())?;
    coordinator
        .stop_run()
        .await
        .map_err(|err| err.to_string())?;

    let report = GoalMutationReport {
        run_id: run.run_id,
        run_dir: run.run_dir.display().to_string(),
        events_path: run.events_path.display().to_string(),
        workflow_id,
        goal_id: cmd.goal_id,
        status: checkpoint.status,
        artifact_path,
        artifact_digest,
        artifact_bytes,
    };
    if cmd.json {
        print_json(&report, "workflow goal checkpoint JSON")?;
    } else {
        println!(
            "workflow goal checkpoint recorded: {} status={} artifact={} run={}",
            report.goal_id, report.status, report.artifact_path, report.run_dir
        );
    }
    Ok(())
}

fn execute_goal_status(
    cmd: WorkflowGoalStatusCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    let mut report = goal_status_report(cmd.target, config_path, global_session_dir)?;
    if let Some(goal_id) = cmd.goal_id.as_ref() {
        report
            .projection
            .goals
            .retain(|id, _| id.as_str() == goal_id.as_str());
    }
    if cmd.json {
        print_json(&report, "workflow goal status JSON")?;
    } else {
        for goal in report.projection.goals.values() {
            println!(
                "{} status={} stories={} ready={}",
                goal.goal_id,
                goal.status,
                goal.stories.len(),
                goal.ready_for_completion
            );
        }
    }
    Ok(())
}

fn execute_goal_read(
    cmd: WorkflowGoalReadCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    let report = goal_status_report(cmd.target, config_path, global_session_dir)?;
    let goal = report
        .projection
        .goals
        .get(&cmd.goal_id)
        .ok_or_else(|| format!("goal `{}` not found in projection", cmd.goal_id))?;
    let body = serde_json::to_string_pretty(goal)
        .map_err(|err| format!("failed to render goal JSON: {err}"))?;
    if let Some(output) = cmd.output.as_ref() {
        write_explicit_output(output, &body)?;
    }
    if cmd.json {
        println!("{body}");
    } else {
        println!(
            "workflow goal: {} status={} stories={} ready={}",
            goal.goal_id,
            goal.status,
            goal.stories.len(),
            goal.ready_for_completion
        );
    }
    Ok(())
}

async fn execute_mission_init(
    cmd: WorkflowMissionInitCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    let context = resolve_workflow_context(config_path, global_session_dir)?;
    fs::create_dir_all(&context.session_dir).map_err(|err| {
        format!(
            "failed to create session dir {}: {err}",
            context.session_dir.display()
        )
    })?;
    let mut coordinator_config = CoordinatorConfig::new(context.session_dir);
    coordinator_config.session_mode_source = Some(SessionModeSource::Prompt);
    let clock: Arc<dyn Clock + Send + Sync> = Arc::new(RealClock::new());
    let coordinator = spawn_coordinator(
        coordinator_config,
        Arc::clone(&clock),
        Arc::new(DefaultRedactor::default()),
    );
    let workspace = std::env::current_dir()
        .map_err(|err| format!("failed to resolve current working directory: {err}"))?;
    let run = coordinator
        .start_run("workflow research mission init".to_string(), workspace)
        .await
        .map_err(|err| err.to_string())?;
    let workflow_id = cmd
        .workflow_id
        .clone()
        .unwrap_or_else(|| format!("wf_research_{}", run.run_id));
    let lane = cmd
        .lane
        .clone()
        .or_else(|| Some(context.workflow.run.default_lane.clone()));
    let artifact = ResearchMissionArtifact {
        schema_version: RESEARCH_MISSION_SCHEMA_VERSION,
        workflow_id: workflow_id.clone(),
        mission_id: cmd.mission_id.clone(),
        objective: cmd.objective.clone(),
        question: cmd.question.clone(),
        validator_mode: cmd.validator_mode.to_core(),
        sandbox: ResearchSandboxArtifact {
            summary: cmd.sandbox.clone(),
            allowed_commands: cmd.allowed_commands.clone(),
            constraints: cmd.constraints.clone(),
        },
        evidence_refs: cmd.evidence_refs.clone(),
    };
    validate_research_mission_artifact(&artifact)
        .map_err(|errors| format!("invalid research mission artifact: {}", errors.join("; ")))?;
    let (artifact_path, artifact_digest, artifact_bytes) = write_json_artifact(
        &run,
        &research_mission_artifact_name(&cmd.mission_id),
        &artifact,
    )?;
    coordinator
        .start_workflow(
            supervisor_actor(),
            WorkflowStartRequest {
                workflow_id: workflow_id.clone(),
                mode: RESEARCH_MISSION_MODE.to_string(),
                owner: cmd.owner.clone(),
                lane,
                title: Some(format!("research mission: {}", cmd.objective)),
                idempotency_key: Some(format!("research-mission:{}", cmd.mission_id)),
            },
        )
        .await
        .map_err(|err| err.to_string())?;
    coordinator
        .record_workflow_evidence(
            supervisor_actor(),
            WorkflowEvidenceRequest {
                workflow_id: workflow_id.clone(),
                category: RESEARCH_MISSION_EVIDENCE_CATEGORY.to_string(),
                summary: format!("research mission `{}` initialized", cmd.mission_id),
                artifact_path: Some(artifact_path.clone()),
                artifact_digest: Some(artifact_digest.clone()),
                acceptance_ref: Some(cmd.mission_id.clone()),
                metadata: research_mission_metadata(&artifact),
            },
        )
        .await
        .map_err(|err| err.to_string())?;
    coordinator
        .stop_run()
        .await
        .map_err(|err| err.to_string())?;
    let report = MissionMutationReport {
        run_id: run.run_id,
        run_dir: run.run_dir.display().to_string(),
        events_path: run.events_path.display().to_string(),
        workflow_id,
        mission_id: cmd.mission_id,
        status: "active".to_string(),
        artifact_path,
        artifact_digest,
        artifact_bytes,
    };
    if cmd.json {
        print_json(&report, "workflow mission init JSON")?;
    } else {
        println!(
            "workflow mission initialized: {} artifact={} run={}",
            report.mission_id, report.artifact_path, report.run_dir
        );
    }
    Ok(())
}

async fn execute_mission_run(
    cmd: WorkflowMissionRunCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    let context = resolve_workflow_context(config_path, global_session_dir)?;
    if let Some(command) = cmd.validator_command.as_ref() {
        if cmd.validator_mode.to_core() != ResearchValidatorMode::MissionValidatorScript {
            return Err(
                "--validator-command requires --validator-mode mission-validator-script"
                    .to_string(),
            );
        }
        if cmd.validator_result_ref.is_some() {
            return Err(
                "pass either --validator-command or --validator-result-ref, not both".to_string(),
            );
        }
        if let Err(denial) = ensure_workflow_permission_allowed(
            &context,
            PermissionKind::Shell,
            PermissionRuleRequest::ShellCommand(command.clone()),
            command.clone(),
            "workflow mission validator command",
        ) {
            let workflow_id = cmd
                .workflow_id
                .clone()
                .unwrap_or_else(|| format!("wf_research_{}", cmd.mission_id));
            let audit_run_dir = record_workflow_permission_denial(
                &context,
                workflow_id,
                RESEARCH_MISSION_MODE,
                &cmd.owner,
                cmd.lane.clone(),
                format!("research mission validator denied: {}", cmd.mission_id),
                &denial,
            )
            .await
            .ok();
            return Err(permission_denial_error(&denial, audit_run_dir));
        }
    }
    fs::create_dir_all(&context.session_dir).map_err(|err| {
        format!(
            "failed to create session dir {}: {err}",
            context.session_dir.display()
        )
    })?;
    let coordinator_config = coordinator_config_for_workflow(&context);
    let clock: Arc<dyn Clock + Send + Sync> = Arc::new(RealClock::new());
    let coordinator = spawn_coordinator(
        coordinator_config,
        Arc::clone(&clock),
        Arc::new(DefaultRedactor::default()),
    );
    let workspace = std::env::current_dir()
        .map_err(|err| format!("failed to resolve current working directory: {err}"))?;
    let run = coordinator
        .start_run("workflow research mission run".to_string(), workspace)
        .await
        .map_err(|err| err.to_string())?;
    let workflow_id = cmd
        .workflow_id
        .clone()
        .unwrap_or_else(|| format!("wf_research_{}", run.run_id));
    let lane = cmd
        .lane
        .clone()
        .or_else(|| Some(context.workflow.run.default_lane.clone()));
    let validator_mode = cmd.validator_mode.to_core();
    let mut validator_status = cmd.validator_status.clone();
    let mut validator_result_ref = cmd.validator_result_ref.clone();
    if let Some(command) = cmd.validator_command.as_ref() {
        if validator_mode != ResearchValidatorMode::MissionValidatorScript {
            return Err(
                "--validator-command requires --validator-mode mission-validator-script"
                    .to_string(),
            );
        }
        if cmd.validator_result_ref.is_some() {
            return Err(
                "pass either --validator-command or --validator-result-ref, not both".to_string(),
            );
        }
        let tool_result = coordinator
            .execute_agent_tool_call(
                supervisor_actor(),
                None,
                "bash",
                json!({
                    "command": command,
                    "description": format!("Run research mission validator `{}`", cmd.mission_id),
                }),
            )
            .await
            .map_err(|err| format!("validator command failed permissioned execution: {err}"))?;
        let command_passed = tool_result
            .structured_json
            .as_ref()
            .and_then(|value| value.get("success"))
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let validator_artifact = json!({
            "schema_version": RESEARCH_MISSION_SCHEMA_VERSION,
            "workflow_id": workflow_id.clone(),
            "mission_id": cmd.mission_id.clone(),
            "iteration": cmd.iteration,
            "command": command,
            "tool_result": tool_result,
        });
        let (validator_path, _, _) = write_json_artifact(
            &run,
            &research_validator_artifact_name(&cmd.mission_id, cmd.iteration),
            &validator_artifact,
        )?;
        validator_result_ref = Some(validator_path);
        validator_status = if command_passed {
            "passed".to_string()
        } else {
            "failed".to_string()
        };
    }
    let validator = ResearchValidatorArtifact {
        mode: validator_mode,
        status: validator_status,
        command: cmd
            .validator_command
            .clone()
            .map(|command| ResearchValidatorCommand {
                command,
                permission_kind: "bash".to_string(),
            }),
        result_ref: validator_result_ref,
        review_ref: cmd.review_ref.clone(),
    };
    let artifact = ResearchResultArtifact {
        schema_version: RESEARCH_MISSION_SCHEMA_VERSION,
        workflow_id: workflow_id.clone(),
        mission_id: cmd.mission_id.clone(),
        iteration: cmd.iteration,
        status: cmd.status.as_str().to_string(),
        summary: cmd.summary.clone(),
        candidate_ref: cmd.candidate_ref.clone(),
        validator,
        evidence_refs: cmd.evidence_refs.clone(),
    };
    validate_research_result_artifact(&artifact)
        .map_err(|errors| format!("invalid research result artifact: {}", errors.join("; ")))?;
    let (artifact_path, artifact_digest, artifact_bytes) = write_json_artifact(
        &run,
        &research_result_artifact_name(&cmd.mission_id, cmd.iteration),
        &artifact,
    )?;
    coordinator
        .start_workflow(
            supervisor_actor(),
            WorkflowStartRequest {
                workflow_id: workflow_id.clone(),
                mode: RESEARCH_MISSION_MODE.to_string(),
                owner: cmd.owner.clone(),
                lane,
                title: Some(format!("research mission result: {}", cmd.mission_id)),
                idempotency_key: Some(format!(
                    "research-result:{}:{}:{}",
                    cmd.mission_id, cmd.iteration, run.run_id
                )),
            },
        )
        .await
        .map_err(|err| err.to_string())?;
    coordinator
        .record_workflow_evidence(
            supervisor_actor(),
            WorkflowEvidenceRequest {
                workflow_id: workflow_id.clone(),
                category: RESEARCH_MISSION_EVIDENCE_CATEGORY.to_string(),
                summary: cmd.summary.clone(),
                artifact_path: Some(artifact_path.clone()),
                artifact_digest: Some(artifact_digest.clone()),
                acceptance_ref: Some(cmd.mission_id.clone()),
                metadata: research_result_metadata(&artifact),
            },
        )
        .await
        .map_err(|err| err.to_string())?;
    coordinator
        .stop_run()
        .await
        .map_err(|err| err.to_string())?;
    let report = MissionMutationReport {
        run_id: run.run_id,
        run_dir: run.run_dir.display().to_string(),
        events_path: run.events_path.display().to_string(),
        workflow_id,
        mission_id: cmd.mission_id,
        status: artifact.status,
        artifact_path,
        artifact_digest,
        artifact_bytes,
    };
    if cmd.json {
        print_json(&report, "workflow mission run JSON")?;
    } else {
        println!(
            "workflow mission result recorded: {} status={} artifact={} run={}",
            report.mission_id, report.status, report.artifact_path, report.run_dir
        );
    }
    Ok(())
}

fn execute_mission_status(
    cmd: WorkflowMissionStatusCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    let mut report = mission_status_report(cmd.target, config_path, global_session_dir)?;
    if let Some(mission_id) = cmd.mission_id.as_ref() {
        report
            .projection
            .missions
            .retain(|id, _| id.as_str() == mission_id.as_str());
    }
    if cmd.json {
        print_json(&report, "workflow mission status JSON")?;
    } else {
        for mission in report.projection.missions.values() {
            println!(
                "{} status={} iterations={} ready={}",
                mission.mission_id,
                mission.status,
                mission.iterations.len(),
                mission.ready_for_completion
            );
        }
    }
    Ok(())
}

fn execute_mission_read(
    cmd: WorkflowMissionReadCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    let report = mission_status_report(cmd.target, config_path, global_session_dir)?;
    let mission = report
        .projection
        .missions
        .get(&cmd.mission_id)
        .ok_or_else(|| format!("mission `{}` not found in projection", cmd.mission_id))?;
    let body = serde_json::to_string_pretty(mission)
        .map_err(|err| format!("failed to render mission JSON: {err}"))?;
    if let Some(output) = cmd.output.as_ref() {
        write_explicit_output(output, &body)?;
    }
    if cmd.json {
        println!("{body}");
    } else {
        println!(
            "workflow mission: {} status={} iterations={} ready={}",
            mission.mission_id,
            mission.status,
            mission.iterations.len(),
            mission.ready_for_completion
        );
    }
    Ok(())
}

async fn execute_wiki_add(
    cmd: WorkflowWikiAddCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    let context = resolve_workflow_context(config_path, global_session_dir)?;
    let root = resolve_wiki_root(&context.workflow)?;
    let page_path = wiki_page_path(&root, &cmd.slug)?;
    let policy_path = workflow_policy_path(&page_path)?;
    if let Err(denial) = ensure_workflow_permission_allowed(
        &context,
        PermissionKind::EditFs,
        PermissionRuleRequest::WorkspacePath(policy_path.clone()),
        policy_path,
        "workflow wiki add",
    ) {
        let workflow_id = cmd
            .workflow_id
            .clone()
            .unwrap_or_else(|| format!("wf_wiki_{}", cmd.slug));
        let audit_run_dir = record_workflow_permission_denial(
            &context,
            workflow_id,
            WIKI_MODE,
            &cmd.owner,
            cmd.lane.clone(),
            format!("wiki add denied: {}", cmd.slug),
            &denial,
        )
        .await
        .ok();
        return Err(permission_denial_error(&denial, audit_run_dir));
    }
    let body = read_wiki_body(cmd.body.as_ref(), cmd.body_file.as_ref())?;
    let contents = render_wiki_page(&cmd.title, &cmd.category, &cmd.tags, &body);
    let summary = wiki_summary(&cmd.slug, &page_path, &contents);
    let workflow_id = cmd
        .workflow_id
        .clone()
        .unwrap_or_else(|| format!("wf_wiki_{}", cmd.slug));
    let audit =
        start_wiki_mutation_audit(&context, workflow_id, cmd.owner, cmd.lane, "add", summary)
            .await?;
    if let Some(parent) = page_path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            let message = format!("failed to create wiki root {}: {err}", parent.display());
            let _ = abort_wiki_mutation_audit(audit, message.clone()).await;
            return Err(message);
        }
    }
    if let Err(err) = fs::write(&page_path, &contents) {
        let message = format!("failed to write wiki page {}: {err}", page_path.display());
        let _ = abort_wiki_mutation_audit(audit, message.clone()).await;
        return Err(message);
    }
    let report = finish_wiki_mutation_audit(audit).await?;
    if cmd.json {
        print_json(&report, "workflow wiki add JSON")?;
    } else {
        println!(
            "workflow wiki page added: {} digest={} run={}",
            report.page.slug, report.page.digest, report.run_dir
        );
    }
    Ok(())
}

async fn execute_wiki_delete(
    cmd: WorkflowWikiDeleteCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    let context = resolve_workflow_context(config_path, global_session_dir)?;
    let root = resolve_wiki_root(&context.workflow)?;
    let page_path = wiki_page_path(&root, &cmd.slug)?;
    let policy_path = workflow_policy_path(&page_path)?;
    if let Err(denial) = ensure_workflow_permission_allowed(
        &context,
        PermissionKind::EditFs,
        PermissionRuleRequest::WorkspacePath(policy_path.clone()),
        policy_path,
        "workflow wiki delete",
    ) {
        let workflow_id = cmd
            .workflow_id
            .clone()
            .unwrap_or_else(|| format!("wf_wiki_{}", cmd.slug));
        let audit_run_dir = record_workflow_permission_denial(
            &context,
            workflow_id,
            WIKI_MODE,
            &cmd.owner,
            cmd.lane.clone(),
            format!("wiki delete denied: {}", cmd.slug),
            &denial,
        )
        .await
        .ok();
        return Err(permission_denial_error(&denial, audit_run_dir));
    }
    let contents = fs::read_to_string(&page_path)
        .map_err(|err| format!("failed to read wiki page {}: {err}", page_path.display()))?;
    let mut summary = wiki_summary(&cmd.slug, &page_path, &contents);
    summary.digest = wiki_digest(&contents);
    let workflow_id = cmd
        .workflow_id
        .clone()
        .unwrap_or_else(|| format!("wf_wiki_{}", cmd.slug));
    let audit = start_wiki_mutation_audit(
        &context,
        workflow_id,
        cmd.owner,
        cmd.lane,
        "delete",
        summary,
    )
    .await?;
    if let Err(err) = fs::remove_file(&page_path) {
        let message = format!("failed to delete wiki page {}: {err}", page_path.display());
        let _ = abort_wiki_mutation_audit(audit, message.clone()).await;
        return Err(message);
    }
    let report = finish_wiki_mutation_audit(audit).await?;
    if cmd.json {
        print_json(&report, "workflow wiki delete JSON")?;
    } else {
        println!(
            "workflow wiki page deleted: {} previous_digest={} run={}",
            report.page.slug, report.page.digest, report.run_dir
        );
    }
    Ok(())
}

fn execute_wiki_read(
    cmd: WorkflowWikiReadCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    let context = resolve_workflow_context(config_path, global_session_dir)?;
    let root = resolve_wiki_root(&context.workflow)?;
    let page_path = wiki_page_path(&root, &cmd.slug)?;
    let contents = fs::read_to_string(&page_path)
        .map_err(|err| format!("failed to read wiki page {}: {err}", page_path.display()))?;
    let page = parse_wiki_page(&cmd.slug, &contents);
    if cmd.json {
        print_json(&page, "workflow wiki read JSON")?;
    } else {
        println!("{contents}");
    }
    Ok(())
}

fn execute_wiki_list(
    cmd: WorkflowWikiListCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    let context = resolve_workflow_context(config_path, global_session_dir)?;
    let root = resolve_wiki_root(&context.workflow)?;
    let pages = load_wiki_pages(&root)?;
    let report = WikiListReport {
        root: root.display().to_string(),
        pages: pages.into_iter().map(|(_, summary, _)| summary).collect(),
    };
    if cmd.json {
        print_json(&report, "workflow wiki list JSON")?;
    } else {
        for page in &report.pages {
            println!(
                "{} title={} category={} digest={}",
                page.slug, page.title, page.category, page.digest
            );
        }
    }
    Ok(())
}

fn execute_wiki_query(
    cmd: WorkflowWikiQueryCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    let context = resolve_workflow_context(config_path, global_session_dir)?;
    let root = resolve_wiki_root(&context.workflow)?;
    let matches = load_wiki_pages(&root)?
        .into_iter()
        .filter(|(_, _, page)| {
            wiki_matches(
                page,
                cmd.term.as_deref(),
                cmd.tag.as_deref(),
                cmd.category.as_deref(),
            )
        })
        .map(|(_, summary, _)| summary)
        .collect::<Vec<_>>();
    let report = WikiQueryReport {
        root: root.display().to_string(),
        matches,
    };
    if cmd.json {
        print_json(&report, "workflow wiki query JSON")?;
    } else {
        for page in &report.matches {
            println!(
                "{} title={} category={} digest={}",
                page.slug, page.title, page.category, page.digest
            );
        }
    }
    Ok(())
}

fn execute_wiki_lint(
    cmd: WorkflowWikiListCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    let context = resolve_workflow_context(config_path, global_session_dir)?;
    let root = resolve_wiki_root(&context.workflow)?;
    let findings = load_wiki_pages(&root)?
        .into_iter()
        .flat_map(|(_, _, page)| wiki_lint(&page))
        .collect::<Vec<_>>();
    let report = WikiLintReport {
        root: root.display().to_string(),
        findings,
    };
    if cmd.json {
        print_json(&report, "workflow wiki lint JSON")?;
    } else if report.findings.is_empty() {
        println!("workflow wiki lint: clean");
    } else {
        for finding in &report.findings {
            println!("{} {}: {}", finding.slug, finding.level, finding.message);
        }
    }
    Ok(())
}

async fn execute_init(
    cmd: WorkflowInitCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    let project_root = std::env::current_dir()
        .map_err(|err| format!("failed to resolve current working directory: {err}"))?;
    let files = workflow_init_files(&project_root);
    let context = if cmd.apply {
        Some(resolve_workflow_context(config_path, global_session_dir)?)
    } else {
        None
    };
    if let Some(context) = context.as_ref() {
        for (path, _) in &files {
            if path.exists() {
                continue;
            }
            let policy_path = workflow_policy_path(path)?;
            if let Err(denial) = ensure_workflow_permission_allowed(
                context,
                PermissionKind::EditFs,
                PermissionRuleRequest::WorkspacePath(policy_path.clone()),
                policy_path,
                "workflow init apply",
            ) {
                let audit_run_dir = record_workflow_permission_denial(
                    context,
                    "wf_workflow_init".to_string(),
                    "workflow.operator_utility",
                    "workflow-cli",
                    Some(context.workflow.run.default_lane.clone()),
                    "workflow init apply denied".to_string(),
                    &denial,
                )
                .await
                .ok();
                return Err(permission_denial_error(&denial, audit_run_dir));
            }
        }
    }
    let mut report_files = Vec::new();
    for (path, body) in files {
        let exists = path.exists();
        let action = if exists {
            "exists".to_string()
        } else if cmd.apply {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|err| {
                    format!(
                        "failed to create workflow init dir {}: {err}",
                        parent.display()
                    )
                })?;
            }
            fs::write(&path, body).map_err(|err| {
                format!(
                    "failed to write workflow init file {}: {err}",
                    path.display()
                )
            })?;
            "created".to_string()
        } else {
            "would_create".to_string()
        };
        report_files.push(InitFileReport {
            path: path.display().to_string(),
            exists,
            action,
        });
    }

    let report = InitReport {
        mode: if cmd.apply { "apply" } else { "check" }.to_string(),
        applied: cmd.apply,
        files: report_files,
    };
    if cmd.json {
        print_json(&report, "workflow init JSON")?;
    } else {
        println!(
            "workflow init {}: {} file(s)",
            report.mode,
            report.files.len()
        );
        for file in &report.files {
            println!("- {}: {}", file.action, file.path);
        }
    }
    Ok(())
}

fn status_report(
    target: WorkflowReadTargetArgs,
    workflow_id: Option<String>,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<WorkflowStatusReport, String> {
    let run_dir = resolve_read_run_dir(target, config_path.clone(), global_session_dir.clone())?;
    let context = resolve_workflow_context(config_path, global_session_dir)?;
    let closeout_policy = effective_closeout_policy(&context, None)?;
    let signoff_policy = WorkflowSignoffPolicy::simulator_default();
    let events_path = run_dir.join(EVENTS_FILE_NAME);
    let events = load_events_from_run_dir(&run_dir)?;
    let mut projection = project_workflows(events.iter().map(|event| &event.payload));
    let mut persistent_tasks = project_persistent_tasks(&events);
    if let Some(workflow_id) = workflow_id {
        projection
            .workflows
            .retain(|id, _| id.as_str() == workflow_id.as_str());
        projection.evidence.retain(|id, _| id == &workflow_id);
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
    let run_id = run_id_from_run_dir(&run_dir);
    let closeout = projection
        .workflows
        .keys()
        .map(|workflow_id| {
            let readiness = closeout_readiness_for_report(
                &projection,
                &persistent_tasks,
                &signoff_policy,
                &closeout_policy,
                workflow_id,
                run_id.clone(),
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
    Ok(WorkflowStatusReport {
        run_dir: run_dir.display().to_string(),
        events_path: events_path.display().to_string(),
        workflow_count,
        active_count,
        projection,
        persistent_tasks,
        closeout,
    })
}

fn effective_closeout_policy(
    context: &WorkflowRuntimeContext,
    policy_id: Option<&str>,
) -> Result<WorkflowCloseoutPolicy, String> {
    let policy_id = policy_id.unwrap_or(&context.workflow.closeout.default_policy);
    context
        .workflow
        .effective_closeout_policy(policy_id)
        .map_err(|err| format!("invalid workflow closeout policy `{policy_id}`: {err:?}"))
}

fn closeout_readiness_for_report(
    projection: &WorkflowProjection,
    persistent_tasks: &PersistentTaskProjection,
    signoff_policy: &WorkflowSignoffPolicy,
    closeout_policy: &WorkflowCloseoutPolicy,
    workflow_id: &str,
    run_id: Option<String>,
) -> WorkflowCloseoutReadiness {
    let mut readiness = projection.closeout_readiness(
        workflow_id.to_string(),
        persistent_tasks,
        signoff_policy,
        closeout_policy,
    );
    readiness.run_id = run_id;
    readiness
}

fn run_id_from_run_dir(run_dir: &Path) -> Option<String> {
    run_dir
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
}

fn goal_status_report(
    target: WorkflowReadTargetArgs,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<GoalStatusReport, String> {
    let run_dir = resolve_read_run_dir(target, config_path, global_session_dir)?;
    let events_path = run_dir.join(EVENTS_FILE_NAME);
    let events = load_events_from_run_dir(&run_dir)?;
    let projection = project_goal_ledger(events.iter().map(|event| &event.payload));
    Ok(GoalStatusReport {
        run_dir: run_dir.display().to_string(),
        events_path: events_path.display().to_string(),
        projection,
    })
}

fn mission_status_report(
    target: WorkflowReadTargetArgs,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<MissionStatusReport, String> {
    let run_dir = resolve_read_run_dir(target, config_path, global_session_dir)?;
    let events_path = run_dir.join(EVENTS_FILE_NAME);
    let events = load_events_from_run_dir(&run_dir)?;
    let projection = project_research_missions(events.iter().map(|event| &event.payload));
    Ok(MissionStatusReport {
        run_dir: run_dir.display().to_string(),
        events_path: events_path.display().to_string(),
        projection,
    })
}

fn parse_plan_options(raw_options: &[String]) -> Result<Vec<PlanConsensusOption>, String> {
    raw_options
        .iter()
        .map(|raw| {
            let (id, summary) = split_key_value(raw, "plan option")?;
            Ok(PlanConsensusOption {
                id,
                summary,
                pros: Vec::new(),
                cons: Vec::new(),
            })
        })
        .collect()
}

fn parse_goal_stories(
    raw_stories: &[String],
    owner_workflow_id: Option<&str>,
    acceptance: &[String],
) -> Result<Vec<GoalStoryArtifact>, String> {
    raw_stories
        .iter()
        .map(|raw| {
            let (story_id, objective) = split_key_value(raw, "goal story")?;
            Ok(GoalStoryArtifact {
                story_id,
                objective,
                status: "pending".to_string(),
                owner_workflow_id: owner_workflow_id.map(str::to_string),
                acceptance: acceptance.to_vec(),
                evidence_refs: Vec::new(),
            })
        })
        .collect()
}

fn split_key_value(raw: &str, label: &str) -> Result<(String, String), String> {
    let (key, value) = raw
        .split_once('=')
        .ok_or_else(|| format!("{label} `{raw}` must use id=value syntax"))?;
    let key = key.trim();
    let value = value.trim();
    if key.is_empty() || value.is_empty() {
        return Err(format!("{label} `{raw}` must have non-empty id and value"));
    }
    Ok((key.to_string(), value.to_string()))
}

fn parse_metadata_pairs(raw_metadata: &[String]) -> Result<BTreeMap<String, String>, String> {
    let mut metadata = BTreeMap::new();
    for raw in raw_metadata {
        let (key, value) = split_key_value(raw, "workflow evidence metadata")?;
        metadata.insert(key, value);
    }
    Ok(metadata)
}

fn build_quality_gate(cmd: &WorkflowGoalCheckpointCommand) -> Option<GoalQualityGate> {
    if !cmd.final_goal
        && cmd.verification_refs.is_empty()
        && cmd.review_refs.is_empty()
        && cmd.cleanup_refs.is_empty()
        && cmd.quality_gate_refs.is_empty()
    {
        return None;
    }
    Some(GoalQualityGate {
        status: "passed".to_string(),
        verification_refs: cmd.verification_refs.clone(),
        review_refs: cmd.review_refs.clone(),
        cleanup_refs: cmd.cleanup_refs.clone(),
        evidence_refs: cmd.quality_gate_refs.clone(),
    })
}

fn write_json_artifact<T: Serialize>(
    run: &RunInfo,
    name: &str,
    value: &T,
) -> Result<(String, String, u64), String> {
    let body = serde_json::to_string_pretty(value)
        .map_err(|err| format!("failed to render workflow artifact JSON: {err}"))?;
    let store = ArtifactStore::new(run.artifacts_dir.clone())
        .map_err(|err| format!("failed to open workflow artifact store: {err}"))?;
    let artifact = store
        .write_text(name, &body)
        .map_err(|err| format!("failed to write workflow artifact: {err}"))?;
    let digest = artifact.digest.unwrap_or_default();
    Ok((artifact.path, digest, body.len() as u64))
}

fn resolve_wiki_root(workflow: &WorkflowRuntimeConfig) -> Result<PathBuf, String> {
    let root = if workflow.wiki.root.is_absolute() {
        workflow.wiki.root.clone()
    } else {
        std::env::current_dir()
            .map_err(|err| format!("failed to resolve current working directory: {err}"))?
            .join(&workflow.wiki.root)
    };
    Ok(root)
}

fn read_wiki_body(body: Option<&String>, body_file: Option<&PathBuf>) -> Result<String, String> {
    match (body, body_file) {
        (Some(_), Some(_)) => Err("pass either --body or --body-file, not both".to_string()),
        (Some(body), None) => Ok(body.clone()),
        (None, Some(path)) => fs::read_to_string(path)
            .map_err(|err| format!("failed to read wiki body file {}: {err}", path.display())),
        (None, None) => Err("wiki add requires --body or --body-file".to_string()),
    }
}

fn load_wiki_pages(root: &Path) -> Result<Vec<(PathBuf, WikiPageSummary, WikiPage)>, String> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut pages = Vec::new();
    for entry in fs::read_dir(root)
        .map_err(|err| format!("failed to read wiki root {}: {err}", root.display()))?
    {
        let entry = entry.map_err(|err| format!("failed to read wiki entry: {err}"))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let Some(slug) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let contents = fs::read_to_string(&path)
            .map_err(|err| format!("failed to read wiki page {}: {err}", path.display()))?;
        let summary = wiki_summary(slug, &path, &contents);
        let page = parse_wiki_page(slug, &contents);
        pages.push((path, summary, page));
    }
    pages.sort_by(|left, right| left.1.slug.cmp(&right.1.slug));
    Ok(pages)
}

async fn start_wiki_mutation_audit(
    context: &WorkflowRuntimeContext,
    workflow_id: String,
    owner: String,
    lane: Option<String>,
    action: &str,
    page: WikiPageSummary,
) -> Result<WikiMutationAudit, String> {
    fs::create_dir_all(&context.session_dir).map_err(|err| {
        format!(
            "failed to create session dir {}: {err}",
            context.session_dir.display()
        )
    })?;
    let coordinator = workflow_mutation_coordinator(context, context.session_dir.clone());
    let workspace = std::env::current_dir()
        .map_err(|err| format!("failed to resolve current working directory: {err}"))?;
    let run = coordinator
        .start_run(format!("workflow wiki {action}"), workspace)
        .await
        .map_err(|err| err.to_string())?;
    let lane = lane.or_else(|| Some(context.workflow.run.default_lane.clone()));
    coordinator
        .start_workflow(
            supervisor_actor(),
            WorkflowStartRequest {
                workflow_id: workflow_id.clone(),
                mode: WIKI_MODE.to_string(),
                owner: owner.clone(),
                lane,
                title: Some(format!("wiki {action}: {}", page.slug)),
                idempotency_key: Some(format!("wiki:{action}:{}:{}", page.slug, run.run_id)),
            },
        )
        .await
        .map_err(|err| err.to_string())?;
    coordinator
        .record_workflow_operator_decision(
            supervisor_actor(),
            workflow_id.clone(),
            format!("wiki-{action}-intent"),
            owner,
            Some(format!(
                "wiki {action} intent recorded before project-visible mutation for {}",
                page.slug
            )),
            None,
        )
        .await
        .map_err(|err| err.to_string())?;
    Ok(WikiMutationAudit {
        coordinator,
        run,
        workflow_id,
        action: action.to_string(),
        page,
    })
}

async fn finish_wiki_mutation_audit(
    audit: WikiMutationAudit,
) -> Result<WikiMutationReport, String> {
    let WikiMutationAudit {
        coordinator,
        run,
        workflow_id,
        action,
        page,
    } = audit;
    coordinator
        .record_workflow_evidence(
            supervisor_actor(),
            WorkflowEvidenceRequest {
                workflow_id: workflow_id.clone(),
                category: WIKI_EVIDENCE_CATEGORY.to_string(),
                summary: format!("wiki {}: {} digest={}", action, page.slug, page.digest),
                artifact_path: None,
                artifact_digest: Some(page.digest.clone()),
                acceptance_ref: Some(page.slug.clone()),
                metadata: wiki_evidence_metadata(&action, &page),
            },
        )
        .await
        .map_err(|err| err.to_string())?;
    coordinator
        .stop_run()
        .await
        .map_err(|err| err.to_string())?;
    Ok(WikiMutationReport {
        run_id: run.run_id,
        run_dir: run.run_dir.display().to_string(),
        events_path: run.events_path.display().to_string(),
        workflow_id,
        action,
        page,
    })
}

async fn abort_wiki_mutation_audit(audit: WikiMutationAudit, reason: String) -> Result<(), String> {
    let WikiMutationAudit {
        coordinator,
        workflow_id,
        action,
        page,
        ..
    } = audit;
    coordinator
        .record_workflow_operator_decision(
            supervisor_actor(),
            workflow_id,
            format!("wiki-{action}-failed"),
            "workflow-cli".to_string(),
            Some(format!(
                "wiki {action} failed before durable evidence for {}: {reason}",
                page.slug
            )),
            None,
        )
        .await
        .map_err(|err| err.to_string())?;
    coordinator.stop_run().await.map_err(|err| err.to_string())
}

fn snapshot_input_from_command(cmd: &WorkflowSnapshotWriteCommand) -> ContextSnapshotInput {
    let mut input = ContextSnapshotInput::new(
        cmd.source_command.clone(),
        cmd.task.clone(),
        cmd.desired_outcome.clone(),
    );
    input.probable_intent = cmd.probable_intent.clone();
    input.constraints = cmd.constraints.clone();
    input.unknowns = cmd.unknowns.clone();
    input.likely_touchpoints = cmd.likely_touchpoints.clone();
    input.ambiguity = ContextSnapshotAmbiguity {
        score: cmd.ambiguity_score,
        threshold: cmd.ambiguity_threshold,
    };
    input.handoff_ready = cmd.handoff_ready;
    input
}

fn print_snapshot_write_report(
    cmd: &WorkflowSnapshotWriteCommand,
    run: &RunInfo,
    snapshot: ContextSnapshotWriteResult,
) -> Result<(), String> {
    let report = SnapshotWriteReport {
        run_id: run.run_id.clone(),
        run_dir: run.run_dir.display().to_string(),
        events_path: run.events_path.display().to_string(),
        snapshot,
    };

    if cmd.json {
        print_json(&report, "workflow snapshot JSON")?;
    } else {
        println!(
            "workflow snapshot written: {} ambiguity={:.3} artifact={} run={}",
            report.snapshot.slug,
            report.snapshot.ambiguity_score,
            report.snapshot.artifact_path,
            report.run_dir
        );
    }
    Ok(())
}

#[derive(Debug)]
struct WorkflowRuntimeContext {
    session_dir: PathBuf,
    workflow: WorkflowRuntimeConfig,
    agent_catalog: Option<AgentCatalog>,
    permission_policy: PermissionPolicy,
    shell_allowlist: ShellAllowlist,
}

#[derive(Debug, Clone)]
struct WorkflowPermissionDenial {
    kind: PermissionKind,
    public_kind: &'static str,
    selector: String,
    action: String,
    decision: &'static str,
    reason: String,
}

fn resolve_workflow_context(
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<WorkflowRuntimeContext, String> {
    let loaded = load_resolved_config(config_path.as_deref()).map_err(|err| err.to_string())?;
    if let Some(loaded) = loaded {
        let mut config = loaded.config;
        config.apply_session_dir_override(global_session_dir);
        let agent_catalog = resolve_agent_catalog(&config);
        let permission_policy = PermissionPolicy::from_config(&config);
        let shell_allowlist = config.permissions.shell_allowlist.clone();
        return Ok(WorkflowRuntimeContext {
            session_dir: config.paths.session_dir,
            workflow: config.runtime.workflow,
            agent_catalog: Some(agent_catalog),
            permission_policy,
            shell_allowlist,
        });
    }

    Ok(WorkflowRuntimeContext {
        session_dir: global_session_dir.unwrap_or_else(|| PathBuf::from(DEFAULT_SESSION_DIR)),
        workflow: WorkflowRuntimeConfig::default(),
        agent_catalog: None,
        permission_policy: PermissionPolicy::default(),
        shell_allowlist: ShellAllowlist::default(),
    })
}

fn coordinator_config_for_workflow(context: &WorkflowRuntimeContext) -> CoordinatorConfig {
    let mut coordinator_config = CoordinatorConfig::new(context.session_dir.clone());
    coordinator_config.session_mode_source = Some(SessionModeSource::Prompt);
    coordinator_config.permission_policy = context.permission_policy.clone();
    coordinator_config.tool_registry = Arc::new(harness_tools::coordinator_registry(
        context.shell_allowlist.clone(),
    ));
    coordinator_config
}

fn workflow_mutation_coordinator(
    context: &WorkflowRuntimeContext,
    session_dir: PathBuf,
) -> harness_core::coord::CoordinatorHandle {
    let mut coordinator_config = coordinator_config_for_workflow(context);
    coordinator_config.session_dir = session_dir;
    let clock: Arc<dyn Clock + Send + Sync> = Arc::new(RealClock::new());
    spawn_coordinator(
        coordinator_config,
        Arc::clone(&clock),
        Arc::new(DefaultRedactor::default()),
    )
}

fn ensure_workflow_permission_allowed(
    context: &WorkflowRuntimeContext,
    kind: PermissionKind,
    selector: PermissionRuleRequest,
    selector_label: String,
    action: &str,
) -> Result<(), WorkflowPermissionDenial> {
    let decision = context
        .permission_policy
        .evaluate_request(None, kind, Some(&selector));
    if matches!(decision, PolicyDecision::Allow) {
        return Ok(());
    }

    let decision_label = policy_decision_label(decision);
    let public_kind = public_permission_kind(kind);
    let reason = format!(
        "{action} requires `{public_kind}` permission for `{selector_label}`, but policy resolved to {decision_label}"
    );
    Err(WorkflowPermissionDenial {
        kind,
        public_kind,
        selector: selector_label,
        action: action.to_string(),
        decision: decision_label,
        reason,
    })
}

fn public_permission_kind(kind: PermissionKind) -> &'static str {
    match kind {
        PermissionKind::EditFs => "edit",
        PermissionKind::Shell => "bash",
        PermissionKind::Network => "network",
        PermissionKind::Question => "question",
        PermissionKind::Task => "task",
        PermissionKind::WebFetch => "webfetch",
        PermissionKind::WebSearch => "websearch",
        PermissionKind::CodeSearch => "codesearch",
        PermissionKind::Lsp => "lsp",
    }
}

fn policy_decision_label(decision: PolicyDecision) -> &'static str {
    match decision {
        PolicyDecision::Allow => "allow",
        PolicyDecision::Deny => "deny",
        PolicyDecision::Ask { .. } => "ask(default=deny)",
    }
}

fn workflow_policy_path(path: &Path) -> Result<String, String> {
    let cwd = std::env::current_dir()
        .map_err(|err| format!("failed to resolve current working directory: {err}"))?;
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let scoped = absolute.strip_prefix(&cwd).unwrap_or(absolute.as_path());
    Ok(scoped.to_string_lossy().replace('\\', "/"))
}

async fn record_workflow_permission_denial(
    context: &WorkflowRuntimeContext,
    workflow_id: String,
    mode: &str,
    owner: &str,
    lane: Option<String>,
    title: String,
    denial: &WorkflowPermissionDenial,
) -> Result<String, String> {
    fs::create_dir_all(&context.session_dir).map_err(|err| {
        format!(
            "failed to create session dir {} for permission denial audit: {err}",
            context.session_dir.display()
        )
    })?;
    let coordinator = workflow_mutation_coordinator(context, context.session_dir.clone());
    let workspace = std::env::current_dir()
        .map_err(|err| format!("failed to resolve current working directory: {err}"))?;
    let run = coordinator
        .start_run("workflow permission denial".to_string(), workspace)
        .await
        .map_err(|err| err.to_string())?;
    let effective_lane = lane.or_else(|| Some(context.workflow.run.default_lane.clone()));
    coordinator
        .start_workflow(
            supervisor_actor(),
            WorkflowStartRequest {
                workflow_id: workflow_id.clone(),
                mode: mode.to_string(),
                owner: owner.to_string(),
                lane: effective_lane,
                title: Some(title),
                idempotency_key: Some(format!(
                    "permission-denial:{}:{}:{}",
                    denial.public_kind, workflow_id, run.run_id
                )),
            },
        )
        .await
        .map_err(|err| err.to_string())?;
    coordinator
        .record_workflow_operator_decision(
            supervisor_actor(),
            workflow_id.clone(),
            format!("permission-denied:{}", denial.public_kind),
            owner.to_string(),
            Some(denial.reason.clone()),
            None,
        )
        .await
        .map_err(|err| err.to_string())?;
    coordinator
        .record_workflow_evidence(
            supervisor_actor(),
            WorkflowEvidenceRequest {
                workflow_id,
                category: PERMISSION_DECISION_EVIDENCE_CATEGORY.to_string(),
                summary: denial.reason.clone(),
                artifact_path: None,
                artifact_digest: None,
                acceptance_ref: Some(denial.selector.clone()),
                metadata: BTreeMap::from([
                    ("action".to_string(), denial.action.clone()),
                    ("decision".to_string(), denial.decision.to_string()),
                    (
                        "permission_kind".to_string(),
                        denial.public_kind.to_string(),
                    ),
                    (
                        "permission_kind_internal".to_string(),
                        denial.kind.as_str().to_string(),
                    ),
                    ("selector".to_string(), denial.selector.clone()),
                    ("status".to_string(), PERMISSION_DENIED_STATUS.to_string()),
                ]),
            },
        )
        .await
        .map_err(|err| err.to_string())?;
    coordinator
        .stop_run()
        .await
        .map_err(|err| err.to_string())?;
    Ok(run.run_dir.display().to_string())
}

fn permission_denial_error(
    denial: &WorkflowPermissionDenial,
    audit_run_dir: Option<String>,
) -> String {
    match audit_run_dir {
        Some(run_dir) => format!("{}; denial recorded in {}", denial.reason, run_dir),
        None => denial.reason.clone(),
    }
}

fn session_dir_and_run_id_from_run_dir(run_dir: &Path) -> Result<(PathBuf, String), String> {
    let run_id = run_dir
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| format!("run dir {} has no run id file name", run_dir.display()))?
        .to_string();
    let session_dir = run_dir.parent().ok_or_else(|| {
        format!(
            "run dir {} has no parent session directory",
            run_dir.display()
        )
    })?;
    Ok((session_dir.to_path_buf(), run_id))
}

fn resolve_read_run_dir(
    target: WorkflowReadTargetArgs,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<PathBuf, String> {
    if let Some(run_dir) = target.run_dir {
        return Ok(run_dir);
    }
    let context = resolve_workflow_context(config_path, global_session_dir)?;
    if let Some(run_id) = target.run_id {
        return Ok(context.session_dir.join(run_id));
    }
    latest_run_dir(&context.session_dir)
}

fn latest_run_dir(session_dir: &Path) -> Result<PathBuf, String> {
    let entries = fs::read_dir(session_dir).map_err(|err| {
        format!(
            "failed to read session dir {} to find latest workflow run: {err}",
            session_dir.display()
        )
    })?;
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| format!("failed to read session dir entry: {err}"))?;
        let path = entry.path();
        if !path.join(EVENTS_FILE_NAME).is_file() {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok();
        candidates.push((modified, path));
    }
    candidates.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    candidates.pop().map(|(_, path)| path).ok_or_else(|| {
        format!(
            "no workflow run found under {}; pass --run-dir or --run-id",
            session_dir.display()
        )
    })
}

fn render_dossier_markdown(report: &WorkflowStatusReport, dossier: &RunDossier) -> String {
    let mut body = format!(
        "# Workflow Run Dossier\n\n- Run dir: `{}`\n- Events: `{}`\n- Workflows: {}\n- Active: {}\n\n",
        report.run_dir, report.events_path, report.workflow_count, report.active_count
    );
    for workflow in &dossier.workflows {
        body.push_str(&format!(
            "## `{}`\n\n- Mode: `{}`\n- Status: `{}`\n- Owner: `{}`\n- Terminal: `{}`\n- Signoff allowed: `{}`\n- Closeout policy: `{}` v{}\n- Closeout allowed: `{}`\n- Dossier export stale: `{}`\n\n",
            workflow.workflow_id,
            workflow.mode,
            workflow.status,
            workflow.owner,
            workflow.terminal,
            workflow.signoff.allowed,
            workflow.closeout.policy_id,
            workflow.closeout.policy_version,
            workflow.closeout.overall_allowed,
            workflow.closeout.stale_export
        ));
        if !workflow.closeout.matrix.is_empty() {
            body.push_str("Closeout matrix:\n");
            for dimension in &workflow.closeout.matrix {
                body.push_str(&format!(
                    "- `{}` allowed={} waived={} blockers={}\n",
                    dimension.id,
                    dimension.allowed,
                    dimension.waived,
                    if dimension.blocking_refs.is_empty() {
                        "none".to_string()
                    } else {
                        dimension.blocking_refs.join(", ")
                    }
                ));
            }
            body.push('\n');
        }
        body.push_str(&format!(
            "- Quality gate passed: `{}`\n",
            workflow.quality_gate.passed
        ));
        if !workflow.quality_gate.missing.is_empty() {
            body.push_str("Quality gate gaps:\n");
            for gate in &workflow.quality_gate.missing {
                body.push_str(&format!("- `{gate}`\n"));
            }
            body.push('\n');
        }
        if !workflow.quality_gate.recovery_hints.is_empty() {
            body.push_str("Recovery hints:\n");
            for hint in &workflow.quality_gate.recovery_hints {
                body.push_str(&format!("- {hint}\n"));
            }
            body.push('\n');
        }
        if !workflow.evidence.is_empty() {
            body.push_str("Evidence:\n");
            for evidence in &workflow.evidence {
                body.push_str(&format!(
                    "- `{}`: {}\n",
                    evidence.category, evidence.summary
                ));
            }
            body.push('\n');
        }
        if !workflow.signoff.missing_evidence_categories.is_empty() {
            body.push_str("Missing signoff evidence:\n");
            for category in &workflow.signoff.missing_evidence_categories {
                body.push_str(&format!("- `{category}`\n"));
            }
            body.push('\n');
        }
        if !workflow.continuations.is_empty() {
            body.push_str("Continuations:\n");
            for continuation in &workflow.continuations {
                body.push_str(&format!(
                    "- `{}`: status={} iteration={} reason={}\n",
                    continuation.continuation_id,
                    continuation.status,
                    continuation.iteration,
                    continuation
                        .stop_reason
                        .as_deref()
                        .or(continuation.last_schedule_reason.as_deref())
                        .unwrap_or("n/a")
                ));
            }
            body.push('\n');
        }
    }
    body
}

fn workflow_init_files(project_root: &Path) -> Vec<(PathBuf, &'static str)> {
    vec![(
        project_root.join(".agent-harness/workflows/README.md"),
        "# Harness workflows\n\nThis directory is reserved for safe workflow bootstrap artifacts generated by `harness workflow init --apply`. Replayable workflow state remains in session `events.jsonl` files and redacted artifact references, not in this directory.\n",
    )]
}

fn write_explicit_output(path: &Path, body: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create output dir {}: {err}", parent.display()))?;
    }
    fs::write(path, body).map_err(|err| format!("failed to write {}: {err}", path.display()))
}

fn print_json(value: &impl Serialize, context: &str) -> Result<(), String> {
    let body = serde_json::to_string_pretty(value)
        .map_err(|err| format!("failed to render {context}: {err}"))?;
    println!("{body}");
    Ok(())
}

fn supervisor_actor() -> EventActor {
    EventActor::new(ActorKind::Supervisor, None)
}
