use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Args, Subcommand};
use harness_core::clock::{Clock, RealClock};
use harness_core::config::load_resolved_config;
use harness_core::context_snapshot::{
    ContextSnapshotAmbiguity, ContextSnapshotInput, ContextSnapshotOptions,
    ContextSnapshotWriteResult,
};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, RunInfo};
use harness_core::event::{ActorKind, EventActor};
use harness_core::redact::DefaultRedactor;
use serde::Serialize;

use crate::defaults::DEFAULT_SESSION_DIR;

#[derive(Debug, Args, Clone)]
pub struct WorkflowCommand {
    #[command(subcommand)]
    command: WorkflowCommands,
}

#[derive(Debug, Subcommand, Clone)]
enum WorkflowCommands {
    Snapshot(WorkflowSnapshotCommand),
}

#[derive(Debug, Args, Clone)]
struct WorkflowSnapshotCommand {
    #[command(subcommand)]
    command: WorkflowSnapshotCommands,
}

#[derive(Debug, Subcommand, Clone)]
enum WorkflowSnapshotCommands {
    Write(WorkflowSnapshotWriteCommand),
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

#[derive(Debug, Serialize)]
struct SnapshotWriteReport {
    run_id: String,
    run_dir: String,
    events_path: String,
    snapshot: ContextSnapshotWriteResult,
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
        WorkflowCommands::Snapshot(snapshot) => match snapshot.command {
            WorkflowSnapshotCommands::Write(write) => runtime.block_on(execute_snapshot_write(
                write,
                config_path,
                global_session_dir,
            )),
        },
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("workflow command failed: {err}");
            ExitCode::from(1)
        }
    }
}

async fn execute_snapshot_write(
    cmd: WorkflowSnapshotWriteCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<(), String> {
    let session_dir = resolve_session_dir(config_path, global_session_dir)?;
    fs::create_dir_all(&session_dir).map_err(|err| {
        format!(
            "failed to create session dir {}: {err}",
            session_dir.display()
        )
    })?;

    let mut coordinator_config = CoordinatorConfig::new(session_dir);
    coordinator_config.session_mode_source = Some(harness_core::proj::SessionModeSource::Prompt);
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
        .write_context_snapshot(
            EventActor::new(ActorKind::Supervisor, None),
            cmd.workflow_id.clone(),
            input,
            options,
        )
        .await;

    let stop_result = coordinator.stop_run().await;
    let result = result.map_err(|err| err.to_string())?;
    stop_result.map_err(|err| err.to_string())?;

    print_snapshot_write_report(&cmd, &run, result)
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
        let body = serde_json::to_string_pretty(&report)
            .map_err(|err| format!("failed to render workflow snapshot JSON: {err}"))?;
        println!("{body}");
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

fn resolve_session_dir(
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<PathBuf, String> {
    let loaded = load_resolved_config(config_path.as_deref()).map_err(|err| err.to_string())?;
    if let Some(loaded) = loaded {
        let mut config = loaded.config;
        config.apply_session_dir_override(global_session_dir);
        return Ok(config.paths.session_dir);
    }

    Ok(global_session_dir.unwrap_or_else(|| PathBuf::from(DEFAULT_SESSION_DIR)))
}
