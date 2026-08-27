use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use harness_core::coord::{
    LifecycleHookCommandExecutor, LifecycleHookCommandInvocation,
    LifecycleHookCommandOutput, TokioLifecycleHookCommandExecutor,
};
use harness_core::event::{ProviderRequestFinishedEvent, ToolCallStartedEvent};
use harness_core::store::{EventStoreError, EventStoreOpener, JsonlFileEventStore};

struct CountingEventStoreOpener {
    opens: std::sync::Arc<AtomicUsize>,
}

impl EventStoreOpener for CountingEventStoreOpener {
    fn open(
        &self,
        session_dir: &std::path::Path,
        run_id: &str,
        deterministic: bool,
    ) -> Result<JsonlFileEventStore, EventStoreError> {
        JsonlFileEventStore::open(session_dir, run_id, deterministic)
    }

    fn open_existing(
        &self,
        session_dir: &std::path::Path,
        run_id: &str,
        deterministic: bool,
    ) -> Result<JsonlFileEventStore, EventStoreError> {
        let store = JsonlFileEventStore::open_existing(session_dir, run_id, deterministic)?;
        self.opens.fetch_add(1, Ordering::SeqCst);
        Ok(store)
    }
}

#[derive(Clone)]
struct CountingLifecycleHookExecutor {
    calls: std::sync::Arc<AtomicUsize>,
}

#[async_trait]
impl LifecycleHookCommandExecutor for CountingLifecycleHookExecutor {
    async fn execute(
        &self,
        invocation: LifecycleHookCommandInvocation,
    ) -> Result<LifecycleHookCommandOutput, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        TokioLifecycleHookCommandExecutor.execute(invocation).await
    }
}

fn coordinator_for_resume_counter_test(
    session_dir: &std::path::Path,
    provider: std::sync::Arc<dyn Provider>,
    tool_calls: std::sync::Arc<AtomicUsize>,
    hook_calls: std::sync::Arc<AtomicUsize>,
) -> CoordinatorHandle {
    let mut config = CoordinatorConfig::new(session_dir.to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.provider_model_concurrency = 1;
    config.provider = provider;

    let mut registry = ToolRegistry::new();
    registry.register(std::sync::Arc::new(CountingShellTool { calls: tool_calls }));
    config.tool_registry = std::sync::Arc::new(registry);
    config.permission_policy = allow_all_permission_policy();
    config.agent_profiles = agent_profiles();
    config
        .agent_profiles
        .get_mut("default")
        .unwrap_or_abort()
        .toolset = vec!["shell.run".to_string()];
    config.hook_runtime_config = HookRuntimeConfig {
        hooks: HooksConfig {
            lifecycle: vec![LifecycleHookConfig {
                id: Some("tool-finished-counter".to_string()),
                event: HookLifecycleEvent::ToolCallFinished,
                command: vec!["true".to_string()],
                cwd: None,
                timeout_ms: 5_000,
                critical: false,
                env: BTreeMap::new(),
            }],
        },
        shell_allowlist: ShellAllowlist {
            executables: vec!["true".to_string()],
            ..ShellAllowlist::default()
        },
        suppress_execution: false,
    };
    config.hook_command_executor = std::sync::Arc::new(CountingLifecycleHookExecutor {
        calls: hook_calls,
    });

    spawn_coordinator(
        config,
        std::sync::Arc::new(FakeClock::new()),
        std::sync::Arc::new(DefaultRedactor::default()),
    )
}

fn write_counter_history_fixture(session_dir: &std::path::Path, run_id: &str) {
    let worker = EventActor::new(ActorKind::Worker, Some("agent_000001".to_string()));
    let hook = HookExecutionMetadata {
        hook_name: "historical-tool-hook".to_string(),
        status: HookExecutionStatus::Succeeded,
        hook_event: Some("tool_call_finished".to_string()),
        command_digest: Some("digest-historical-hook".to_string()),
        output_digest: Some("digest-historical-hook-output".to_string()),
        output_summary: Some("historical hook completed".to_string()),
        duration_ms: Some(1),
    };
    let metadata = ToolCallMetadata {
        canonical_tool_id: Some("shell.run".to_string()),
        alias_source_tool_id: None,
        lineage: None,
        artifact_refs: Vec::new(),
        timing: None,
        hook_executions: vec![hook],
    };
    write_resume_fixture(
        session_dir,
        run_id,
        &[
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
                worker.clone(),
                Some("req_000001"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".into(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "historical tool prompt".to_string(),
                    request_digest: "digest-historical-request".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                4,
                worker.clone(),
                Some("req_000001"),
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000003".into(),
                    tool_id: "shell.run".to_string(),
                    args_summary: "{\"command\":\"printf historical\"}".to_string(),
                    args_digest: "digest-historical-args".to_string(),
                    metadata: Some(metadata.clone()),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                5,
                worker.clone(),
                Some("req_000001"),
                EventV1::ToolCallStarted(ToolCallStartedEvent {
                    tool_call_id: "toolcall_000003".into(),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                6,
                worker.clone(),
                Some("req_000001"),
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000003".into(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("historical tool output".to_string()),
                    output_digest: Some("digest-historical-output".to_string()),
                    output_json: Some(json!({"output":"historical"})),
                    metadata: Some(metadata),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                7,
                worker.clone(),
                Some("req_000001"),
                EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                    request_id: "req_000001".into(),
                    finish_reason: "tool_call".to_string(),
                    output_digest: Some("digest-historical-provider-output".to_string()),
                    usage: None,
                    metadata: None,
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                8,
                worker,
                Some("req_000001"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000001".to_string().into(),
                    result_summary: "historical tool output".to_string(),
                    result_digest: "digest-historical-task".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                9,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "segment complete".to_string(),
                }),
            ),
        ],
    );
}

fn write_interrupted_tool_history_fixture(session_dir: &std::path::Path, run_id: &str) {
    let worker = EventActor::new(ActorKind::Worker, Some("agent_000001".to_string()));
    write_resume_fixture(
        session_dir,
        run_id,
        &[
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
                worker.clone(),
                Some("req_000001"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000002".into(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "interrupted historical tool".to_string(),
                    request_digest: "digest-interrupted-request".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                4,
                worker.clone(),
                Some("req_000001"),
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000001".into(),
                    tool_id: "shell.run".to_string(),
                    args_summary: "{\"command\":\"printf must-not-run\"}".to_string(),
                    args_digest: "digest-interrupted-args".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                5,
                worker,
                Some("req_000001"),
                EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                    request_id: "req_000002".into(),
                    finish_reason: "tool_call".to_string(),
                    output_digest: Some("digest-interrupted-output".to_string()),
                    usage: None,
                    metadata: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                6,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "interrupted segment persisted".to_string(),
                }),
            ),
        ],
    );
}

