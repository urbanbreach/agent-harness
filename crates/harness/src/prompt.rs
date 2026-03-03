use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::Args;
use harness_core::clock::{Clock, FakeClock, RealClock};
use harness_core::config::resolve_config_path;
use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
use harness_core::event::{ActorKind, EventActor, EventEnvelopeV1, EventV1};
use harness_core::redact::DefaultRedactor;
use uuid::Uuid;

use crate::{bootstrap, logging};

const WAIT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Args, Clone)]
pub struct PromptCommand {
    #[arg(long)]
    pub text: String,

    #[arg(long)]
    pub profile: Option<String>,

    #[arg(long)]
    pub out: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    pub print_run_dir: bool,
}

pub fn execute(
    cmd: PromptCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> ExitCode {
    let settings = match resolve_settings(config_path, global_session_dir) {
        Ok(settings) => settings,
        Err(err) => {
            eprintln!("prompt setup failed: {err}");
            return ExitCode::from(2);
        }
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("failed to build async runtime: {err}");
            return ExitCode::from(1);
        }
    };

    match runtime.block_on(run_prompt(&cmd, &settings)) {
        Ok(outcome) => {
            if let Some(out) = &cmd.out {
                if let Err(err) = copy_events_file(&outcome.events_path, out) {
                    eprintln!("failed to write --out file: {err}");
                    return ExitCode::from(1);
                }
            }

            if cmd.print_run_dir {
                println!("{}", outcome.run_dir.display());
            }

            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("prompt failed: {err}");
            ExitCode::from(1)
        }
    }
}

struct PromptSettings {
    config: harness_core::config::HarnessConfig,
    coordinator_config: CoordinatorConfig,
    deterministic: bool,
    config_digest: String,
}

struct PromptOutcome {
    run_dir: PathBuf,
    events_path: PathBuf,
}

fn resolve_settings(
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<PromptSettings, String> {
    let explicit_config = resolve_config_path(config_path.as_deref()).ok_or_else(|| {
        "prompt mode requires a config file; pass --config <path> or create harness.jsonc"
            .to_string()
    })?;

    let mut config = bootstrap::load_harness_config(&explicit_config)?;
    config.apply_session_dir_override(global_session_dir);

    let config_bytes = fs::read(&explicit_config)
        .map_err(|err| format!("failed to read config file {}: {err}", explicit_config.display()))?;
    let config_digest = blake3::hash(&config_bytes).to_hex().to_string();

    let deterministic = config.deterministic.enabled
        || matches!(std::env::var("HARNESS_DETERMINISTIC").as_deref(), Ok("1"));

    let coordinator_config = bootstrap::build_interactive_coordinator_config(&config)?;

    Ok(PromptSettings {
        config,
        coordinator_config,
        deterministic,
        config_digest,
    })
}

async fn run_prompt(cmd: &PromptCommand, settings: &PromptSettings) -> Result<PromptOutcome, String> {
    let mut coordinator_config = settings.coordinator_config.clone();
    coordinator_config.deterministic_store = settings.deterministic;
    coordinator_config.config_digest = settings.config_digest.clone();
    coordinator_config.harness_version = env!("CARGO_PKG_VERSION").to_string();

    coordinator_config.run_id_override = Some(if settings.deterministic {
        format!("prompt_{:016x}", settings.config.deterministic.seed)
    } else {
        let entropy = format!(
            "{}:{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        );
        format!("prompt_{}", Uuid::new_v5(&Uuid::NAMESPACE_OID, entropy.as_bytes()).simple())
    });

    if let Some(run_id) = &coordinator_config.run_id_override {
        let stale_run_dir = coordinator_config.session_dir.join(run_id);
        if stale_run_dir.exists() {
            fs::remove_dir_all(&stale_run_dir)
                .map_err(|err| format!("failed to reset deterministic run dir: {err}"))?;
        }
    }

    fs::create_dir_all(&coordinator_config.session_dir)
        .map_err(|err| format!("failed to create session dir: {err}"))?;

    let clock: Arc<dyn Clock + Send + Sync> = if settings.deterministic {
        Arc::new(FakeClock::new())
    } else {
        Arc::new(RealClock::new())
    };

    let coordinator = spawn_coordinator(
        coordinator_config,
        clock,
        Arc::new(DefaultRedactor::default()),
    );

    let workspace = std::env::current_dir()
        .map_err(|err| format!("failed to resolve current working directory: {err}"))?;

    let run = coordinator
        .start_run("prompt", workspace)
        .await
        .map_err(|err| err.to_string())?;

    logging::init_logging(&settings.config, &run.artifacts_dir)?;

    let profile_name = cmd
        .profile
        .clone()
        .unwrap_or_else(|| bootstrap::interactive_profile_name(&settings.config));
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), profile_name, None)
        .await
        .map_err(|err| err.to_string())?;

    let request_id = coordinator
        .request_agent_turn(user_actor(), agent_id, cmd.text.clone())
        .await
        .map_err(|err| err.to_string())?;

    let wait_result = wait_for_prompt_completion(&run.events_path, &request_id, WAIT_TIMEOUT).await;
    let stop_result = coordinator.stop_run().await;

    wait_result?;
    stop_result.map_err(|err| err.to_string())?;

    Ok(PromptOutcome {
        run_dir: run.run_dir,
        events_path: run.events_path,
    })
}

fn supervisor_actor() -> EventActor {
    EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string()))
}

fn user_actor() -> EventActor {
    EventActor::new(ActorKind::User, Some("agent-supervisor".to_string()))
}

async fn wait_for_prompt_completion(
    events_path: &Path,
    request_id: &str,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;

    loop {
        let events = load_events(events_path)?;

        if let Some(run_error) = events.iter().find_map(|event| match &event.payload {
            EventV1::RunFailed(data) => Some(data.error.clone()),
            _ => None,
        }) {
            return Err(format!(
                "run failed before prompt completion for {request_id}: {run_error}"
            ));
        }

        if events.iter().any(|event| match &event.payload {
            EventV1::ProviderRequestFinished(data) => data.request_id == request_id,
            EventV1::TaskCompleted(data) => data.task_id == request_id,
            _ => false,
        }) {
            return Ok(());
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for ProviderRequestFinished or TaskCompleted for {request_id}"
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
