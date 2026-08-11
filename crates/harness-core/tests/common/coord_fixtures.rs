use harness_core::UnwrapOrAbort;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use harness_core::agent::{
    build_provider_context_messages, build_provider_tool_defs, stream_assistant_response_once,
    AgentModelRef, AgentModelSettings, AgentProfile, AgentRequest, AgentRuntimeEvent,
    ProviderBoundaryContext, ProviderContext, ProviderContextCheckpoint,
    StreamAssistantResponseOnceRequest,
};
use harness_core::clock::FakeClock;
use harness_core::config::{
    CompactionRuntimeConfig, HookLifecycleEvent, HookRuntimeConfig, HooksConfig,
    LifecycleHookConfig, PermissionMode, ShellAllowlist,
};
use harness_core::coord::{
    spawn_coordinator, ChildTaskRequestMetadata, CoordinatorConfig, CoordinatorError,
    CoordinatorHandle, JobOutcome, JobProgressKind, ManualCompactionOutcome, RunInfo,
};
use harness_core::event::{
    ActorKind, AgentSpawnedEvent, EventActor, EventEnvelopeV1, EventV1, ExecutionTimingMetadata,
    HookExecutionMetadata, HookExecutionStatus, PermissionDecision as EventPermissionDecision,
    PermissionRequestedEvent, PermissionResolvedEvent, ProviderRequestStartedEvent,
    ProviderRequestStartedMetadata, RunFinishedEvent, RunStartedEvent, TaskCancelledEvent,
    TaskCompletedEvent, TaskCompletionMetadata, TaskLineageMetadata, TaskResultLateEvent,
    TaskScheduleState, TaskScheduledEvent, ToolCallFinishedEvent, ToolCallMetadata,
    ToolCallRequestedEvent, ToolCallStatus, SCHEMA_VERSION,
};
use harness_core::perm::{PermissionDecision as RuntimePermissionDecision, PermissionPolicy};
use harness_core::proj::{inspect_resume_plan, ChildSessionTerminalState, LifecycleSegmentStatus};
use harness_core::redact::DefaultRedactor;
use harness_core::store::EventStoreError;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult};
use harness_providers::mock::{request_digest, MockProvider};
use harness_providers::{
    CompletionMessage, CompletionRequest, CompletionUsage, MessageRole, Provider,
    ProviderErrorCategory, ProviderEventStream, ProviderStreamEvent,
    ProviderStreamFinishedMetadata, ProviderStreamStartMetadata, ProviderStreamThinkingMetadata,
};
use serde_json::json;
use tokio::sync::Notify;
use tokio_stream::StreamExt;

#[path = "mod.rs"]
mod common;
use common::load_events;

#[path = "coord_fixtures/provider_tools.rs"]
mod provider_tools;
use self::provider_tools::*;

async fn deterministic_runs_suppress_live_hook_execution() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let hook_output_path = temp_dir.path().join("deterministic-hook-side-effect.txt");
    let hook_runtime_config = HookRuntimeConfig {
        hooks: HooksConfig {
            lifecycle: vec![LifecycleHookConfig {
                id: Some("tool-finish-suppressed".to_string()),
                event: HookLifecycleEvent::ToolCallFinished,
                command: vec![
                    "bash".to_string(),
                    "-lc".to_string(),
                    "printf '%s|%s' \"$HARNESS_HOOK_EVENT\" \"$HARNESS_HOOK_TOOL_ID\" > \"$HOOK_OUTPUT_PATH\""
                        .to_string(),
                ],
                cwd: Some(".".to_string()),
                timeout_ms: 4_000,
                critical: false,
                env: BTreeMap::from([(
                    "HOOK_OUTPUT_PATH".to_string(),
                    hook_output_path.display().to_string(),
                )]),
            }],
        },
        shell_allowlist: ShellAllowlist {
            executables: vec!["bash".to_string()],
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        },
        suppress_execution: true,
    };

    let clock = Arc::new(FakeClock::new());
    let coordinator = test_tool_lifecycle_coordinator_with_hook_runtime(
        temp_dir.path(),
        clock,
        lifecycle_tool_registry(Arc::new(Notify::new())),
        Duration::from_millis(50),
        15_000,
        5,
        1,
        hook_runtime_config,
    );

    let run = coordinator
        .start_run(
            "deterministic_runs_suppress_live_hook_execution",
            temp_dir.path().to_path_buf(),
        )
        .await
        .unwrap_or_abort();

    let tool_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "true"}),
        )
        .await
        .unwrap_or_abort();

    tokio::task::yield_now().await;
    coordinator.stop_run().await.unwrap_or_abort();

    assert!(
        !hook_output_path.exists(),
        "deterministic suppression should prevent live hook side effects"
    );

    let events = load_events(&run.events_path);
    let tool_finished = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallFinished(data) if data.tool_call_id.as_str() == tool_call_id => {
                Some(data)
            }
            _ => None,
        })
        .unwrap_or_abort();
    let hook_executions = tool_finished
        .metadata
        .as_ref()
        .map(|metadata| metadata.hook_executions.clone())
        .unwrap_or_abort();
    assert_eq!(hook_executions.len(), 1);
    assert_eq!(hook_executions[0].hook_name, "tool-finish-suppressed");
    assert_eq!(hook_executions[0].status, HookExecutionStatus::Skipped);
    assert_eq!(
        hook_executions[0].output_summary.as_deref(),
        Some("suppressed during deterministic execution")
    );
}