#[tokio::test]
async fn resume_does_not_schedule_interrupted_historical_tool() {
    // arrange
    // act
    // assert
    // Given an event history ending after a tool request but before tool execution.
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_resume_interrupted_tool_inert";
    write_interrupted_tool_history_fixture(temp_dir.path(), run_id);
    let provider = CapturingProvider::new(Vec::<&str>::new());
    let tool_calls = std::sync::Arc::new(AtomicUsize::new(0));
    let hook_calls = std::sync::Arc::new(AtomicUsize::new(0));
    let coordinator = coordinator_for_resume_counter_test(
        temp_dir.path(),
        std::sync::Arc::new(provider.clone()),
        std::sync::Arc::clone(&tool_calls),
        std::sync::Arc::clone(&hook_calls),
    );

    // When the coordinator restores the run and receives no explicit continuation.
    coordinator
        .resume_run(run_id, "interactive")
        .await
        .unwrap_or_else(|error| panic!("interrupted fixture failed to resume: {error}"));

    // Then historical provider, tool, and hook work remains inert.
    assert!(provider.requests().is_empty());
    assert_eq!(tool_calls.load(Ordering::SeqCst), 0);
    assert_eq!(hook_calls.load(Ordering::SeqCst), 0);
    coordinator.stop_run().await.unwrap_or_abort();
}

