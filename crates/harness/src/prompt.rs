use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::Args;
use harness_core::agent::{default_model_settings_for_profile, AgentModelSettings};
use harness_core::clock::{Clock, FakeClock, RealClock};
use harness_core::config::{
    resolve_config_path, resolve_configured_model_metadata, ShellAllowlist,
};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
use harness_core::event::{ActorKind, EventActor, EventEnvelopeV1, EventV1};
use harness_core::perm::PermissionPolicy;
use harness_core::proj::inspect_resume_plan;
use harness_core::redact::DefaultRedactor;
use harness_core::store::{EventStore, EventStoreError};
use harness_tools::coordinator_registry;
use harness_tui::load_events_from_run_dir;
use uuid::Uuid;

use crate::recovery::{
    inspect_session_recovery, latest_run_name, resolve_session_run_dir, select_resume_agent_id,
};
use crate::{
    bootstrap, logging,
    scenarios::{golden_path_profiles, golden_path_provider},
};

const DEFAULT_WAIT_TIMEOUT: Duration = Duration::from_secs(30);
const WAIT_TIMEOUT_ENV: &str = "HARNESS_PROMPT_WAIT_TIMEOUT_MS";
const PROVIDER_ERROR_REASON_GRACE: Duration = Duration::from_secs(2);
const DEFAULT_SESSION_DIR: &str = ".agent-harness/sessions";
const DEFAULT_MOCK_PROFILE: &str = "worker";

#[derive(Debug, Args, Clone)]
pub struct PromptCommand {
    #[arg(long)]
    pub text: String,

    #[arg(long)]
    pub model: Option<String>,

    #[arg(long)]
    pub variant: Option<String>,

    #[arg(long, default_value_t = false)]
    pub thinking: bool,

    #[arg(long, default_value_t = false, conflicts_with = "resume")]
    pub mock: bool,

    #[arg(long, conflicts_with = "resume")]
    pub profile: Option<String>,

