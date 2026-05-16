use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{ArgGroup, Args, Subcommand, ValueEnum};
use harness_core::agent_catalog::{resolve_agent_catalog, AgentCatalog};
use harness_core::clock::{Clock, RealClock};
use harness_core::config::{load_resolved_config, WorkflowRuntimeConfig};
use harness_core::context_snapshot::{
    ContextSnapshotAmbiguity, ContextSnapshotInput, ContextSnapshotOptions,
    ContextSnapshotWriteResult,
};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, RunInfo};
use harness_core::event::{ActorKind, EventActor};
use harness_core::goal_ledger::{
    goal_checkpoint_artifact_name, goal_checkpoint_metadata, goal_ledger_artifact_name,
    goal_ledger_metadata, project_goal_ledger, validate_goal_checkpoint_artifact,
    validate_goal_ledger_artifact, GoalCheckpointArtifact, GoalLedgerArtifact,
    GoalLedgerProjection, GoalQualityGate, GoalStoryArtifact, GOAL_LEDGER_EVIDENCE_CATEGORY,
    GOAL_LEDGER_MODE, GOAL_LEDGER_SCHEMA_VERSION,
};
use harness_core::persistent_task::{project_persistent_tasks, PersistentTaskProjection};
use harness_core::plan_consensus::{
    plan_consensus_artifact_name, plan_consensus_metadata, resolve_plan_consensus_lanes,
    validate_plan_consensus_artifact, PlanConsensusArtifact, PlanConsensusLane,
    PlanConsensusOption, PLAN_CONSENSUS_EVIDENCE_CATEGORY, PLAN_CONSENSUS_MODE,
    PLAN_CONSENSUS_SCHEMA_VERSION,
};
use harness_core::proj::SessionModeSource;
use harness_core::redact::DefaultRedactor;
use harness_core::run_dossier::{build_run_dossier_with_tasks, RunDossier};
use harness_core::tool::ArtifactStore;
use harness_core::workflow::{
    project_workflows, WorkflowEvidenceRequest, WorkflowProjection, WorkflowSignoffPolicy,
    WorkflowStartRequest, WorkflowStartResult,
};
use serde::Serialize;

use crate::cli_io::{load_events_from_run_dir, EVENTS_FILE_NAME};
use crate::defaults::DEFAULT_SESSION_DIR;

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
    /// Record an operator signoff decision in a coordinator-owned audit run.
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
        .args(["approve", "fail", "request_evidence"])
))]
struct WorkflowSignoffCommand {
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

    /// Operator id recorded with the decision.
    #[arg(long, default_value = "operator")]
    operator: String,

    /// Decision reason.
    #[arg(long)]
    reason: Option<String>,

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
}

#[derive(Debug, Serialize)]
struct WorkflowMutationReport {
    run_id: String,
    run_dir: String,
    events_path: String,
    workflow_id: String,
    decision: String,
    terminal_outcome: Option<String>,
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
        WorkflowCommands::Init(init) => execute_init(init),
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
            println!(
                "- {} mode={} status={} owner={}",
                workflow.workflow_id, workflow.mode, workflow.status, workflow.owner
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
    let decision = if cmd.approve {
        "signoff-approved"
    } else if cmd.fail {
        "signoff-failed"
    } else {
        "request-evidence"
    };
    let terminal_outcome = if cmd.approve {
        Some("outcome.finished")
    } else if cmd.fail {
        Some("outcome.failed")
    } else {
        None
    };
    let reason = cmd
        .reason
        .clone()
        .unwrap_or_else(|| format!("workflow signoff decision: {decision}"));
    let report = execute_workflow_audit_mutation(WorkflowAuditMutationRequest {
        config_path,
        global_session_dir,
        run_name: "workflow signoff",
        workflow_id: &cmd.workflow_id,
        decision,
        owner: &cmd.operator,
        reason: &reason,
        terminal_outcome,
    })
    .await?;

    if cmd.json {
        print_json(&report, "workflow signoff JSON")?;
    } else {
        println!(
            "workflow signoff recorded: {} decision={} audit_run={}",
            report.workflow_id, report.decision, report.run_id
        );
    }
    Ok(())
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
    } = request;
    let context = resolve_workflow_context(config_path, global_session_dir)?;
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
    })
}

fn execute_dossier_export(
    cmd: WorkflowDossierExportCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    let report = status_report(cmd.target, cmd.workflow_id, config_path, global_session_dir)?;
    let dossier = build_run_dossier_with_tasks(
        &report.projection,
        &report.persistent_tasks,
        &WorkflowSignoffPolicy::simulator_default(),
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

fn execute_init(cmd: WorkflowInitCommand) -> Result<(), String> {
    let project_root = std::env::current_dir()
        .map_err(|err| format!("failed to resolve current working directory: {err}"))?;
    let files = workflow_init_files(&project_root);
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
    let run_dir = resolve_read_run_dir(target, config_path, global_session_dir)?;
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
        persistent_tasks.tasks.retain(|_, task| {
            task.metadata
                .get(harness_core::workflow::WORKFLOW_TASK_METADATA_KEY)
                == Some(&workflow_id)
        });
    }
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
    })
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
        return Ok(WorkflowRuntimeContext {
            session_dir: config.paths.session_dir,
            workflow: config.runtime.workflow,
            agent_catalog: Some(agent_catalog),
        });
    }

    Ok(WorkflowRuntimeContext {
        session_dir: global_session_dir.unwrap_or_else(|| PathBuf::from(DEFAULT_SESSION_DIR)),
        workflow: WorkflowRuntimeConfig::default(),
        agent_catalog: None,
    })
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
            "## `{}`\n\n- Mode: `{}`\n- Status: `{}`\n- Owner: `{}`\n- Terminal: `{}`\n- Signoff allowed: `{}`\n\n",
            workflow.workflow_id,
            workflow.mode,
            workflow.status,
            workflow.owner,
            workflow.terminal,
            workflow.signoff.allowed
        ));
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