#[tokio::test]
async fn resume_replay_provider_tool_hook_side_effect_counters_remain_zero() {
    // arrange
    // act
    // assert
    // Given: a resumable history with only completed historical work.
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_resume_side_effect_counters";
    write_counter_history_fixture(temp_dir.path(), run_id);
    let provider = CapturingProvider::new(vec!["continuation without a tool"]);
    let tool_calls = std::sync::Arc::new(AtomicUsize::new(0));
    let hook_calls = std::sync::Arc::new(AtomicUsize::new(0));
    let coordinator = coordinator_for_resume_counter_test(
        temp_dir.path(),
        std::sync::Arc::new(provider.clone()),
        std::sync::Arc::clone(&tool_calls),
        std::sync::Arc::clone(&hook_calls),
    );

    // When: restore the run, before asking for a new provider turn.
    let run = coordinator
        .resume_run(run_id, "interactive")
        .await
        .unwrap_or_else(|error| panic!("resume fixture failed: {error}"));

    // Then: replay is side-effect free across all three execution surfaces.
    let before = [
        provider.requests().len(),
        tool_calls.load(Ordering::SeqCst),
        hook_calls.load(Ordering::SeqCst),
    ];
    assert_eq!(before, [0, 0, 0]);

    // When: one explicit response-without-tool continuation is subscribed first.
    let store = coordinator.event_store().await.unwrap_or_abort();
    let mut events = store.subscribe(1).unwrap_or_abort();
    let request_id = coordinator
        .request_agent_turn(
            supervisor_actor(),
            "agent_000001",
            "continue without invoking a tool",
        )
        .await
        .unwrap_or_abort();
    await_turn_terminal(&mut events, &request_id).await;
    coordinator.stop_run().await.unwrap_or_abort();

    // Then: only the explicit provider continuation executed.
    let after = [
        provider.requests().len(),
        tool_calls.load(Ordering::SeqCst),
        hook_calls.load(Ordering::SeqCst),
    ];
    assert_eq!(after, [1, 0, 0]);
    eprintln!(
        "G007_TASK4 side_effect_counters run={} before={:?} after={:?} request_id={request_id} events_path={}",
        run.run_id,
        before,
        after,
        run.events_path.display()
    );
}

#[tokio::test]
async fn resumed_continuation_cache_hit_avoids_second_journal_reduction() {
    // arrange
    // act
    // assert
    // Given: one resumed turn has completed and advanced the in-memory canonical overlay.
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_resume_cache_hit";
    write_counter_history_fixture(temp_dir.path(), run_id);
    let provider = CapturingProvider::new(vec!["first continuation", "cached continuation"]);
    let coordinator = coordinator_for_resume_counter_test(
        temp_dir.path(),
        std::sync::Arc::new(provider.clone()),
        std::sync::Arc::new(AtomicUsize::new(0)),
        std::sync::Arc::new(AtomicUsize::new(0)),
    );
    let run = coordinator
        .resume_run(run_id, "interactive")
        .await
        .unwrap_or_abort();
    let store = coordinator.event_store().await.unwrap_or_abort();
    let mut events = store.subscribe(1).unwrap_or_abort();
    let first = coordinator
        .request_agent_turn(supervisor_actor(), "agent_000001", "first continuation")
        .await
        .unwrap_or_abort();
    await_turn_terminal(&mut events, &first).await;
    let detached_journal = run.run_dir.join("events.before-cache-hit.jsonl");
    std::fs::rename(&run.events_path, &detached_journal).unwrap_or_abort();
    std::fs::write(&run.events_path, "{complete-but-invalid}\n").unwrap_or_abort();

    // When: a second continuation starts after the journal path becomes unreadable as history.
    let second = coordinator
        .request_agent_turn(supervisor_actor(), "agent_000001", "cached continuation")
        .await
        .unwrap_or_abort();
    await_turn_terminal(&mut events, &second).await;

    // Then: the installed canonical baseline and appended-turn overlay serve the request.
    assert_eq!(provider.requests().len(), 2);
    assert!(provider.requests()[1]
        .messages
        .iter()
        .any(|message| message.content.contains("first continuation")));
    coordinator.stop_run().await.unwrap_or_abort();
}

#[tokio::test]
async fn resume_opens_one_writer_for_the_single_loaded_canonical_history() {
    // arrange
    // act
    // assert
    // Given: the event-store opener counts exclusive writer acquisition.
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_resume_single_reduction";
    write_counter_history_fixture(temp_dir.path(), run_id);
    let opens = std::sync::Arc::new(AtomicUsize::new(0));
    let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.provider = std::sync::Arc::new(CapturingProvider::new(Vec::<&str>::new()));
    config.agent_profiles = agent_profiles();
    config.event_store_opener = std::sync::Arc::new(CountingEventStoreOpener {
        opens: std::sync::Arc::clone(&opens),
    });
    let coordinator = spawn_coordinator(
        config,
        std::sync::Arc::new(FakeClock::new()),
        std::sync::Arc::new(DefaultRedactor::default()),
    );

    // When: resume derives both the operational plan and provider cache.
    let resumed = coordinator.resume_run(run_id, "interactive").await;

    // Then: resume acquires one writer while deriving its plan and provider cache once.
    assert!(resumed.is_ok(), "single-load resume failed: {resumed:?}");
    assert_eq!(opens.load(Ordering::SeqCst), 1);
    coordinator.stop_run().await.unwrap_or_abort();
}