    #[arg(long, value_name = "RUN_ID_OR_PATH", conflicts_with = "profile")]
    pub resume: Option<String>,

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
    let settings = match resolve_settings(&cmd, config_path, global_session_dir) {
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
    logging_config: Option<harness_core::config::HarnessConfig>,
    coordinator_config: CoordinatorConfig,
    default_profile: String,
    deterministic: bool,
    deterministic_seed: u64,
    config_digest: String,
}

struct PromptOutcome {
    run_dir: PathBuf,
    events_path: PathBuf,
}

fn resolve_settings(
    cmd: &PromptCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<PromptSettings, String> {
    if cmd.mock {
        return resolve_mock_settings(config_path, global_session_dir);
    }

    let explicit_config = resolve_config_path(config_path.as_deref()).ok_or_else(|| {
        "prompt mode requires a config file; pass --config <path> or create harness.jsonc. A starting point lives at configs/harness.example.jsonc, or re-run with --mock"
            .to_string()
    })?;

    let mut config = bootstrap::load_harness_config(&explicit_config)?;
    config.apply_session_dir_override(global_session_dir);

    let config_bytes = fs::read(&explicit_config).map_err(|err| {
        format!(
            "failed to read config file {}: {err}",
            explicit_config.display()
        )
    })?;
    let config_digest = blake3::hash(&config_bytes).to_hex().to_string();

    let deterministic = config.deterministic.enabled
        || matches!(std::env::var("HARNESS_DETERMINISTIC").as_deref(), Ok("1"));
    let deterministic_seed = config.deterministic.seed;
    let default_profile = bootstrap::interactive_profile_name(&config);
    let coordinator_config = bootstrap::build_interactive_coordinator_config(&config)?;

    Ok(PromptSettings {
        logging_config: Some(config),
        coordinator_config,
        default_profile,
        deterministic,
        deterministic_seed,
        config_digest,
    })
}

fn resolve_mock_settings(
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
) -> Result<PromptSettings, String> {
    let explicit_config = resolve_config_path(config_path.as_deref());
    let mut shell_allowlist = ShellAllowlist::default();
    let mut session_dir = PathBuf::from(DEFAULT_SESSION_DIR);
    let mut deterministic = false;
    let mut deterministic_seed = 0;
    let mut config_digest = "none".to_string();
    let mut logging_config = None;

    if let Some(path) = explicit_config {
        let mut config = bootstrap::load_harness_config(&path)?;
        config.apply_session_dir_override(global_session_dir.clone());

        let config_bytes = fs::read(&path)
            .map_err(|err| format!("failed to read config file {}: {err}", path.display()))?;
        config_digest = blake3::hash(&config_bytes).to_hex().to_string();
        shell_allowlist = config.permissions.shell_allowlist.clone();
        session_dir = config.paths.session_dir.clone();
        deterministic = config.deterministic.enabled;
        deterministic_seed = config.deterministic.seed;
        logging_config = Some(config);
    }

    let session_dir = global_session_dir.unwrap_or(session_dir);
    deterministic |= matches!(std::env::var("HARNESS_DETERMINISTIC").as_deref(), Ok("1"));

    let mut coordinator_config = CoordinatorConfig::new(session_dir);
    coordinator_config.permission_policy = default_prompt_permission_policy();
    coordinator_config.tool_registry = Arc::new(coordinator_registry(shell_allowlist));
    coordinator_config.provider = Arc::new(golden_path_provider());
    coordinator_config.agent_profiles = golden_path_profiles();

    Ok(PromptSettings {
        logging_config,
        coordinator_config,
        default_profile: DEFAULT_MOCK_PROFILE.to_string(),
        deterministic,
        deterministic_seed,
        config_digest,
    })
}

async fn run_prompt(
    cmd: &PromptCommand,
    settings: &PromptSettings,
) -> Result<PromptOutcome, String> {
    if let Some(selector) = &cmd.resume {
        return run_resumed_prompt(cmd, settings, selector).await;
    }

    let mut coordinator_config = settings.coordinator_config.clone();
    coordinator_config.deterministic_store = settings.deterministic;
    coordinator_config.hook_runtime_config.suppress_execution = settings.deterministic;
    coordinator_config.config_digest = settings.config_digest.clone();
    coordinator_config.harness_version = env!("CARGO_PKG_VERSION").to_string();

    coordinator_config.run_id_override = Some(if settings.deterministic {
        format!("prompt_{:016x}", settings.deterministic_seed)
    } else {
        let entropy = format!(
            "{}:{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        );
        format!(
            "prompt_{}",
            Uuid::new_v5(&Uuid::NAMESPACE_OID, entropy.as_bytes()).simple()
        )
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

    if let Some(config) = &settings.logging_config {
        logging::init_logging(config, &run.artifacts_dir)?;
    }

    let profile_name = cmd
        .profile
        .clone()
        .unwrap_or_else(|| settings.default_profile.clone());
    let model_override = resolve_prompt_model_override(cmd, settings, &profile_name)?;
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), profile_name, None)
        .await
        .map_err(|err| err.to_string())?;

    let request_id = match model_override {
        Some(model_override) => {
            coordinator
                .request_agent_turn_with_model(
                    user_actor(),
                    agent_id,
                    cmd.text.clone(),
                    model_override.model_ref,
                    Some(model_override.model_settings),
                )
                .await
        }
        None => {
            coordinator
                .request_agent_turn(user_actor(), agent_id, cmd.text.clone())
                .await
        }
    }
    .map_err(|err| err.to_string())?;
    let event_store = coordinator
        .event_store()
        .await
        .map_err(|err| err.to_string())?;

    let wait_timeout = prompt_wait_timeout();
    let wait_result = wait_for_prompt_completion_with_output(
        event_store,
        &request_id,
        wait_timeout,
        cmd.thinking,
    )
    .await;
    let stop_result = coordinator.stop_run().await;

    wait_result?;
    stop_result.map_err(|err| err.to_string())?;

    Ok(PromptOutcome {
        run_dir: run.run_dir,
        events_path: run.events_path,
    })
}

async fn run_resumed_prompt(
    cmd: &PromptCommand,
    settings: &PromptSettings,
    selector: &str,
) -> Result<PromptOutcome, String> {
    let mut coordinator_config = settings.coordinator_config.clone();
    coordinator_config.deterministic_store = settings.deterministic;
    coordinator_config.hook_runtime_config.suppress_execution = settings.deterministic;
    coordinator_config.config_digest = settings.config_digest.clone();
    coordinator_config.harness_version = env!("CARGO_PKG_VERSION").to_string();
    coordinator_config.run_id_override = None;

    let run_dir = resolve_session_run_dir(selector, &coordinator_config.session_dir)?;
    let recovery = inspect_session_recovery(&run_dir)?;
    if !recovery.resumable {
        let reason = recovery
            .resume_disabled_reason
            .clone()
            .unwrap_or_else(|| "resume unavailable without reason".to_string());
        return Err(format!(
            "resume is disabled for {}: {reason}",
            recovery.run_id
        ));
    }

    let resume_plan = inspect_resume_plan(&run_dir);
    let historical_events = load_events_from_run_dir(&run_dir).map_err(|err| err.to_string())?;
    let resume_agent_id =
        select_resume_agent_id(&resume_plan, &historical_events, &recovery.run_id)?;
    let resume_profile = resume_plan
        .known_agents
        .get(&resume_agent_id)
        .cloned()
        .unwrap_or_else(|| settings.default_profile.clone());
    let run_name = recovery
        .run_name
        .clone()
        .or_else(|| latest_run_name(&historical_events))
        .unwrap_or_else(|| "interactive".to_string());

    let session_dir = run_dir.parent().ok_or_else(|| {
        format!(
            "failed to resolve parent session directory for {}",
            run_dir.display()
        )
    })?;
    fs::create_dir_all(session_dir)
        .map_err(|err| format!("failed to create session dir: {err}"))?;
    coordinator_config.session_dir = session_dir.to_path_buf();

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

    let run = coordinator
        .resume_run(recovery.run_id.clone(), run_name)
        .await
        .map_err(|err| err.to_string())?;

    if let Some(config) = &settings.logging_config {
        logging::init_logging(config, &run.artifacts_dir)?;
    }

    let model_override = resolve_prompt_model_override(cmd, settings, &resume_profile)?;
    let request_id = match model_override {
        Some(model_override) => {
            coordinator
                .request_agent_turn_with_model(
                    user_actor(),
                    resume_agent_id,
                    cmd.text.clone(),
                    model_override.model_ref,
                    Some(model_override.model_settings),
                )
                .await
        }
        None => {
            coordinator
                .request_agent_turn(user_actor(), resume_agent_id, cmd.text.clone())
                .await
        }
    }
    .map_err(|err| err.to_string())?;
    let event_store = coordinator
        .event_store()
        .await
        .map_err(|err| err.to_string())?;

    let wait_timeout = prompt_wait_timeout();
    let wait_result = wait_for_prompt_completion_with_output(
        event_store,
        &request_id,
        wait_timeout,
        cmd.thinking,
    )
    .await;
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

fn default_prompt_permission_policy() -> PermissionPolicy {
    use harness_core::config::PermissionMode;

    PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Deny,
        PermissionMode::Deny,
    )
}

#[cfg(test)]
async fn wait_for_prompt_completion(
    event_store: Arc<dyn EventStore>,
    request_id: &str,
    timeout: Duration,
) -> Result<(), String> {
    wait_for_prompt_completion_with_output(event_store, request_id, timeout, false).await
}

async fn wait_for_prompt_completion_with_output(
    event_store: Arc<dyn EventStore>,
    request_id: &str,
    timeout: Duration,
    show_thinking: bool,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    let mut tracker = PromptCompletionTracker::new(request_id);
    let mut printer = PromptStreamPrinter::new(show_thinking);
    let mut next_seq = 1;
    let mut stream = event_store
        .subscribe(next_seq)
        .map_err(|err| format!("failed to subscribe to prompt event stream: {err}"))?;

    loop {
        let wait_until = tracker.next_wait_deadline(deadline);
        let wait_duration = wait_until.saturating_duration_since(Instant::now());

        match tokio::time::timeout(
            wait_duration,
            std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)),
        )
        .await
        {
            Ok(Some(Ok(event))) => {
                next_seq = event.seq.saturating_add(1);
                printer.observe(&event, request_id);
                match tracker.observe(&event) {
                    PromptCompletionStatus::Continue => {}
                    PromptCompletionStatus::Completed => {
                        printer.finish();
                        return Ok(());
                    }
                    PromptCompletionStatus::Failed(error) => {
                        printer.finish();
                        return Err(error);
                    }
                }
            }
            Ok(Some(Err(EventStoreError::SubscriberLagged(_)))) => {
                stream = event_store.subscribe(next_seq).map_err(|err| {
                    format!("failed to resubscribe to prompt event stream: {err}")
                })?;
            }
            Ok(Some(Err(err))) => {
                printer.finish();
                return Err(format!("prompt event stream error: {err}"));
            }
            Ok(None) => {
                printer.finish();
                return Err(format!(
                    "prompt event stream closed before completion for {request_id}"
                ));
            }
            Err(_) => {}
        }

        if let Some(error) = tracker.provider_error_timeout() {
            printer.finish();
            return Err(error);
        }

        if Instant::now() >= deadline {
            printer.finish();
            return Err(format!(
                "timed out waiting for ProviderRequestFinished or TaskCompleted for {request_id}"
            ));
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PromptModelOverride {
    model_ref: Option<String>,
    model_settings: AgentModelSettings,
}

fn resolve_prompt_model_override(
    cmd: &PromptCommand,
    settings: &PromptSettings,
    profile_name: &str,
) -> Result<Option<PromptModelOverride>, String> {
    if cmd.model.is_none() && cmd.variant.is_none() && !cmd.thinking {
        return Ok(None);
    }

    let mut model_settings = default_model_settings_for_profile(profile_name);
    let mut model_ref_override = None;

    if let Some(config) = settings.logging_config.as_ref() {
        let (provider, model) = if let Some(model_ref) = cmd.model.as_deref() {
            parse_cli_model_ref(model_ref)?
        } else {
            let profile = config.agents.get(profile_name).ok_or_else(|| {
                format!("unknown agent `{profile_name}` while resolving prompt model override")
            })?;
            parse_cli_model_ref(&profile.model_ref)?
        };

        let resolved =
            resolve_configured_model_metadata(config, &provider, &model, cmd.variant.as_deref())
                .map_err(|err| err.to_string())?;

        model_settings.variant = resolved.variant.clone();
        model_settings.reasoning_effort = resolved.reasoning_effort.clone();
        model_settings.text_verbosity = resolved.text_verbosity.clone();
        model_settings.reasoning_summary =
            if resolved.supports_reasoning_summaries && model_settings.reasoning_effort.is_some() {
                Some("auto".to_string())
            } else {
                None
            };

        if cmd.thinking && model_settings.reasoning_summary.is_none() {
            model_settings.reasoning_summary = Some("auto".to_string());
        }

        if cmd.model.is_some() || cmd.variant.is_some() || cmd.thinking {
            model_ref_override = Some(format!("{}:{}", resolved.provider, resolved.model));
        }
    } else {
        if let Some(model_ref) = cmd.model.as_deref() {
            let (provider, model) = parse_cli_model_ref(model_ref)?;
            model_ref_override = Some(format!("{provider}:{model}"));
        }
        if let Some(variant) = cmd.variant.as_ref() {
            model_settings.variant = Some(variant.clone());
        }
        if cmd.thinking && model_settings.reasoning_summary.is_none() {
            model_settings.reasoning_summary = Some("auto".to_string());
        }
    }

    Ok(Some(PromptModelOverride {
        model_ref: model_ref_override,
        model_settings,
    }))
}

fn parse_cli_model_ref(model_ref: &str) -> Result<(String, String), String> {
    let normalized = model_ref.trim();
    let Some((provider, model)) = normalized
        .split_once(':')
        .or_else(|| normalized.split_once('/'))
    else {
        return Err(format!(
            "invalid model selector `{normalized}`; use `<provider>:<model>`"
        ));
    };

    let provider = provider.trim();
    let model = model.trim();
    if provider.is_empty() || model.is_empty() {
        return Err(format!(
            "invalid model selector `{normalized}`; use `<provider>:<model>`"
        ));
    }

    Ok((provider.to_string(), model.to_string()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptStreamSection {
    Thinking,
    Assistant,
}

struct PromptStreamPrinter {
    show_thinking: bool,
    active_section: Option<PromptStreamSection>,
    wrote_output: bool,
}

impl PromptStreamPrinter {
    fn new(show_thinking: bool) -> Self {
        Self {
            show_thinking,
            active_section: None,
            wrote_output: false,
        }
    }

    fn observe(&mut self, event: &EventEnvelopeV1, request_id: &str) {
        match &event.payload {
            EventV1::ProviderReasoningDelta(data)
                if self.show_thinking && data.request_id == request_id =>
            {
                self.write_thinking(&data.delta);
            }
            EventV1::ProviderStreamDelta(data) if data.request_id == request_id => {
                self.write_assistant(&data.delta);
            }
            _ => {}
        }
    }

    fn finish(&mut self) {
        if self.wrote_output {
            println!();
        }
        self.active_section = None;
        self.wrote_output = false;
    }

    fn write_thinking(&mut self, delta: &str) {
        if delta.is_empty() {
            return;
        }
        if self.active_section != Some(PromptStreamSection::Thinking) {
            if self.wrote_output {
                println!();
            }
            print!("Thinking: ");
            self.active_section = Some(PromptStreamSection::Thinking);
        }
        self.wrote_output = true;
        print!("{delta}");
        let _ = std::io::stdout().flush();
    }

    fn write_assistant(&mut self, delta: &str) {
        if delta.is_empty() {
            return;
        }
        if self.active_section == Some(PromptStreamSection::Thinking) {
            println!();
        }
        self.active_section = Some(PromptStreamSection::Assistant);
        self.wrote_output = true;
        print!("{delta}");
        let _ = std::io::stdout().flush();
    }
}

/// Prompt mode waits on the coordinator event stream once, then processes replayed
/// and live events incrementally. That keeps the steady-state wait cost bounded by
/// new events instead of rereading and reparsing the full JSONL log every poll tick.
#[derive(Debug)]
struct PromptCompletionTracker<'a> {
    request_id: &'a str,
    prompt_task_id: Option<String>,
    provider_error_seen_at: Option<Instant>,
}

impl<'a> PromptCompletionTracker<'a> {
    fn new(request_id: &'a str) -> Self {
        Self {
            request_id,
            prompt_task_id: None,
            provider_error_seen_at: None,
        }
    }

    fn observe(&mut self, event: &EventEnvelopeV1) -> PromptCompletionStatus {
        match &event.payload {
            EventV1::RunFailed(data) => {
                return PromptCompletionStatus::Failed(format!(
                    "run failed before prompt completion for {}: {}",
                    self.request_id, data.error
                ));
            }
            EventV1::TaskScheduled(data)
                if event_matches_request(event, self.request_id)
                    && data
                        .queue_key
                        .as_deref()
                        .is_some_and(|queue_key| queue_key.starts_with("provider_model:")) =>
            {
                self.prompt_task_id = Some(data.task_id.clone());
            }
            EventV1::ProviderRequestFinished(data)
                if data.request_id == self.request_id
                    && data.finish_reason.eq_ignore_ascii_case("error")
                    && self.provider_error_seen_at.is_none() =>
            {
                self.provider_error_seen_at = Some(Instant::now());
            }
            EventV1::TaskCancelled(data)
                if self.matches_cancelled_prompt_task(event, &data.task_id) =>
            {
                return PromptCompletionStatus::Failed(format!(
                    "prompt request {} was cancelled: {}",
                    self.request_id, data.reason
                ));
            }
            EventV1::TaskCompleted(data) if self.matches_prompt_task(&data.task_id) => {
                return PromptCompletionStatus::Completed;
            }
            _ => {}
        }

        PromptCompletionStatus::Continue
    }

    fn next_wait_deadline(&self, timeout_deadline: Instant) -> Instant {
        self.provider_error_seen_at
            .map(|seen_at| std::cmp::min(timeout_deadline, seen_at + PROVIDER_ERROR_REASON_GRACE))
            .unwrap_or(timeout_deadline)
    }

    fn provider_error_timeout(&self) -> Option<String> {
        self.provider_error_seen_at.and_then(|seen_at| {
            (Instant::now().saturating_duration_since(seen_at) >= PROVIDER_ERROR_REASON_GRACE).then(
                || {
                    format!(
                        "prompt request {} finished with provider error",
                        self.request_id
                    )
                },
            )
        })
    }

    fn matches_prompt_task(&self, task_id: &str) -> bool {
        self.prompt_task_id.as_deref() == Some(task_id) || task_id == self.request_id
    }

    fn matches_cancelled_prompt_task(&self, event: &EventEnvelopeV1, task_id: &str) -> bool {
        event_matches_request(event, self.request_id)
            && (self.prompt_task_id.is_none()
                || self.prompt_task_id.as_deref() == Some(task_id)
                || task_id == self.request_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PromptCompletionStatus {
    Continue,
    Completed,
    Failed(String),
}

#[cfg(test)]
fn evaluate_prompt_completion(
    events: &[EventEnvelopeV1],
    request_id: &str,
) -> PromptCompletionStatus {
    let prompt_task_id = prompt_task_id(events, request_id);

    if let Some(run_error) = events.iter().find_map(|event| match &event.payload {
        EventV1::RunFailed(data) => Some(data.error.clone()),
        _ => None,
    }) {
        return PromptCompletionStatus::Failed(format!(
            "run failed before prompt completion for {request_id}: {run_error}"
        ));
    }

    if let Some(cancel_reason) = events.iter().find_map(|event| match &event.payload {
        EventV1::TaskCancelled(data)
            if event_matches_request(event, request_id)
                && (prompt_task_id.is_none()
                    || prompt_task_id.is_some_and(|task_id| data.task_id == task_id)
                    || data.task_id == request_id) =>
        {
            Some(data.reason.clone())
        }
        _ => None,
    }) {
        return PromptCompletionStatus::Failed(format!(
            "prompt request {request_id} was cancelled: {cancel_reason}"
        ));
    }

    if events.iter().any(|event| match &event.payload {
        EventV1::TaskCompleted(data) => {
            prompt_task_id.is_some_and(|task_id| data.task_id == task_id)
                || data.task_id == request_id
        }
        _ => false,
    }) {
        return PromptCompletionStatus::Completed;
    }

    PromptCompletionStatus::Continue
}

#[cfg(test)]
fn prompt_task_id<'a>(events: &'a [EventEnvelopeV1], request_id: &str) -> Option<&'a str> {
    events.iter().find_map(|event| match &event.payload {
        EventV1::TaskScheduled(data)
            if event_matches_request(event, request_id)
                && data
                    .queue_key
                    .as_deref()
                    .is_some_and(|queue_key| queue_key.starts_with("provider_model:")) =>
        {
            Some(data.task_id.as_str())
        }
        _ => None,
    })
}

fn event_matches_request(event: &EventEnvelopeV1, request_id: &str) -> bool {
    event.correlation_id.as_deref() == Some(request_id)
}

#[cfg(test)]
fn has_provider_error_finish(events: &[EventEnvelopeV1], request_id: &str) -> bool {
    events.iter().any(|event| match &event.payload {
        EventV1::ProviderRequestFinished(data) => {
            data.request_id == request_id && data.finish_reason.eq_ignore_ascii_case("error")
        }
        _ => false,
    })
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

fn prompt_wait_timeout() -> Duration {
    let raw = env::var(WAIT_TIMEOUT_ENV).ok();
    parse_wait_timeout_ms(raw.as_deref())
}

fn parse_wait_timeout_ms(raw: Option<&str>) -> Duration {
    let Some(raw) = raw else {
        return DEFAULT_WAIT_TIMEOUT;
    };

    let Ok(ms) = raw.trim().parse::<u64>() else {
        return DEFAULT_WAIT_TIMEOUT;
    };

    if ms == 0 {
        return DEFAULT_WAIT_TIMEOUT;
    }

    Duration::from_millis(ms)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use harness_core::event::{
        ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestFinishedEvent,
        RunFailedEvent, TaskCancelledEvent, TaskCompletedEvent, TaskScheduleState,
        TaskScheduledEvent,
    };
    use harness_core::store::{
        EventEnvelopeWithoutSeqV1, EventStore, EventStoreError, EventStream, InMemoryEventStore,
    };

    use super::{
        evaluate_prompt_completion, has_provider_error_finish, parse_wait_timeout_ms,
        wait_for_prompt_completion, PromptCompletionStatus, DEFAULT_WAIT_TIMEOUT,
    };

    #[test]
    fn parse_wait_timeout_ms_uses_default_when_unset() {
        assert_eq!(parse_wait_timeout_ms(None), DEFAULT_WAIT_TIMEOUT);
    }

    #[test]
    fn parse_wait_timeout_ms_uses_default_when_invalid() {
        assert_eq!(
            parse_wait_timeout_ms(Some("not-a-number")),
            DEFAULT_WAIT_TIMEOUT
        );
        assert_eq!(parse_wait_timeout_ms(Some("0")), DEFAULT_WAIT_TIMEOUT);
        assert_eq!(parse_wait_timeout_ms(Some("   0  ")), DEFAULT_WAIT_TIMEOUT);
    }

    #[test]
    fn parse_wait_timeout_ms_parses_positive_milliseconds() {
        assert_eq!(
            parse_wait_timeout_ms(Some("1500")),
            Duration::from_millis(1500)
        );
        assert_eq!(
            parse_wait_timeout_ms(Some(" 60000 ")),
            Duration::from_millis(60_000)
        );
    }

    #[test]
    fn evaluate_prompt_completion_reports_cancelled_task_as_error() {
        let events = vec![event_with_correlation(
            EventV1::TaskCancelled(TaskCancelledEvent {
                task_id: "task_000001".to_string(),
                reason: "provider denied request".to_string(),
            }),
            Some("req_000001"),
        )];

        let status = evaluate_prompt_completion(&events, "req_000001");
        assert_eq!(
            status,
            PromptCompletionStatus::Failed(
                "prompt request req_000001 was cancelled: provider denied request".to_string()
            )
        );
    }

    #[test]
    fn evaluate_prompt_completion_waits_for_cancellation_after_provider_finish_error() {
        let events = vec![event(EventV1::ProviderRequestFinished(
            ProviderRequestFinishedEvent {
                request_id: "req_000001".to_string(),
                finish_reason: "error".to_string(),
                output_digest: None,
                usage: None,
            },
        ))];

        let status = evaluate_prompt_completion(&events, "req_000001");
        assert_eq!(status, PromptCompletionStatus::Continue);
    }

    #[test]
    fn evaluate_prompt_completion_waits_for_prompt_task_completion_after_provider_finish() {
        let events = vec![
            provider_task_scheduled_event("task_000001", "req_000001"),
            event(EventV1::ProviderRequestFinished(
                ProviderRequestFinishedEvent {
                    request_id: "req_000001".to_string(),
                    finish_reason: "done".to_string(),
                    output_digest: Some("abc123".to_string()),
                    usage: None,
                },
            )),
        ];

        let status = evaluate_prompt_completion(&events, "req_000001");
        assert_eq!(status, PromptCompletionStatus::Continue);
    }

    #[test]
    fn evaluate_prompt_completion_waits_for_tool_task_completion() {
        let events = vec![
            provider_task_scheduled_event("task_000001", "req_000001"),
            event_with_correlation(
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000002".to_string(),
                    result_summary: "tool ok".to_string(),
                    result_digest: "def456".to_string(),
                    metadata: None,
                }),
                Some("req_000001"),
            ),
        ];

        let status = evaluate_prompt_completion(&events, "req_000001");
        assert_eq!(status, PromptCompletionStatus::Continue);
    }

    #[test]
    fn evaluate_prompt_completion_ignores_cancelled_child_tool_task() {
        let events = vec![
            provider_task_scheduled_event("task_000001", "req_000001"),
            event_with_correlation(
                EventV1::TaskCancelled(TaskCancelledEvent {
                    task_id: "task_000002".to_string(),
                    reason: "tool execution failed: expected audit error".to_string(),
                }),
                Some("req_000001"),
            ),
            event_with_correlation(
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000001".to_string(),
                    result_summary: "ok".to_string(),
                    result_digest: "abc123".to_string(),
                    metadata: None,
                }),
                Some("req_000001"),
            ),
        ];

        let status = evaluate_prompt_completion(&events, "req_000001");
        assert_eq!(status, PromptCompletionStatus::Completed);
    }

    #[test]
    fn evaluate_prompt_completion_reports_success_for_prompt_task_completed() {
        let events = vec![
            provider_task_scheduled_event("task_000001", "req_000001"),
            event_with_correlation(
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000001".to_string(),
                    result_summary: "ok".to_string(),
                    result_digest: "abc123".to_string(),
                    metadata: None,
                }),
                Some("req_000001"),
            ),
        ];

        let status = evaluate_prompt_completion(&events, "req_000001");
        assert_eq!(status, PromptCompletionStatus::Completed);
    }

    #[test]
    fn evaluate_prompt_completion_prioritizes_run_failed() {
        let events = vec![event(EventV1::RunFailed(RunFailedEvent {
            error: "fatal".to_string(),
        }))];

        let status = evaluate_prompt_completion(&events, "req_000001");
        assert_eq!(
            status,
            PromptCompletionStatus::Failed(
                "run failed before prompt completion for req_000001: fatal".to_string()
            )
        );
    }

    #[test]
    fn has_provider_error_finish_detects_error_finish_reason() {
        let events = vec![event(EventV1::ProviderRequestFinished(
            ProviderRequestFinishedEvent {
                request_id: "req_000007".to_string(),
                finish_reason: "error".to_string(),
                output_digest: None,
                usage: None,
            },
        ))];

        assert!(has_provider_error_finish(&events, "req_000007"));
        assert!(!has_provider_error_finish(&events, "req_000008"));
    }

    #[test]
    fn evaluate_prompt_completion_still_supports_task_id_equals_request_id_fallback() {
        let events = vec![event(EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "req_000123".to_string(),
            result_summary: "ok".to_string(),
            result_digest: "abc123".to_string(),
            metadata: None,
        }))];

        let status = evaluate_prompt_completion(&events, "req_000123");
        assert_eq!(status, PromptCompletionStatus::Completed);
    }