fn test_coordinator(session_dir: &Path) -> CoordinatorHandle {
    let mut config = CoordinatorConfig::new(session_dir.to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 32;
    let clock = Arc::new(FakeClock::new());
    let redactor = Arc::new(DefaultRedactor::default());
    spawn_coordinator(config, clock, redactor)
}

fn structured_model_summary(goal: &str, next_step: &str) -> String {
    format!(
        "## Goal\n- {goal}\n\n## Constraints\n- Preserve Harness checkpoint structure.\n\n## Progress\n- Done: older turns were summarized by the configured compaction model.\n- In progress: continue from preserved recent context.\n- Blocked: (none)\n\n## Key Decisions\n- Use the model summary because it passed Harness validation.\n\n## Next Steps\n1. {next_step}\n\n## Critical Context\n- This is a structured checkpoint update.\n- Source facts: model summary retained compacted turn facts.\n- Relevant files/artifacts: (none)"
    )
}

fn structured_split_model_summary(goal: &str, next_step: &str, split_prefix: &str) -> String {
    format!(
        "## Goal\n- {goal}\n\n## Constraints\n- Preserve Harness checkpoint structure.\n\n## Progress\n- Done: older turns were summarized by the configured compaction model.\n- In progress: continue from preserved split-turn suffix.\n- Blocked: (none)\n\n## Key Decisions\n- Use the model split-prefix summary because it passed Harness validation.\n\n## Next Steps\n1. {next_step}\n\n## Critical Context\n- Split prefix summary: {split_prefix}; the provider-visible suffix follows this checkpoint as recent context.\n- Source facts: split prefix summary: {split_prefix}\n- Relevant files/artifacts: (none)"
    )
}

fn test_compaction_excerpt(text: &str) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= 240 {
        return normalized;
    }

    let mut truncated = normalized.chars().take(240).collect::<String>();
    truncated.push('…');
    truncated
}

fn manual_checkpoint(run: &RunInfo, events: &[EventEnvelopeV1]) -> ProviderContextCheckpoint {
    checkpoint_for_trigger(run, events, "manual")
}

fn overflow_checkpoint(run: &RunInfo, events: &[EventEnvelopeV1]) -> ProviderContextCheckpoint {
    checkpoint_for_trigger(run, events, "overflow_retry")
}

