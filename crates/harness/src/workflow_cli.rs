use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{ArgGroup, Args, Subcommand, ValueEnum};
use harness_core::clock::{Clock, RealClock};
use harness_core::config::{load_resolved_config, WorkflowRuntimeConfig};
use harness_core::context_snapshot::{
    ContextSnapshotAmbiguity, ContextSnapshotInput, ContextSnapshotOptions,
    ContextSnapshotWriteResult,
};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, RunInfo};
use harness_core::event::{ActorKind, EventActor};
use harness_core::proj::SessionModeSource;
use harness_core::redact::DefaultRedactor;
use harness_core::run_dossier::{build_run_dossier, RunDossier};
use harness_core::workflow::{
    project_workflows, WorkflowProjection, WorkflowSignoffPolicy, WorkflowStartRequest,
    WorkflowStartResult,
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
    let report = execute_workflow_audit_mutation(
        config_path,
        global_session_dir,
        "workflow signoff",
        &cmd.workflow_id,
        decision,
        &cmd.operator,
        &reason,
        terminal_outcome,
    )
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
    let report = execute_workflow_audit_mutation(
        config_path,
        global_session_dir,
        "workflow cancel",
        &cmd.workflow_id,
        "abort",
        &cmd.owner,
        &reason,
        Some("outcome.cancelled"),
    )
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

async fn execute_workflow_audit_mutation(
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
    run_name: &str,
    workflow_id: &str,
    decision: &str,
    owner: &str,
    reason: &str,
    terminal_outcome: Option<&str>,
) -> Result<WorkflowMutationReport, String> {
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
    let dossier = build_run_dossier(
        &report.projection,
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
    if let Some(workflow_id) = workflow_id {
        projection
            .workflows
            .retain(|id, _| id.as_str() == workflow_id.as_str());
        projection.evidence.retain(|id, _| id == &workflow_id);
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
    })
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
}

fn resolve_workflow_context(
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<WorkflowRuntimeContext, String> {
    let loaded = load_resolved_config(config_path.as_deref()).map_err(|err| err.to_string())?;
    if let Some(loaded) = loaded {
        let mut config = loaded.config;
        config.apply_session_dir_override(global_session_dir);
        return Ok(WorkflowRuntimeContext {
            session_dir: config.paths.session_dir,
            workflow: config.runtime.workflow,
        });
    }

    Ok(WorkflowRuntimeContext {
        session_dir: global_session_dir.unwrap_or_else(|| PathBuf::from(DEFAULT_SESSION_DIR)),
        workflow: WorkflowRuntimeConfig::default(),
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