    #[tokio::test]
    async fn wait_for_prompt_completion_subscribes_once_and_streams_new_events() {
        let store = Arc::new(CountingEventStore::new());
        for index in 0..256 {
            store
                .append(draft_event(
                    EventV1::TaskCompleted(TaskCompletedEvent {
                        task_id: format!("tool_task_{index:04}"),
                        result_summary: "ok".to_string(),
                        result_digest: format!("digest_{index:04}"),
                        metadata: None,
                    }),
                    Some("other_request"),
                ))
                .expect("append unrelated task completion");
        }

        let wait_store: Arc<dyn EventStore> = store.clone();
        let waiter = tokio::spawn(async move {
            wait_for_prompt_completion(wait_store, "req_000001", Duration::from_secs(1)).await
        });

        tokio::task::yield_now().await;

        store
            .append(draft_event(
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000001".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:default:gpt-4o-mini".to_string()),
                }),
                Some("req_000001"),
            ))
            .expect("append prompt task scheduled");
        store
            .append(draft_event(
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000001".to_string(),
                    result_summary: "ok".to_string(),
                    result_digest: "abc123".to_string(),
                    metadata: None,
                }),
                Some("req_000001"),
            ))
            .expect("append prompt task completed");

        assert_eq!(waiter.await.expect("join waiter"), Ok(()));
        assert_eq!(store.subscribe_calls(), 1);
        assert_eq!(store.replay_calls(), 0);
    }

    fn event(payload: EventV1) -> EventEnvelopeV1 {
        event_with_correlation(payload, None)
    }

    fn draft_event(payload: EventV1, correlation_id: Option<&str>) -> EventEnvelopeWithoutSeqV1 {
        EventEnvelopeWithoutSeqV1 {
            schema_version: 1,
            event_id: "evt_1".to_string(),
            run_id: "run_1".to_string(),
            mono_ms: 0,
            ts: None,
            actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
            correlation_id: correlation_id.map(ToOwned::to_owned),
            causation_id: None,
            stream_key: None,
            payload,
        }
    }

    fn provider_task_scheduled_event(task_id: &str, request_id: &str) -> EventEnvelopeV1 {
        event_with_correlation(
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: task_id.to_string(),
                state: TaskScheduleState::Started,
                queue_key: Some("provider_model:default:gpt-4o-mini".to_string()),
            }),
            Some(request_id),
        )
    }

    fn event_with_correlation(payload: EventV1, correlation_id: Option<&str>) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: 1,
            event_id: "evt_1".to_string(),
            seq: 1,
            run_id: "run_1".to_string(),
            mono_ms: 0,
            ts: None,
            actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
            correlation_id: correlation_id.map(ToOwned::to_owned),
            causation_id: None,
            stream_key: None,
            payload,
        }
    }

    struct CountingEventStore {
        inner: InMemoryEventStore,
        subscribe_calls: AtomicUsize,
        replay_calls: AtomicUsize,
    }

    impl CountingEventStore {
        fn new() -> Self {
            Self {
                inner: InMemoryEventStore::new(),
                subscribe_calls: AtomicUsize::new(0),
                replay_calls: AtomicUsize::new(0),
            }
        }

        fn subscribe_calls(&self) -> usize {
            self.subscribe_calls.load(Ordering::SeqCst)
        }

        fn replay_calls(&self) -> usize {
            self.replay_calls.load(Ordering::SeqCst)
        }
    }

    impl EventStore for CountingEventStore {
        fn append(
            &self,
            envelope: EventEnvelopeWithoutSeqV1,
        ) -> Result<EventEnvelopeV1, EventStoreError> {
            self.inner.append(envelope)
        }

        fn replay(&self, from_seq: u64) -> Result<EventStream, EventStoreError> {
            self.replay_calls.fetch_add(1, Ordering::SeqCst);
            self.inner.replay(from_seq)
        }

        fn subscribe(&self, from_seq: u64) -> Result<EventStream, EventStoreError> {
            self.subscribe_calls.fetch_add(1, Ordering::SeqCst);
            self.inner.subscribe(from_seq)
        }
    }
}