#[allow(
    deprecated,
    reason = "deprecated compaction event variants kept for backward compatibility tests"
)]
fn checkpoint_for_trigger(
    run: &RunInfo,
    events: &[EventEnvelopeV1],
    trigger_reason: &str,
) -> ProviderContextCheckpoint {
    let written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) if payload.trigger_reason == trigger_reason => {
                Some(payload.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();
    let checkpoint_path = run.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(&checkpoint_path).unwrap_or_abort();
    serde_json::from_str(&checkpoint_body).unwrap_or_abort()
}

fn test_agent_coordinator(session_dir: &Path, delay: Duration) -> CoordinatorHandle {
    test_agent_coordinator_with_provider(
        session_dir,
        Arc::new(SlowMockProvider {
            inner: test_mock_provider(),
            delay,
        }),
        1,
    )
}

fn test_agent_coordinator_with_provider(
    session_dir: &Path,
    provider: Arc<dyn Provider>,
    provider_model_concurrency: usize,
) -> CoordinatorHandle {
    test_agent_coordinator_with_provider_and_compaction(
        session_dir,
        provider,
        provider_model_concurrency,
        CompactionRuntimeConfig::default(),
    )
}

fn test_agent_coordinator_with_provider_and_compaction(
    session_dir: &Path,
    provider: Arc<dyn Provider>,
    provider_model_concurrency: usize,
    compaction: CompactionRuntimeConfig,
) -> CoordinatorHandle {
    test_agent_coordinator_with_provider_compaction_and_hooks(
        session_dir,
        provider,
        provider_model_concurrency,
        compaction,
        HookRuntimeConfig::default(),
    )
}

fn test_agent_coordinator_with_provider_compaction_and_hooks(
    session_dir: &Path,
    provider: Arc<dyn Provider>,
    provider_model_concurrency: usize,
    compaction: CompactionRuntimeConfig,
    hook_runtime_config: HookRuntimeConfig,
) -> CoordinatorHandle {
    let mut config = CoordinatorConfig::new(session_dir.to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.provider_model_concurrency = provider_model_concurrency;
    config.provider = provider;
    config.compaction = compaction;
    config.hook_runtime_config = hook_runtime_config;
    config.agent_profiles = agent_profiles();

    let clock = Arc::new(FakeClock::new());
    let redactor = Arc::new(DefaultRedactor::default());
    spawn_coordinator(config, clock, redactor)
}

fn allow_all_permission_policy() -> PermissionPolicy {
    PermissionPolicy::allow_all()
}

fn shell_only_permission_policy() -> PermissionPolicy {
    PermissionPolicy::new(
        PermissionMode::Deny,
        PermissionMode::Allow,
        PermissionMode::Deny,
    )
}

fn ask_shell_permission_policy() -> PermissionPolicy {
    PermissionPolicy::new(
        PermissionMode::Deny,
        PermissionMode::Ask,
        PermissionMode::Deny,
    )
    .with_ask_timeout_ms(5_000)
}

fn deny_all_permission_policy() -> PermissionPolicy {
    PermissionPolicy::new(
        PermissionMode::Deny,
        PermissionMode::Deny,
        PermissionMode::Deny,
    )
}

fn test_resume_coordinator(session_dir: &Path) -> CoordinatorHandle {
    test_resume_coordinator_with_provider(session_dir, Arc::new(test_mock_provider()))
}

fn test_resume_coordinator_with_provider(
    session_dir: &Path,
    provider: Arc<dyn Provider>,
) -> CoordinatorHandle {
    let mut config = CoordinatorConfig::new(session_dir.to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.permission_policy = ask_shell_permission_policy();
    config.tool_registry = test_tool_registry();
    config.provider = provider;
    config.agent_profiles = agent_profiles();

    let clock = Arc::new(FakeClock::new());
    let redactor = Arc::new(DefaultRedactor::default());
    spawn_coordinator(config, clock, redactor)
}

fn test_tool_registry() -> Arc<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(TestShellTool));
    Arc::new(registry)
}

fn named_tool_registry(tools: Vec<NamedShellTool>) -> Arc<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    for tool in tools {
        registry.register(Arc::new(tool));
    }
    Arc::new(registry)
}

fn test_agent_tool_coordinator(
    session_dir: &Path,
    provider: Arc<dyn Provider>,
    tool_registry: Arc<ToolRegistry>,
    permission_policy: PermissionPolicy,
    alpha_toolset: Vec<String>,
    alpha_max_iters: usize,
) -> CoordinatorHandle {
    test_agent_tool_coordinator_with_compaction(
        session_dir,
        provider,
        tool_registry,
        permission_policy,
        alpha_toolset,
        alpha_max_iters,
        CompactionRuntimeConfig::default(),
    )
}

