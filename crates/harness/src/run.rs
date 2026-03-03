use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::Args;
use harness_core::clock::{Clock, FakeClock, RealClock};
use harness_core::config::{load_config_from_file, resolve_config_path, ShellAllowlist};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
use harness_core::event::{EventEnvelopeV1, EventV1, ToolCallStatus};
use harness_core::perm::PermissionDecision;
use harness_core::redact::DefaultRedactor;
use harness_tools::coordinator_registry;
use uuid::Uuid;

use crate::scenarios::{
    create_workspace, default_permission_policy, golden_path_patch, golden_path_profiles,
    golden_path_provider, supervisor_actor, worker_actor, ScenarioName,
};

const DEFAULT_SESSION_DIR: &str = ".agent-harness/sessions";
const WAIT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Args, Clone)]
pub struct RunCommand {
    #[arg(long, value_enum)]
    pub scenario: ScenarioName,

    #[arg(long, default_value_t = false)]
    pub deterministic: bool,

    #[arg(long)]
    pub session_dir: Option<PathBuf>,

    #[arg(long)]
    pub out: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    pub print_run_dir: bool,
}

struct RunSettings {
    session_dir: PathBuf,
    shell_allowlist: ShellAllowlist,
    deterministic: bool,
    seed: u64,
    config_digest: String,
}

pub fn execute(
    cmd: RunCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> ExitCode {
    let settings = match resolve_settings(&cmd, config_path, global_session_dir) {
        Ok(settings) => settings,
        Err(err) => {
            eprintln!("run setup failed: {err}");
            return ExitCode::from(2);
        }
    };

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("failed to build async runtime: {err}");
            return ExitCode::from(1);
        }
    };

    match runtime.block_on(run_once(&cmd, &settings)) {
        Ok(outcome) => {
            if let Some(out) = &cmd.out {
                if let Err(err) = copy_events_file(&outcome.events_path, out) {
                    eprintln!("failed to write --out file: {err}");
                    return ExitCode::from(1);
                }
            }

            if cmd.print_run_dir {
                println!("{}", outcome.run_dir.display());
            } else {
                println!(
                    "scenario {} complete: {}",
                    cmd.scenario.as_str(),
                    outcome.events_path.display()
                );
            }

            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("run failed: {err}");
            ExitCode::from(1)
        }
    }
}

fn resolve_settings(
    cmd: &RunCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<RunSettings, String> {
    let explicit_config = resolve_config_path(config_path.as_deref());
    let mut shell_allowlist = ShellAllowlist::default();
    let mut config_session_dir = PathBuf::from(DEFAULT_SESSION_DIR);
    let mut config_deterministic = false;
    let mut config_seed = 0;
    let mut config_digest = "none".to_string();

    if let Some(path) = explicit_config {
        let config =
            load_config_from_file(&path).map_err(|err| format!("{} ({})", err, path.display()))?;
        let config_bytes = fs::read(&path)
            .map_err(|err| format!("failed to read config file {}: {err}", path.display()))?;
        config_digest = blake3::hash(&config_bytes).to_hex().to_string();
        shell_allowlist = config.permissions.shell_allowlist;
        config_session_dir = config.paths.session_dir;
        config_deterministic = config.deterministic.enabled;
        config_seed = config.deterministic.seed;
    }

    let session_dir = cmd
        .session_dir
        .clone()
        .or(global_session_dir)
        .unwrap_or(config_session_dir);
    let deterministic = cmd.deterministic
        || config_deterministic
        || matches!(std::env::var("HARNESS_DETERMINISTIC").as_deref(), Ok("1"));

    Ok(RunSettings {
        session_dir,
        shell_allowlist,
        deterministic,
        seed: config_seed,
        config_digest,
    })
}

struct RunOutcome {
    run_dir: PathBuf,
    events_path: PathBuf,
}

async fn run_once(cmd: &RunCommand, settings: &RunSettings) -> Result<RunOutcome, String> {
    fs::create_dir_all(&settings.session_dir)
        .map_err(|err| format!("failed to create session dir: {err}"))?;

    let deterministic_run_id = settings
        .deterministic
        .then(|| deterministic_run_id(settings.seed, cmd.scenario));

    if let Some(run_id) = &deterministic_run_id {
        let stale_run_dir = settings.session_dir.join(run_id);
        if stale_run_dir.exists() {
            fs::remove_dir_all(&stale_run_dir)
                .map_err(|err| format!("failed to reset deterministic run dir: {err}"))?;
        }
    }

    let workspace = create_workspace(
        &settings.session_dir,
        cmd.scenario,
        deterministic_run_id.as_deref(),
    )?;

    let clock: Arc<dyn Clock + Send + Sync> = if settings.deterministic {
        Arc::new(FakeClock::new())
    } else {
        Arc::new(RealClock::new())
    };

    let mut coordinator_config = CoordinatorConfig::new(settings.session_dir.clone());
    coordinator_config.deterministic_store = settings.deterministic;
    coordinator_config.permission_policy = default_permission_policy();
    coordinator_config.tool_registry =
        Arc::new(coordinator_registry(settings.shell_allowlist.clone()));
    coordinator_config.provider = Arc::new(golden_path_provider());
    coordinator_config.agent_profiles = golden_path_profiles();
    coordinator_config.run_id_override = deterministic_run_id;
    coordinator_config.config_digest = settings.config_digest.clone();
    coordinator_config.harness_version = env!("CARGO_PKG_VERSION").to_string();

    let coordinator = spawn_coordinator(
        coordinator_config,
        clock,
        Arc::new(DefaultRedactor::default()),
    );

    let run = coordinator
        .start_run(cmd.scenario.as_str(), &workspace)
        .await
        .map_err(|err| err.to_string())?;

    coordinator
        .spawn_agent(supervisor_actor(), "planner", None)
        .await
        .map_err(|err| err.to_string())?;

    let worker_agent_id = coordinator
        .spawn_agent(supervisor_actor(), "worker", None)
        .await
        .map_err(|err| err.to_string())?;

    let tool_call_id = coordinator
        .request_tool_call(
            worker_actor(worker_agent_id),
            Some("deep".to_string()),
            "edit.hashline_apply",
            serde_json::to_value(golden_path_patch()).map_err(|err| err.to_string())?,
        )
        .await
        .map_err(|err| err.to_string())?;

    let permission_id =
        wait_for_permission_id(&run.events_path, &tool_call_id, WAIT_TIMEOUT).await?;
    let decision = if cmd.scenario.interactive_permissions() {
        interactive_permission_decision(&permission_id)?
    } else {
        PermissionDecision::Allow
    };

    coordinator
        .resolve_permission(permission_id, decision)
        .await
        .map_err(|err| err.to_string())?;

    let tool_status = wait_for_tool_finished(&run.events_path, &tool_call_id, WAIT_TIMEOUT).await?;
    if tool_status != ToolCallStatus::Succeeded {
        return Err(format!("tool call did not succeed: {tool_status:?}"));
    }

    coordinator
        .stop_run()
        .await
        .map_err(|err| err.to_string())?;

    Ok(RunOutcome {
        run_dir: run.run_dir,
        events_path: run.events_path,
    })
}

fn interactive_permission_decision(permission_id: &str) -> Result<PermissionDecision, String> {
    println!("permission requested: {permission_id} (allow/deny)");
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|err| format!("failed to read interactive permission input: {err}"))?;

    let normalized = input.trim().to_ascii_lowercase();
    if normalized == "allow" || normalized == "a" || normalized == "y" {
        Ok(PermissionDecision::Allow)
    } else {
        Ok(PermissionDecision::Deny)
    }
}

