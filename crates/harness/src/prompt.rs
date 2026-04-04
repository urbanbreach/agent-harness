use std::env;
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
use harness_core::proj::inspect_resume_plan;
use harness_core::redact::DefaultRedactor;
use harness_tui::load_events_from_run_dir;
use uuid::Uuid;

use crate::recovery::{
    inspect_session_recovery, latest_run_name, resolve_session_run_dir, select_resume_agent_id,
};
use crate::{bootstrap, logging};

const DEFAULT_WAIT_TIMEOUT: Duration = Duration::from_secs(30);
const WAIT_TIMEOUT_ENV: &str = "HARNESS_PROMPT_WAIT_TIMEOUT_MS";
const PROVIDER_ERROR_REASON_GRACE: Duration = Duration::from_secs(2);

#[derive(Debug, Args, Clone)]
pub struct PromptCommand {
    #[arg(long)]
    pub text: String,

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

    let config_bytes = fs::read(&explicit_config).map_err(|err| {
        format!(
            "failed to read config file {}: {err}",
            explicit_config.display()
        )
    })?;
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

    let wait_timeout = prompt_wait_timeout();
    let wait_result = wait_for_prompt_completion(&run.events_path, &request_id, wait_timeout).await;
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

    logging::init_logging(&settings.config, &run.artifacts_dir)?;

    let request_id = coordinator
        .request_agent_turn(user_actor(), resume_agent_id, cmd.text.clone())
        .await
        .map_err(|err| err.to_string())?;

    let wait_timeout = prompt_wait_timeout();
    let wait_result = wait_for_prompt_completion(&run.events_path, &request_id, wait_timeout).await;
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
    let mut provider_error_seen_at: Option<Instant> = None;

    loop {
        let events = load_events(events_path)?;

        if has_provider_error_finish(&events, request_id) && provider_error_seen_at.is_none() {
            provider_error_seen_at = Some(Instant::now());
        }

        match evaluate_prompt_completion(&events, request_id) {
            PromptCompletionStatus::Continue => {}
            PromptCompletionStatus::Completed => return Ok(()),
            PromptCompletionStatus::Failed(error) => return Err(error),
        }

        if let Some(seen_at) = provider_error_seen_at {
            if Instant::now().saturating_duration_since(seen_at) >= PROVIDER_ERROR_REASON_GRACE {
                return Err(format!(
                    "prompt request {request_id} finished with provider error"
                ));
            }
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for ProviderRequestFinished or TaskCompleted for {request_id}"
            ));
        }

        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PromptCompletionStatus {
    Continue,
    Completed,
    Failed(String),
}

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

fn has_provider_error_finish(events: &[EventEnvelopeV1], request_id: &str) -> bool {
    events.iter().any(|event| match &event.payload {
        EventV1::ProviderRequestFinished(data) => {
            data.request_id == request_id && data.finish_reason.eq_ignore_ascii_case("error")
        }
        _ => false,
    })
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
    use std::time::Duration;

    use harness_core::event::{
        ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestFinishedEvent,
        RunFailedEvent, TaskCancelledEvent, TaskCompletedEvent, TaskScheduleState,
        TaskScheduledEvent,
    };

    use super::{
        evaluate_prompt_completion, has_provider_error_finish, parse_wait_timeout_ms,
        PromptCompletionStatus, DEFAULT_WAIT_TIMEOUT,
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

    fn event(payload: EventV1) -> EventEnvelopeV1 {
        event_with_correlation(payload, None)
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
}