fn test_agent_tool_coordinator_with_compaction(
    session_dir: &Path,
    provider: Arc<dyn Provider>,
    tool_registry: Arc<ToolRegistry>,
    permission_policy: PermissionPolicy,
    alpha_toolset: Vec<String>,
    alpha_max_iters: usize,
    compaction: CompactionRuntimeConfig,
) -> CoordinatorHandle {
    let mut config = CoordinatorConfig::new(session_dir.to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.provider_model_concurrency = 1;
    config.provider = provider;
    config.tool_registry = tool_registry;
    config.permission_policy = permission_policy;
    config.compaction = compaction;
    config.agent_profiles = agent_profiles();
    if let Some(profile) = config.agent_profiles.get_mut("default") {
        profile.toolset = alpha_toolset.clone();
        profile.max_iters = Some(alpha_max_iters);
    }
    if let Some(profile) = config.agent_profiles.get_mut("alpha") {
        profile.toolset = alpha_toolset;
        profile.max_iters = Some(alpha_max_iters);
    }

    let clock = Arc::new(FakeClock::new());
    let redactor = Arc::new(DefaultRedactor::default());
    spawn_coordinator(config, clock, redactor)
}

fn lifecycle_tool_registry(blocking_release: Arc<Notify>) -> Arc<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(TestShellTool));
    registry.register(Arc::new(FailingShellTool));
    registry.register(Arc::new(BlockingShellTool {
        release: blocking_release,
    }));
    Arc::new(registry)
}

fn test_tool_lifecycle_coordinator(
    session_dir: &Path,
    clock: Arc<FakeClock>,
    tool_registry: Arc<ToolRegistry>,
    provider_delay: Duration,
    stale_timeout_ms: u64,
    watchdog_tick_ms: u64,
    tool_concurrency: usize,
) -> CoordinatorHandle {
    let mut config = CoordinatorConfig::new(session_dir.to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.tool_registry = tool_registry;
    config.provider_model_concurrency = 1;
    config.tool_concurrency = tool_concurrency;
    config.stale_timeout_ms = stale_timeout_ms;
    config.watchdog_tick_ms = watchdog_tick_ms;
    config.provider = Arc::new(SlowMockProvider {
        inner: test_mock_provider(),
        delay: provider_delay,
    });
    config.agent_profiles = agent_profiles();
    if let Some(profile) = config.agent_profiles.get_mut("alpha") {
        profile.toolset = vec![
            "shell.run".to_string(),
            "shell.fail".to_string(),
            "shell.block".to_string(),
        ];
    }

    let redactor = Arc::new(DefaultRedactor::default());
    spawn_coordinator(config, clock, redactor)
}

#[expect(
    clippy::too_many_arguments,
    reason = "test helper keeps lifecycle coordinator knobs explicit for focused hook/runtime scenarios"
)]
fn test_tool_lifecycle_coordinator_with_hook_runtime(
    session_dir: &Path,
    clock: Arc<FakeClock>,
    tool_registry: Arc<ToolRegistry>,
    provider_delay: Duration,
    stale_timeout_ms: u64,
    watchdog_tick_ms: u64,
    tool_concurrency: usize,
    hook_runtime_config: HookRuntimeConfig,
) -> CoordinatorHandle {
    let mut config = CoordinatorConfig::new(session_dir.to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.tool_registry = tool_registry;
    config.provider_model_concurrency = 1;
    config.tool_concurrency = tool_concurrency;
    config.stale_timeout_ms = stale_timeout_ms;
    config.watchdog_tick_ms = watchdog_tick_ms;
    config.provider = Arc::new(SlowMockProvider {
        inner: test_mock_provider(),
        delay: provider_delay,
    });
    config.agent_profiles = agent_profiles();
    config.hook_runtime_config = hook_runtime_config;
    if let Some(profile) = config.agent_profiles.get_mut("alpha") {
        profile.toolset = vec![
            "shell.run".to_string(),
            "shell.fail".to_string(),
            "shell.block".to_string(),
        ];
    }

    let redactor = Arc::new(DefaultRedactor::default());
    spawn_coordinator(config, clock, redactor)
}

fn supervisor_actor() -> EventActor {
    EventActor::new(ActorKind::Supervisor, Some("agent_supervisor".to_string()))
}

fn tool_task_ids(events: &[EventEnvelopeV1]) -> BTreeSet<String> {
    events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::TaskScheduled(data)
                if data
                    .queue_key
                    .as_deref()
                    .is_some_and(|queue_key| queue_key.starts_with("tool:")) =>
            {
                Some(data.task_id.to_string())
            }
            _ => None,
        })
        .collect()
}