async fn wait_for_permission_id(
    events_path: &Path,
    tool_call_id: &str,
    timeout: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let events = load_events(events_path)?;
        if let Some(permission_id) = events.into_iter().find_map(|event| match event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_deref() == Some(tool_call_id) =>
            {
                Some(data.permission_id)
            }
            _ => None,
        }) {
            return Ok(permission_id);
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for PermissionRequested for {tool_call_id}"
            ));
        }

        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn wait_for_tool_finished(
    events_path: &Path,
    tool_call_id: &str,
    timeout: Duration,
) -> Result<ToolCallStatus, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let events = load_events(events_path)?;
        if let Some(status) = events.into_iter().find_map(|event| match event.payload {
            EventV1::ToolCallFinished(data) if data.tool_call_id == tool_call_id => {
                Some(data.status)
            }
            _ => None,
        }) {
            return Ok(status);
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for ToolCallFinished for {tool_call_id}"
            ));
        }

        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn load_events(path: &Path) -> Result<Vec<EventEnvelopeV1>, String> {
    let body = fs::read_to_string(path)
        .map_err(|err| format!("failed to read events file {}: {err}", path.display()))?;
    body.lines()
        .map(|line| serde_json::from_str::<EventEnvelopeV1>(line).map_err(|err| err.to_string()))
        .collect()
}

fn copy_events_file(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create output directory {}: {err}",
                parent.display()
            )
        })?;
    }
    fs::copy(from, to).map_err(|err| {
        format!(
            "failed to copy events file from {} to {}: {err}",
            from.display(),
            to.display()
        )
    })?;
    Ok(())
}

fn deterministic_run_id(seed: u64, scenario: ScenarioName) -> String {
    let namespace = Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("harness-seed:{seed}").as_bytes(),
    );
    let run_uuid = Uuid::new_v5(&namespace, scenario.as_str().as_bytes());
    format!("run_{}", run_uuid.simple())
}

#[cfg(test)]
mod tests {
    use super::{deterministic_run_id, load_events, run_once, RunCommand, RunSettings};
    use crate::replay::summarize_session;
    use crate::scenarios::ScenarioName;
    use harness_core::config::ShellAllowlist;
    use harness_core::event::EventV1;
    use harness_core::proj::RunStatus;
    use sha2::{Digest, Sha256};

    #[tokio::test]
    async fn deterministic_golden_path_twice_produces_identical_sha256_digest() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let settings = RunSettings {
            session_dir: temp_dir.path().join("sessions"),
            shell_allowlist: ShellAllowlist::default(),
            deterministic: true,
            seed: 42,
            config_digest: "none".to_string(),
        };
        let command = RunCommand {
            scenario: ScenarioName::GoldenPath,
            deterministic: true,
            session_dir: None,
            out: None,
            print_run_dir: false,
        };

        let run_a = run_once(&command, &settings)
            .await
            .expect("first deterministic run");
        let digest_a = sha256_hex(&std::fs::read(&run_a.events_path).expect("read first jsonl"));

        let run_b = run_once(&command, &settings)
            .await
            .expect("second deterministic run");
        let digest_b = sha256_hex(&std::fs::read(&run_b.events_path).expect("read second jsonl"));

        assert_eq!(digest_a, digest_b);
    }

    #[test]
    fn deterministic_run_id_is_stable_for_seed_and_scenario() {
        let a = deterministic_run_id(7, ScenarioName::GoldenPath);
        let b = deterministic_run_id(7, ScenarioName::GoldenPath);
        let c = deterministic_run_id(8, ScenarioName::GoldenPath);
        let d = deterministic_run_id(7, ScenarioName::GoldenPathInteractive);

        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }

    #[tokio::test]
    async fn deterministic_run_writes_stable_meta_json_with_null_created_at() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let settings = RunSettings {
            session_dir: temp_dir.path().join("sessions"),
            shell_allowlist: ShellAllowlist::default(),
            deterministic: true,
            seed: 99,
            config_digest: "none".to_string(),
        };
        let command = RunCommand {
            scenario: ScenarioName::GoldenPath,
            deterministic: true,
            session_dir: None,
            out: None,
            print_run_dir: false,
        };

        let run_a = run_once(&command, &settings)
            .await
            .expect("first deterministic run");
        let meta_a = std::fs::read(run_a.run_dir.join("meta.json")).expect("read first meta");

        let run_b = run_once(&command, &settings)
            .await
            .expect("second deterministic run");
        let meta_b = std::fs::read(run_b.run_dir.join("meta.json")).expect("read second meta");

        assert_eq!(meta_a, meta_b);
        let value: serde_json::Value = serde_json::from_slice(&meta_a).expect("parse meta json");
        assert!(value.get("created_at").is_some());
        assert!(value.get("created_at").unwrap().is_null());
    }

    #[tokio::test]
    async fn replay_summary_matches_expected_values_for_golden_path() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let settings = RunSettings {
            session_dir: temp_dir.path().join("sessions"),
            shell_allowlist: ShellAllowlist::default(),
            deterministic: true,
            seed: 2026,
            config_digest: "none".to_string(),
        };
        let command = RunCommand {
            scenario: ScenarioName::GoldenPath,
            deterministic: true,
            session_dir: None,
            out: None,
            print_run_dir: false,
        };

        let run = run_once(&command, &settings)
            .await
            .expect("deterministic golden path run");
        let summary = summarize_session(&run.run_dir).expect("replay summary");

        assert_eq!(summary.status, RunStatus::Finished);
        assert_eq!(summary.counts_by_type.get("run_finished"), Some(&1));
        assert_eq!(summary.counts_by_type.get("edit_applied"), Some(&1));
        assert!(summary.pending_permissions.is_empty());
        assert!(summary.tasks_in_flight.is_empty());
    }

    #[tokio::test]
    async fn edit_applied_diff_refs_match_artifact_written() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let settings = RunSettings {
            session_dir: temp_dir.path().join("sessions"),
            shell_allowlist: ShellAllowlist::default(),
            deterministic: true,
            seed: 2027,
            config_digest: "none".to_string(),
        };
        let command = RunCommand {
            scenario: ScenarioName::GoldenPath,
            deterministic: true,
            session_dir: None,
            out: None,
            print_run_dir: false,
        };

        let run = run_once(&command, &settings)
            .await
            .expect("deterministic golden path run");
        let events = load_events(&run.events_path).expect("load events");

        let (diff_path, diff_digest) = events
            .iter()
            .find_map(|event| match &event.payload {
                EventV1::EditApplied(data) => {
                    Some((data.diff_rel_path.clone(), data.diff_digest.clone()))
                }
                _ => None,
            })
            .expect("edit applied event");

        assert!(diff_path.is_some(), "EditApplied.diff_rel_path must be set");
        assert!(diff_digest.is_some(), "EditApplied.diff_digest must be set");

        let diff_path = diff_path.expect("diff path");
        let diff_digest = diff_digest.expect("diff digest");
        let artifact_digest = events
            .iter()
            .find_map(|event| match &event.payload {
                EventV1::ArtifactWritten(data) if data.path == diff_path => {
                    Some(data.digest.clone())
                }
                _ => None,
            })
            .expect("artifact_written for diff path");

        assert_eq!(artifact_digest, diff_digest);
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        format!("{:x}", hasher.finalize())
    }
}