fn assert_task_event_context(
    event: &EventEnvelopeV1,
    expected_actor: &EventActor,
    expected_correlation: &str,
) {
    assert_eq!(
        &event.actor, expected_actor,
        "unexpected actor for event seq {}",
        event.seq
    );
    assert_eq!(
        event.correlation_id.as_deref(),
        Some(expected_correlation),
        "unexpected correlation for event seq {}",
        event.seq
    );
}

fn provider_started_request_ids(events: &[EventEnvelopeV1]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::ProviderRequestStarted(_) => event.correlation_id.clone(),
            _ => None,
        })
        .collect()
}

fn task_schedule_states_for_request(
    events: &[EventEnvelopeV1],
    request_id: &str,
) -> Vec<TaskScheduleState> {
    events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::TaskScheduled(data) if event.correlation_id.as_deref() == Some(request_id) => {
                Some(data.state)
            }
            _ => None,
        })
        .collect()
}

async fn wait_for_events<F>(
    events_path: &Path,
    timeout: Duration,
    mut predicate: F,
) -> Vec<EventEnvelopeV1>
where
    F: FnMut(&[EventEnvelopeV1]) -> bool,
{
    tokio::time::timeout(timeout, async {
        loop {
            let events = load_events(events_path);
            if predicate(&events) {
                return events;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_abort()
}

fn write_resumable_history_fixture(session_dir: &Path, run_id: &str) {
    let events = vec![
        resume_fixture_event(
            run_id,
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".into(),
                workspace_root: "/workspace/project".to_string(),
            }),
        ),
        resume_fixture_event(
            run_id,
            2,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_000001".to_string(),
                profile: "default".to_string(),
                parent_agent_id: None,
            }),
        ),
        resume_fixture_event_with_actor_and_correlation(
            run_id,
            3,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("req_000001"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_000001".into(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "first question".to_string(),
                request_digest: "digest-req-1".to_string(),
                metadata: None,
            }),
        ),
        resume_fixture_event_with_actor_and_correlation(
            run_id,
            4,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("req_000001"),
            EventV1::ProviderStreamDelta(harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_000001".into(),
                delta: "first answer".to_string(),
            }),
        ),
        resume_fixture_event_with_actor_and_correlation(
            run_id,
            5,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("req_000001"),
            EventV1::ProviderRequestFinished(harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_000001".into(),
                finish_reason: "done".to_string(),
                output_digest: Some("digest-out-1".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
        resume_fixture_event_with_actor_and_correlation(
            run_id,
            6,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("req_000001"),
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: "task_000001".to_string().into(),
                result_summary: "first answer".to_string(),
                result_digest: "digest-task-1".to_string(),
                metadata: None,
            }),
        ),
        resume_fixture_event(
            run_id,
            7,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "segment complete".to_string(),
            }),
        ),
    ];
    let _ = write_resume_fixture(session_dir, run_id, &events);
}

fn write_resumable_multi_turn_history_fixture(session_dir: &Path, run_id: &str) {
    let events = vec![
        resume_fixture_event(
            run_id,
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".into(),
                workspace_root: "/workspace/project".to_string(),
            }),
        ),
        resume_fixture_event(
            run_id,
            2,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_000001".to_string(),
                profile: "default".to_string(),
                parent_agent_id: None,
            }),
        ),
        resume_fixture_event_with_actor_and_correlation(
            run_id,
            3,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("req_000001"),
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_000001".to_string().into(),
                state: TaskScheduleState::Started,
                queue_key: Some("provider_model:mock:model-1".to_string()),
            }),
        ),
        resume_fixture_event_with_actor_and_correlation(
            run_id,
            4,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("req_000001"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_000001".into(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "first question".to_string(),
                request_digest: "digest-req-1".to_string(),
                metadata: None,
            }),
        ),
        resume_fixture_event_with_actor_and_correlation(
            run_id,
            5,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("req_000001"),
            EventV1::ProviderStreamDelta(harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_000001".into(),
                delta: "calling tool".to_string(),
            }),
        ),
        resume_fixture_event_with_actor_and_correlation(
            run_id,
            6,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("req_000001"),
            EventV1::ProviderRequestFinished(harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_000001".into(),
                finish_reason: "done".to_string(),
                output_digest: Some("digest-out-1".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
        resume_fixture_event_with_actor_and_correlation(
            run_id,
            7,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("req_000001"),
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_000002".to_string().into(),
                state: TaskScheduleState::Started,
                queue_key: Some("tool:edit.hashline_apply".to_string()),
            }),
        ),
        resume_fixture_event_with_actor_and_correlation(
            run_id,
            8,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("req_000001"),
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: "task_000002".to_string().into(),
                result_summary: "tool output".to_string(),
                result_digest: "digest-tool-task".to_string(),
                metadata: None,
            }),
        ),
        resume_fixture_event_with_actor_and_correlation(
            run_id,
            9,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("req_000002"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_000002".into(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "tool result + continue".to_string(),
                request_digest: "digest-req-2".to_string(),
                metadata: None,
            }),
        ),
        resume_fixture_event_with_actor_and_correlation(
            run_id,
            10,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("req_000002"),
            EventV1::ProviderStreamDelta(harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_000002".into(),
                delta: "first final answer".to_string(),
            }),
        ),
        resume_fixture_event_with_actor_and_correlation(
            run_id,
            11,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("req_000002"),
            EventV1::ProviderRequestFinished(harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_000002".into(),
                finish_reason: "done".to_string(),
                output_digest: Some("digest-out-2".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
        resume_fixture_event_with_actor_and_correlation(
            run_id,
            12,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("req_000001"),
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: "task_000001".to_string().into(),
                result_summary: "first final answer".to_string(),
                result_digest: "digest-task-1".to_string(),
                metadata: None,
            }),
        ),
        resume_fixture_event(
            run_id,
            13,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "segment complete".to_string(),
            }),
        ),
    ];
    let _ = write_resume_fixture(session_dir, run_id, &events);
}

fn write_resume_fixture(session_dir: &Path, run_id: &str, events: &[EventEnvelopeV1]) -> PathBuf {
    let run_dir = session_dir.join(run_id);
    fs::create_dir_all(run_dir.join("artifacts")).unwrap_or_abort();

    let mut body = String::new();
    for event in events {
        let line = serde_json::to_string(event).unwrap_or_abort();
        body.push_str(&line);
        body.push('\n');
    }

    let events_path = run_dir.join("events.jsonl");
    fs::write(&events_path, body).unwrap_or_abort();
    events_path
}

fn resume_fixture_event(run_id: &str, seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    resume_fixture_event_with_actor_and_correlation(
        run_id,
        seq,
        EventActor::new(ActorKind::System, Some("coordinator".to_string())),
        None,
        payload,
    )
}

fn resume_fixture_event_with_actor_and_correlation(
    run_id: &str,
    seq: u64,
    actor: EventActor,
    correlation_id: Option<&str>,
    payload: EventV1,
) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: run_id.to_string().into(),
        mono_ms: seq,
        ts: None,
        actor,
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some(format!("run:{run_id}")),
        payload,
    }
}

fn agent_profiles() -> BTreeMap<String, AgentProfile> {
    let mut profiles = BTreeMap::new();
    profiles.insert(
        "default".to_string(),
        AgentProfile { name: "default".to_string(), model_ref: "mock:model-1".to_string(),
        model_ref_explicit: true,
        system_prompt: "default-prompt".to_string(),
        cache_retention: Default::default(),
        max_iters: Some(12),
        temperature: Some(0.0),
        tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
        toolset: vec![],
        permission_ruleset: Vec::new(), },
    );
    profiles.insert(
        "alpha".to_string(),
        AgentProfile { name: "alpha".to_string(), model_ref: "mock:model-1".to_string(),
        model_ref_explicit: true,
        system_prompt: "alpha-prompt".to_string(),
        cache_retention: Default::default(),
        max_iters: Some(12),
        temperature: Some(0.0),
        tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
        toolset: vec![],
        permission_ruleset: Vec::new(), },
    );
    profiles.insert(
        "beta".to_string(),
        AgentProfile { name: "beta".to_string(), model_ref: "mock:model-1".to_string(),
        model_ref_explicit: true,
        system_prompt: "beta-prompt".to_string(),
        cache_retention: Default::default(),
        max_iters: Some(12),
        temperature: Some(0.0),
        tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
        toolset: vec![],
        permission_ruleset: Vec::new(), },
    );
    profiles
}
