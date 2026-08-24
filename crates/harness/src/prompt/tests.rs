use crate::UnwrapOrAbort;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use harness_core::auth::{
    AuthProviderId, CredentialClock, CredentialStore, StoredCredential, SystemCredentialClock,
};
use harness_core::event::{
    ActorKind, AgentSpawnedEvent, AssistantMessageFinishedEvent, EventActor, EventEnvelopeV1,
    EventV1, LiveEventEnvelope, LiveEventV1, ProviderRequestFinishedEvent,
    ProviderRequestStartedEvent, RunFailedEvent, RunFinishedEvent, RunStartedEvent,
    TaskCancelledEvent, TaskCompletedEvent, TaskCompletionMetadata, TaskLineageMetadata,
    TaskScheduleState, TaskScheduledEvent, UserMessageSubmittedEvent,
};
use harness_core::session::AssistantPart;
use harness_core::store::{
    EventEnvelopeWithoutSeqV1, EventStore, EventStoreError, EventStream, InMemoryEventStore,
    RuntimeEventStream,
};
use harness_providers::mock::{request_digest, MockProvider};
use harness_providers::{
    CompletionRequest, CompletionUsage, Provider, ProviderEventStream, ProviderStreamEvent,
};

use super::stream::{
    evaluate_prompt_completion, has_provider_error_finish, parse_wait_timeout_ms,
    wait_for_prompt_completion, wait_for_prompt_completion_with_output, PromptCompletionStatus,
    DEFAULT_WAIT_TIMEOUT,
};
use super::{
    apply_prompt_command_config, permission_policy_for_resolution,
    resolve_effective_permission_policy, resolve_permission_mode, resolve_prompt_model_override,
    resolve_settings, run_prompt, user_actor, PermissionModeResolution, PromptCommand,
    PromptOutputFormat, PromptSettings,
};
use harness_core::config::PermissionMode;
use harness_core::coord::CoordinatorConfig;
use harness_core::perm::{PermissionKind, PermissionPolicy, PolicyDecision};
use uuid::Uuid;

#[tokio::test]
async fn prompt_distinct_model_override_reaches_recorded_runtime_context() {
    // arrange
    use harness_core::agent::AgentProfile;
    use harness_core::clock::FakeClock;
    use harness_core::config::{load_config_from_str, resolve_model_selection};
    use harness_core::coord::spawn_coordinator;
    use harness_core::proj::RunMetadata;
    use harness_core::redact::DefaultRedactor;
    use std::collections::BTreeMap;

    let temp = tempfile::tempdir().unwrap_or_abort();
    let config = load_config_from_str(
        r#"{
          provider: { mock: { type: "openai_compatible", baseURL: "http://127.0.0.1:1/v1", apiKey: "test", models: {
            base: { name: "Base", limit: { context: 8192, input: 4096, output: 1024 } },
            "model-1": { name: "Selected", limit: { context: 64000, input: 48000, output: 8000 } }
          } } },
          model: "mock/base",
          agent: { default: { model: "mock/base" } },
          permission: "deny"
        }"#,
    )
    .unwrap_or_abort();
    let expected = resolve_model_selection(&config, "mock:model-1", None)
        .unwrap_or_abort()
        .primary;
    let mut coordinator_config = CoordinatorConfig::new(temp.path().join("sessions"));
    coordinator_config.deterministic_store = true;
    coordinator_config.provider = Arc::new(crate::scenarios::golden_path_provider());
    coordinator_config.agent_profiles = BTreeMap::from([(
        "default".to_string(),
        AgentProfile {
            name: "default".to_string(),
            model_ref: "mock:base".to_string(),
            model_ref_explicit: true,
            system_prompt: "test".to_string(),
            temperature: None,
            cache_retention: Default::default(),
            max_iters: Some(1),
            tool_failure_mode: harness_core::config::ToolFailureMode::ContinueAsToolMessage,
            toolset: Vec::new(),
            permission_ruleset: Vec::new(),
        },
    )]);
    let settings = PromptSettings {
        logging_config: Some(config),
        coordinator_config: coordinator_config.clone(),
        default_profile: "default".to_string(),
        deterministic: true,
        deterministic_seed: 1,
        config_digest: "test".to_string(),
        workspace_root: temp.path().to_path_buf(),
        deps: crate::CliDeps::real().with_current_dir(temp.path().to_path_buf()),
    };
    let mut cmd = default_prompt_command();
    cmd.model = Some("mock:model-1".to_string());
    let model_override = resolve_prompt_model_override(&cmd, &settings, "default")
        .unwrap_or_abort()
        .unwrap_or_abort();
    let coordinator = spawn_coordinator(
        coordinator_config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator
        .start_run("prompt-target", temp.path())
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(crate::scenarios::supervisor_actor(), "default", None)
        .await
        .unwrap_or_abort();

    // act
    coordinator
        .request_agent_turn_with_model_target(
            user_actor(),
            agent_id,
            "prompt override",
            model_override.model_target.unwrap_or_abort(),
        )
        .await
        .unwrap_or_abort();
    let metadata: RunMetadata = serde_json::from_str(
        &std::fs::read_to_string(run.run_dir.join("meta.json")).unwrap_or_abort(),
    )
    .unwrap_or_abort();
    let recorded = metadata.recorded_runtime_context.unwrap_or_abort();

    // assert
    assert_eq!(recorded.provider, expected.provider);
    assert_eq!(recorded.model, expected.model);
    assert_eq!(recorded.model_limits, expected.limits);
    assert_eq!(recorded.reasoning_effort, expected.reasoning_effort);
    assert_eq!(recorded.profile, "default");
    coordinator.stop_run().await.unwrap_or_abort();
}
fn default_prompt_command() -> PromptCommand {
    PromptCommand {
        text: Some("hello".to_string()),
        stdin: false,
        message: Vec::new(),
        model: None,
        variant: None,
        thinking: false,
        mock: false,
        resume: None,
        out: None,
        print_run_dir: false,
        max_turns: None,
        no_subagents: false,
        tools: Vec::new(),
        disallowed_tools: Vec::new(),
        disable_web_search: false,
        no_memory: false,
        prompt_file: None,
        verbatim: false,
        system_prompt_override: None,
        dangerously_skip_permissions: false,
        permission_mode: None,
        session_id: None,
        rules: None,
        reasoning_effort: None,
        allow: Vec::new(),
        deny: Vec::new(),
        fork_session: false,
        sandbox: None,
        format: PromptOutputFormat::Default,
    }
}

#[test]
fn no_config_prompt_without_provider_returns_connect_guidance() {
    // arrange
    // act
    // assert
    let temp = tempfile::tempdir().unwrap_or_abort();
    let deps = crate::CliDeps::real()
        .with_current_dir(temp.path().to_path_buf())
        .with_env(
            "HARNESS_DATA_HOME",
            temp.path().join("data").to_string_lossy(),
        );
    let context = deps.config_load_context().unwrap_or_abort();

    let err = match resolve_settings(
        &default_prompt_command(),
        None,
        None,
        temp.path().to_path_buf(),
        &context,
        &deps,
    ) {
        Ok(_) => panic!("no provider should block prompt setup"),
        Err(err) => err,
    };

    assert!(err.contains("Connect a provider to send prompts"));
}

#[test]
fn no_config_prompt_with_stored_codex_uses_runtime_catalog() {
    // arrange
    // act
    // assert
    let temp = tempfile::tempdir().unwrap_or_abort();
    let data_home = temp.path().join("data");
    let store = CredentialStore::new(data_home.join("harness"));
    store
        .save(&StoredCredential::api_key(
            AuthProviderId::codex(),
            "test-token",
            SystemCredentialClock.now_rfc3339(),
        ))
        .unwrap_or_abort();
    let deps = crate::CliDeps::real()
        .with_current_dir(temp.path().to_path_buf())
        .with_env("HARNESS_DATA_HOME", data_home.to_string_lossy());
    let context = deps.config_load_context().unwrap_or_abort();

    let settings = resolve_settings(
        &default_prompt_command(),
        None,
        None,
        temp.path().to_path_buf(),
        &context,
        &deps,
    )
    .unwrap_or_abort();
    let config = settings.logging_config.unwrap_or_abort();

    assert!(config.providers.contains_key("openai-codex"));
    assert!(settings.config_digest.contains("builtin-auth-runtime"));
    assert_eq!(settings.default_profile, "default");
}

#[tokio::test]
async fn run_prompt_with_mock_provider_completes_successfully() {
    // arrange
    // act
    // assert
    let temp = tempfile::tempdir().unwrap_or_abort();
    let deps = crate::CliDeps::real()
        .with_current_dir(temp.path().to_path_buf())
        .with_env(
            "HARNESS_DATA_HOME",
            temp.path().join("data").to_string_lossy(),
        );
    let context = deps.config_load_context().unwrap_or_abort();

    let mut cmd = default_prompt_command();
    cmd.mock = true;
    cmd.text = Some("Hello from PTY".to_string());
    cmd.permission_mode = Some("bypassPermissions".to_string());

    let settings = resolve_settings(
        &cmd,
        None,
        Some(temp.path().join("sessions")),
        temp.path().to_path_buf(),
        &context,
        &deps,
    )
    .unwrap_or_abort();

    // act
    let mut stdout = std::io::sink();
    let outcome = run_prompt(&cmd, &settings, "Hello from PTY", &mut stdout).await;

    // assert
    let outcome = outcome.unwrap_or_abort();
    assert!(outcome.events_path.exists(), "events file should exist");
    assert!(outcome.run_dir.exists(), "run dir should exist");

    let events_body = std::fs::read_to_string(&outcome.events_path).unwrap_or_abort();
    assert!(
        events_body.contains("Hello world"),
        "expected mock transcript to include scripted response: {events_body}"
    );
    assert!(
        events_body.contains("\"event_type\":\"task_completed\""),
        "expected prompt mock run to complete a task: {events_body}"
    );
}

#[test]
fn parse_wait_timeout_ms_uses_default_when_unset() {
    // arrange
    // act
    // assert
    assert_eq!(parse_wait_timeout_ms(None), DEFAULT_WAIT_TIMEOUT);
}

#[test]
fn parse_wait_timeout_ms_uses_default_when_invalid() {
    // arrange
    // act
    // assert
    assert_eq!(
        parse_wait_timeout_ms(Some("not-a-number")),
        DEFAULT_WAIT_TIMEOUT
    );
    assert_eq!(parse_wait_timeout_ms(Some("0")), DEFAULT_WAIT_TIMEOUT);
    assert_eq!(parse_wait_timeout_ms(Some("   0  ")), DEFAULT_WAIT_TIMEOUT);
}

#[test]
fn parse_wait_timeout_ms_parses_positive_milliseconds() {
    // arrange
    // act
    // assert
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
    // arrange
    // act
    // assert
    let events = vec![event_with_correlation(
        EventV1::TaskCancelled(TaskCancelledEvent {
            task_id: "task_000001".to_string().into(),
            reason: "provider denied request".to_string(),
            task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
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
    // arrange
    // act
    // assert
    let events = vec![event(EventV1::ProviderRequestFinished(
        ProviderRequestFinishedEvent {
            request_id: "req_000001".into(),
            finish_reason: "error".to_string(),
            output_digest: None,
            usage: None,
            metadata: None,
        },
    ))];

    let status = evaluate_prompt_completion(&events, "req_000001");
    assert_eq!(status, PromptCompletionStatus::Continue);
}

#[test]
fn evaluate_prompt_completion_waits_for_prompt_task_completion_after_provider_finish() {
    // arrange
    // act
    // assert
    let events = vec![
        provider_task_scheduled_event("task_000001", "req_000001"),
        event(EventV1::ProviderRequestFinished(
            ProviderRequestFinishedEvent {
                request_id: "req_000001".into(),
                finish_reason: "done".to_string(),
                output_digest: Some("abc123".to_string()),
                usage: None,
                metadata: None,
            },
        )),
    ];

    let status = evaluate_prompt_completion(&events, "req_000001");
    assert_eq!(status, PromptCompletionStatus::Continue);
}

#[tokio::test]
async fn prompt_tracker_waits_for_agent_turn_end_not_provider_finish() {
    // arrange
    // act
    // assert
    let store = Arc::new(CountingEventStore::new());
    let store_clone = Arc::clone(&store);
    let wait_store: Arc<dyn EventStore> = store_clone;
    let waiter = tokio::spawn(async move {
        wait_for_prompt_completion(wait_store, "req_000001", Duration::from_secs(1)).await
    });

    tokio::task::yield_now().await;
    store
        .append(draft_event(
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "provider_call_000001".into(),
                finish_reason: "done".to_string(),
                output_digest: Some("abc123".to_string()),
                usage: None,
                metadata: None,
            }),
            Some("req_000001"),
        ))
        .unwrap_or_abort();

    tokio::task::yield_now().await;
    assert!(
        !waiter.is_finished(),
        "provider finish alone must not complete prompt wait"
    );

    store
        .append(draft_event(
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: "task_000001".to_string().into(),
                result_summary: "ok".to_string(),
                result_digest: "def456".to_string(),
                metadata: Some(TaskCompletionMetadata {
                    lineage: None,
                    task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
                    timing: None,
                    hook_executions: Vec::new(),
                }),
            }),
            Some("req_000001"),
        ))
        .unwrap_or_abort();

    assert_eq!(waiter.await.unwrap_or_abort(), Ok(()));
}

#[tokio::test]
async fn prompt_stream_preserves_typed_live_variants_until_durable_completion() {
    // Given: a prompt waiter has subscribed to the typed runtime stream.
    let store = Arc::new(CountingEventStore::new());
    let wait_store: Arc<dyn EventStore> = Arc::<CountingEventStore>::clone(&store);
    let waiter = tokio::spawn(async move {
        let mut output = Vec::new();
        let result = wait_for_prompt_completion_with_output(
            wait_store,
            "turn-1",
            Duration::from_secs(1),
            true,
            PromptOutputFormat::StreamingJson,
            &mut output,
        )
        .await;
        (result, output)
    });
    tokio::time::timeout(
        Duration::from_millis(500),
        store.wait_until_runtime_subscribed(),
    )
    .await
    .unwrap_or_abort();

    // When: each live fragment kind arrives before the final durable assistant commit.
    for (event_id, payload) in [
        (
            "live-reasoning",
            LiveEventV1::ProviderReasoningDelta {
                request_id: "provider-1".into(),
                delta: "reasoning".to_string(),
            },
        ),
        (
            "live-text",
            LiveEventV1::ProviderTextDelta {
                request_id: "provider-1".into(),
                delta: "answer".to_string(),
            },
        ),
        (
            "live-tool-input",
            LiveEventV1::ProviderToolInputDelta {
                request_id: "provider-1".into(),
                tool_call_id: "tool-1".into(),
                delta: "{\"path\":\"src/lib.rs\"}".to_string(),
            },
        ),
    ] {
        store.publish_live(LiveEventEnvelope {
            event_id: event_id.to_string(),
            run_id: "run-prompt-runtime".into(),
            mono_ms: 1,
            ts: None,
            actor: EventActor::new(ActorKind::Worker, Some("agent-1".to_string())),
            correlation_id: Some("turn-1".to_string()),
            causation_id: None,
            stream_key: Some("agent:agent-1".to_string()),
            payload,
        });
    }
    store
        .append(draft_event(
            EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
                request_id: "provider-1".into(),
                tool_call_count: 0,
                parts: vec![AssistantPart::Text {
                    text: "answer".to_string(),
                }],
                provenance: None,
                assistant_message: None,
            }),
            Some("turn-1"),
        ))
        .unwrap_or_abort();
    store
        .append(draft_event(
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: "task-1".into(),
                result_summary: "answer".to_string(),
                result_digest: "result-digest".to_string(),
                metadata: Some(TaskCompletionMetadata {
                    lineage: None,
                    task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
                    timing: None,
                    hook_executions: Vec::new(),
                }),
            }),
            Some("turn-1"),
        ))
        .unwrap_or_abort();

    // Then: live machine variants are explicit and only durable events settle the waiter.
    let (result, output) = waiter.await.unwrap_or_abort();
    assert_eq!(result, Ok(()));
    let events = String::from_utf8(output)
        .unwrap_or_abort()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap_or_abort())
        .collect::<Vec<_>>();
    assert_eq!(
        events
            .iter()
            .filter(|event| event["delivery"] == "live")
            .count(),
        3
    );
    assert!(events.iter().any(|event| {
        event["delivery"] == "live"
            && event["event"]["payload"]["event_type"] == "provider_tool_input_delta"
    }));
    assert!(events.iter().any(|event| {
        event["delivery"] == "durable"
            && event["event"]["payload"]["event_type"] == "assistant_message_finished"
    }));
}

#[test]
fn evaluate_prompt_completion_waits_for_tool_task_completion() {
    // arrange
    // act
    // assert
    let events = vec![
        provider_task_scheduled_event("task_000001", "req_000001"),
        event_with_correlation(
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: "task_000002".to_string().into(),
                result_summary: "tool ok".to_string(),
                result_digest: "def456".to_string(),
                metadata: Some(TaskCompletionMetadata {
                    lineage: Some(TaskLineageMetadata {
                        parent_tool_call_id: Some("tool_call_000001".to_string()),
                        ..TaskLineageMetadata::default()
                    }),
                    task_scope: Some(harness_core::event::TaskTerminalScope::ToolCall),
                    timing: None,
                    hook_executions: Vec::new(),
                }),
            }),
            Some("req_000001"),
        ),
    ];

    let status = evaluate_prompt_completion(&events, "req_000001");
    assert_eq!(status, PromptCompletionStatus::Continue);
}

#[test]
fn evaluate_prompt_completion_ignores_tool_task_without_agent_turn_schedule() {
    // arrange
    // act
    // assert
    let events = vec![event_with_correlation(
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_000002".to_string().into(),
            result_summary: "tool ok".to_string(),
            result_digest: "def456".to_string(),
            metadata: Some(TaskCompletionMetadata {
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: Some("tool_call_000001".to_string()),
                    ..TaskLineageMetadata::default()
                }),
                task_scope: Some(harness_core::event::TaskTerminalScope::ToolCall),
                timing: None,
                hook_executions: Vec::new(),
            }),
        }),
        Some("req_000001"),
    )];

    let status = evaluate_prompt_completion(&events, "req_000001");
    assert_eq!(status, PromptCompletionStatus::Continue);
}

#[test]
fn evaluate_prompt_completion_ignores_cancelled_child_tool_task() {
    // arrange
    // act
    // assert
    let events = vec![
        provider_task_scheduled_event("task_000001", "req_000001"),
        event_with_correlation(
            EventV1::TaskCancelled(TaskCancelledEvent {
                task_id: "task_000002".to_string().into(),
                reason: "tool execution failed: expected audit error".to_string(),
                task_scope: Some(harness_core::event::TaskTerminalScope::ToolCall),
            }),
            Some("req_000001"),
        ),
        event_with_correlation(
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: "task_000001".to_string().into(),
                result_summary: "ok".to_string(),
                result_digest: "abc123".to_string(),
                metadata: Some(TaskCompletionMetadata {
                    lineage: None,
                    task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
                    timing: None,
                    hook_executions: Vec::new(),
                }),
            }),
            Some("req_000001"),
        ),
    ];

    let status = evaluate_prompt_completion(&events, "req_000001");
    assert_eq!(status, PromptCompletionStatus::Completed);
}

#[test]
fn evaluate_prompt_completion_reports_success_for_prompt_task_completed() {
    // arrange
    // act
    // assert
    let events = vec![
        provider_task_scheduled_event("task_000001", "req_000001"),
        event_with_correlation(
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: "task_000001".to_string().into(),
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
fn evaluate_prompt_completion_reports_success_for_terminal_only_agent_turn_completion() {
    // arrange
    // act
    // assert
    let events = vec![event_with_correlation(
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_000001".to_string().into(),
            result_summary: "ok".to_string(),
            result_digest: "abc123".to_string(),
            metadata: Some(TaskCompletionMetadata {
                lineage: None,
                task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
                timing: None,
                hook_executions: Vec::new(),
            }),
        }),
        Some("req_000001"),
    )];

    let status = evaluate_prompt_completion(&events, "req_000001");
    assert_eq!(status, PromptCompletionStatus::Completed);
}

#[test]
fn evaluate_prompt_completion_prioritizes_run_failed() {
    // arrange
    // act
    // assert
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
    // arrange
    // act
    // assert
    let events = vec![event_with_correlation(
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: "provider_call_000007".into(),
            finish_reason: "error".to_string(),
            output_digest: None,
            usage: None,
            metadata: None,
        }),
        Some("req_000007"),
    )];

    assert!(has_provider_error_finish(&events, "req_000007"));
    assert!(!has_provider_error_finish(&events, "req_000008"));
}

#[test]
fn evaluate_prompt_completion_supports_correlated_task_id_equals_request_id_fallback() {
    // arrange
    // act
    // assert
    let events = vec![event_with_correlation(
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "req_000123".to_string().into(),
            result_summary: "ok".to_string(),
            result_digest: "abc123".to_string(),
            metadata: None,
        }),
        Some("req_000123"),
    )];

    let status = evaluate_prompt_completion(&events, "req_000123");
    assert_eq!(status, PromptCompletionStatus::Completed);
}

#[test]
fn evaluate_prompt_completion_ignores_uncorrelated_agent_turn_completion() {
    // arrange
    // act
    // assert
    let events = vec![event_with_correlation(
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_000999".to_string().into(),
            result_summary: "other turn".to_string(),
            result_digest: "abc123".to_string(),
            metadata: Some(TaskCompletionMetadata {
                lineage: None,
                task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
                timing: None,
                hook_executions: Vec::new(),
            }),
        }),
        Some("req_other"),
    )];

    let status = evaluate_prompt_completion(&events, "req_000123");
    assert_eq!(status, PromptCompletionStatus::Continue);
}

#[tokio::test]
async fn wait_for_prompt_completion_subscribes_once_and_streams_new_events() {
    // arrange
    // act
    // assert
    let store = Arc::new(CountingEventStore::new());
    for index in 0..256 {
        store
            .append(draft_event(
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: format!("tool_task_{index:04}").into(),
                    result_summary: "ok".to_string(),
                    result_digest: format!("digest_{index:04}"),
                    metadata: None,
                }),
                Some("other_request"),
            ))
            .unwrap_or_abort();
    }

    let store_clone = Arc::clone(&store);
    let wait_store: Arc<dyn EventStore> = store_clone;
    let waiter = tokio::spawn(async move {
        wait_for_prompt_completion(wait_store, "req_000001", Duration::from_secs(1)).await
    });

    tokio::task::yield_now().await;

    store
        .append(draft_event(
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_000001".to_string().into(),
                state: TaskScheduleState::Started,
                queue_key: Some("provider_model:default:gpt-4o-mini".to_string()),
                metadata: None,
            }),
            Some("req_000001"),
        ))
        .unwrap_or_abort();
    store
        .append(draft_event(
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: "task_000001".to_string().into(),
                result_summary: "ok".to_string(),
                result_digest: "abc123".to_string(),
                metadata: None,
            }),
            Some("req_000001"),
        ))
        .unwrap_or_abort();

    assert_eq!(waiter.await.unwrap_or_abort(), Ok(()));
    assert_eq!(store.subscribe_calls(), 1);
    assert_eq!(store.replay_calls(), 0);
}

#[test]
fn resolve_permission_mode_bypass_activates_allow_all() {
    // arrange
    // act
    // assert
    assert_eq!(
        resolve_permission_mode(Some("bypassPermissions"), false).unwrap_or_abort(),
        PermissionModeResolution::AllowAll
    );
    assert_eq!(
        resolve_permission_mode(Some("yolo"), false).unwrap_or_abort(),
        PermissionModeResolution::AllowAll
    );
    assert_eq!(
        resolve_permission_mode(None, true).unwrap_or_abort(),
        PermissionModeResolution::AllowAll
    );
}

#[test]
fn resolve_permission_mode_default_resets_to_default() {
    // arrange
    // act
    // assert
    assert_eq!(
        resolve_permission_mode(Some("default"), false).unwrap_or_abort(),
        PermissionModeResolution::ResetToDefault
    );
}

#[test]
fn resolve_permission_mode_without_selection_does_not_activate() {
    // arrange
    // act
    // assert
    assert_eq!(
        resolve_permission_mode(None, false).unwrap_or_abort(),
        PermissionModeResolution::NoChange
    );
}

#[test]
fn resolve_permission_mode_rejects_removed_plan_mode() {
    // arrange
    // act
    let error = resolve_permission_mode(Some("plan"), false).unwrap_err();

    // assert
    assert!(error.contains("unknown permission mode `plan`"));
}

#[test]
fn resolve_permission_mode_accept_edits_allows_edits_only() {
    // arrange
    // act
    // assert
    assert_eq!(
        resolve_permission_mode(Some("acceptEdits"), false).unwrap_or_abort(),
        PermissionModeResolution::AcceptEdits
    );
}

#[test]
fn resolve_permission_mode_dont_ask_denies_mutations() {
    // arrange
    // act
    // assert
    assert_eq!(
        resolve_permission_mode(Some("dontAsk"), false).unwrap_or_abort(),
        PermissionModeResolution::DenyByDefault
    );
}

#[test]
fn accept_edits_policy_evaluates_all_kinds_correctly() {
    // arrange
    // act
    // assert
    let policy = permission_policy_for_resolution(PermissionModeResolution::AcceptEdits).unwrap();
    assert_eq!(
        policy.evaluate(None, PermissionKind::EditFs),
        PolicyDecision::Allow
    );
    assert!(matches!(
        policy.evaluate(None, PermissionKind::Shell),
        PolicyDecision::Ask { .. }
    ));
    assert!(matches!(
        policy.evaluate(None, PermissionKind::Network),
        PolicyDecision::Ask { .. }
    ));
    assert_eq!(
        policy.evaluate(None, PermissionKind::Question),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Task),
        PolicyDecision::Allow
    );
    assert!(matches!(
        policy.evaluate(None, PermissionKind::WebFetch),
        PolicyDecision::Ask { .. }
    ));
    assert!(matches!(
        policy.evaluate(None, PermissionKind::WebSearch),
        PolicyDecision::Ask { .. }
    ));
    assert!(matches!(
        policy.evaluate(None, PermissionKind::CodeSearch),
        PolicyDecision::Ask { .. }
    ));
    assert_eq!(
        policy.evaluate(None, PermissionKind::Lsp),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Read),
        PolicyDecision::Allow
    );
    assert!(matches!(
        policy.evaluate(None, PermissionKind::ExternalDirectory),
        PolicyDecision::Ask { .. }
    ));
    assert!(matches!(
        policy.evaluate(None, PermissionKind::DoomLoop),
        PolicyDecision::Ask { .. }
    ));
}

#[test]
fn dont_ask_policy_evaluates_all_kinds_correctly() {
    // arrange
    // act
    // assert
    let policy = permission_policy_for_resolution(PermissionModeResolution::DenyByDefault).unwrap();
    assert_eq!(
        policy.evaluate(None, PermissionKind::EditFs),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Shell),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Network),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Question),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Task),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::WebFetch),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::WebSearch),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::CodeSearch),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Lsp),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Read),
        PolicyDecision::Allow
    );
    assert!(matches!(
        policy.evaluate(None, PermissionKind::ExternalDirectory),
        PolicyDecision::Ask { .. }
    ));
    assert!(matches!(
        policy.evaluate(None, PermissionKind::DoomLoop),
        PolicyDecision::Ask { .. }
    ));
}

#[test]
fn allow_all_policy_evaluates_all_kinds_correctly() {
    // arrange
    // act
    // assert
    let policy = permission_policy_for_resolution(PermissionModeResolution::AllowAll).unwrap();
    assert_eq!(
        policy.evaluate(None, PermissionKind::EditFs),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Shell),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Network),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Question),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Task),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::WebFetch),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::WebSearch),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::CodeSearch),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Lsp),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Read),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::ExternalDirectory),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::DoomLoop),
        PolicyDecision::Allow
    );
}

#[test]
fn reset_to_default_policy_evaluates_all_kinds_correctly() {
    // arrange
    // act
    // assert
    let policy =
        permission_policy_for_resolution(PermissionModeResolution::ResetToDefault).unwrap();
    assert!(matches!(
        policy.evaluate(None, PermissionKind::EditFs),
        PolicyDecision::Ask { .. }
    ));
    assert!(matches!(
        policy.evaluate(None, PermissionKind::Shell),
        PolicyDecision::Ask { .. }
    ));
    assert!(matches!(
        policy.evaluate(None, PermissionKind::Network),
        PolicyDecision::Ask { .. }
    ));
    assert_eq!(
        policy.evaluate(None, PermissionKind::Question),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Task),
        PolicyDecision::Allow
    );
    assert!(matches!(
        policy.evaluate(None, PermissionKind::WebFetch),
        PolicyDecision::Ask { .. }
    ));
    assert!(matches!(
        policy.evaluate(None, PermissionKind::WebSearch),
        PolicyDecision::Ask { .. }
    ));
    assert!(matches!(
        policy.evaluate(None, PermissionKind::CodeSearch),
        PolicyDecision::Ask { .. }
    ));
    assert_eq!(
        policy.evaluate(None, PermissionKind::Lsp),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Read),
        PolicyDecision::Allow
    );
    assert!(matches!(
        policy.evaluate(None, PermissionKind::ExternalDirectory),
        PolicyDecision::Ask { .. }
    ));
    assert!(matches!(
        policy.evaluate(None, PermissionKind::DoomLoop),
        PolicyDecision::Ask { .. }
    ));
}

#[test]
fn no_change_resolution_yields_no_policy() {
    // arrange
    // act
    // assert
    assert!(permission_policy_for_resolution(PermissionModeResolution::NoChange).is_none());
}

#[test]
fn resolve_permission_mode_rejects_unknown_mode() {
    // arrange
    // act
    // assert
    assert!(resolve_permission_mode(Some("invalid"), false).is_err());
    assert!(resolve_permission_mode(Some(""), false).is_err());
    assert!(resolve_permission_mode(Some("auto"), false).is_err());
}

#[test]
fn sandbox_profile_rejects_invalid_values() {
    // arrange
    // act
    // assert
    assert!(PermissionPolicy::from_sandbox_profile("readonly").is_some());
    assert!(PermissionPolicy::from_sandbox_profile("workspace").is_some());
    assert!(PermissionPolicy::from_sandbox_profile("danger").is_some());
    assert!(PermissionPolicy::from_sandbox_profile("invalid").is_none());
    assert!(PermissionPolicy::from_sandbox_profile("").is_none());
}

#[test]
fn command_path_sandbox_alone_evaluates_all_kinds() {
    // arrange
    // act
    // assert
    let mut cmd = default_prompt_command();
    cmd.sandbox = Some("readonly".to_string());
    let policy = resolve_effective_permission_policy(&cmd, PermissionPolicy::default()).unwrap();
    assert_eq!(
        policy.evaluate(None, PermissionKind::EditFs),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Shell),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Network),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Question),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Task),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::WebFetch),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::WebSearch),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::CodeSearch),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Lsp),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Read),
        PolicyDecision::Allow
    );
    assert!(matches!(
        policy.evaluate(None, PermissionKind::ExternalDirectory),
        PolicyDecision::Ask { .. }
    ));
    assert!(matches!(
        policy.evaluate(None, PermissionKind::DoomLoop),
        PolicyDecision::Ask { .. }
    ));
}

#[test]
fn command_path_permission_mode_overrides_sandbox_evaluates_all_kinds() {
    // arrange
    // act
    // assert
    let mut cmd = default_prompt_command();
    cmd.sandbox = Some("readonly".to_string());
    cmd.permission_mode = Some("bypassPermissions".to_string());
    let policy = resolve_effective_permission_policy(&cmd, PermissionPolicy::default()).unwrap();
    assert_eq!(
        policy.evaluate(None, PermissionKind::EditFs),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Shell),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Network),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Question),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Task),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::WebFetch),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::WebSearch),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::CodeSearch),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Lsp),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Read),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::ExternalDirectory),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::DoomLoop),
        PolicyDecision::Allow
    );
}

#[test]
fn command_path_allow_override_preserves_other_kinds() {
    // arrange
    // act
    // assert
    let mut cmd = default_prompt_command();
    cmd.permission_mode = Some("acceptEdits".to_string());
    cmd.allow = vec!["bash".to_string()];
    let policy = resolve_effective_permission_policy(&cmd, PermissionPolicy::default()).unwrap();
    assert_eq!(
        policy.evaluate(None, PermissionKind::EditFs),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Shell),
        PolicyDecision::Allow
    );
    assert!(matches!(
        policy.evaluate(None, PermissionKind::Network),
        PolicyDecision::Ask { .. }
    ));
    assert_eq!(
        policy.evaluate(None, PermissionKind::Question),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Task),
        PolicyDecision::Allow
    );
    assert!(matches!(
        policy.evaluate(None, PermissionKind::WebFetch),
        PolicyDecision::Ask { .. }
    ));
    assert!(matches!(
        policy.evaluate(None, PermissionKind::WebSearch),
        PolicyDecision::Ask { .. }
    ));
    assert!(matches!(
        policy.evaluate(None, PermissionKind::CodeSearch),
        PolicyDecision::Ask { .. }
    ));
    assert_eq!(
        policy.evaluate(None, PermissionKind::Lsp),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Read),
        PolicyDecision::Allow
    );
    assert!(matches!(
        policy.evaluate(None, PermissionKind::ExternalDirectory),
        PolicyDecision::Ask { .. }
    ));
    assert!(matches!(
        policy.evaluate(None, PermissionKind::DoomLoop),
        PolicyDecision::Ask { .. }
    ));
}

#[test]
fn command_path_deny_override_preserves_other_kinds() {
    // arrange
    // act
    // assert
    let mut cmd = default_prompt_command();
    cmd.dangerously_skip_permissions = true;
    cmd.deny = vec!["edit".to_string()];
    let policy = resolve_effective_permission_policy(&cmd, PermissionPolicy::default()).unwrap();
    assert_eq!(
        policy.evaluate(None, PermissionKind::EditFs),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Shell),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Network),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Question),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Task),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::WebFetch),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::WebSearch),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::CodeSearch),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Lsp),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Read),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::ExternalDirectory),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::DoomLoop),
        PolicyDecision::Allow
    );
}

#[test]
fn command_path_conflicting_allow_and_deny_deny_wins() {
    // arrange
    // act
    // assert
    let mut cmd = default_prompt_command();
    cmd.dangerously_skip_permissions = true;
    cmd.allow = vec!["bash".to_string()];
    cmd.deny = vec!["bash".to_string()];
    let policy = resolve_effective_permission_policy(&cmd, PermissionPolicy::default()).unwrap();
    assert_eq!(
        policy.evaluate(None, PermissionKind::Shell),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::EditFs),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Network),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Question),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Task),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::WebFetch),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::WebSearch),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::CodeSearch),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Lsp),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Read),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::ExternalDirectory),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::DoomLoop),
        PolicyDecision::Allow
    );
}

#[test]
fn command_path_full_precedence_chain_evaluates_all_kinds() {
    // arrange
    // act
    // assert
    let mut cmd = default_prompt_command();
    cmd.sandbox = Some("readonly".to_string());
    cmd.permission_mode = Some("acceptEdits".to_string());
    cmd.allow = vec!["bash".to_string()];
    let policy = resolve_effective_permission_policy(&cmd, PermissionPolicy::default()).unwrap();
    assert_eq!(
        policy.evaluate(None, PermissionKind::EditFs),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Shell),
        PolicyDecision::Allow
    );
    assert!(matches!(
        policy.evaluate(None, PermissionKind::Network),
        PolicyDecision::Ask { .. }
    ));
    assert_eq!(
        policy.evaluate(None, PermissionKind::Question),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Task),
        PolicyDecision::Allow
    );
    assert!(matches!(
        policy.evaluate(None, PermissionKind::WebFetch),
        PolicyDecision::Ask { .. }
    ));
    assert!(matches!(
        policy.evaluate(None, PermissionKind::WebSearch),
        PolicyDecision::Ask { .. }
    ));
    assert!(matches!(
        policy.evaluate(None, PermissionKind::CodeSearch),
        PolicyDecision::Ask { .. }
    ));
    assert_eq!(
        policy.evaluate(None, PermissionKind::Lsp),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Read),
        PolicyDecision::Allow
    );
    assert!(matches!(
        policy.evaluate(None, PermissionKind::ExternalDirectory),
        PolicyDecision::Ask { .. }
    ));
    assert!(matches!(
        policy.evaluate(None, PermissionKind::DoomLoop),
        PolicyDecision::Ask { .. }
    ));
}

#[test]
fn command_path_no_flags_preserves_base_policy_all_kinds() {
    // arrange
    // act
    // assert
    let cmd = default_prompt_command();
    let base = PermissionPolicy::default();
    let policy = resolve_effective_permission_policy(&cmd, base).unwrap();
    assert_eq!(
        policy.evaluate(None, PermissionKind::EditFs),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Shell),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Network),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Question),
        PolicyDecision::Deny
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Task),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::WebFetch),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::WebSearch),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::CodeSearch),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Lsp),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy.evaluate(None, PermissionKind::Read),
        PolicyDecision::Allow
    );
    assert!(matches!(
        policy.evaluate(None, PermissionKind::ExternalDirectory),
        PolicyDecision::Ask { .. }
    ));
    assert!(matches!(
        policy.evaluate(None, PermissionKind::DoomLoop),
        PolicyDecision::Ask { .. }
    ));
}

#[test]
fn command_level_apply_prompt_command_config_sets_permission_policy() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap();
    let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
    config.permission_policy = PermissionPolicy::default();

    let mut cmd = default_prompt_command();
    cmd.permission_mode = Some("acceptEdits".to_string());
    cmd.allow = vec!["bash".to_string()];

    apply_prompt_command_config(&cmd, &mut config, false, "test").unwrap();

    let p = &config.permission_policy;
    assert_eq!(
        p.evaluate(None, PermissionKind::EditFs),
        PolicyDecision::Allow
    );
    assert_eq!(
        p.evaluate(None, PermissionKind::Shell),
        PolicyDecision::Allow
    );
    assert!(matches!(
        p.evaluate(None, PermissionKind::Network),
        PolicyDecision::Ask { .. }
    ));
    assert_eq!(
        p.evaluate(None, PermissionKind::Question),
        PolicyDecision::Deny
    );
    assert_eq!(
        p.evaluate(None, PermissionKind::Task),
        PolicyDecision::Allow
    );
    assert!(matches!(
        p.evaluate(None, PermissionKind::WebFetch),
        PolicyDecision::Ask { .. }
    ));
    assert!(matches!(
        p.evaluate(None, PermissionKind::WebSearch),
        PolicyDecision::Ask { .. }
    ));
    assert!(matches!(
        p.evaluate(None, PermissionKind::CodeSearch),
        PolicyDecision::Ask { .. }
    ));
    assert_eq!(p.evaluate(None, PermissionKind::Lsp), PolicyDecision::Allow);
    assert_eq!(
        p.evaluate(None, PermissionKind::Read),
        PolicyDecision::Allow
    );
    assert!(matches!(
        p.evaluate(None, PermissionKind::ExternalDirectory),
        PolicyDecision::Ask { .. }
    ));
    assert!(matches!(
        p.evaluate(None, PermissionKind::DoomLoop),
        PolicyDecision::Ask { .. }
    ));
}

#[test]
fn command_level_apply_prompt_command_config_sandbox_overrides_base_then_mode_overrides_sandbox() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap();
    let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
    config.permission_policy = PermissionPolicy::default();

    let mut cmd = default_prompt_command();
    cmd.sandbox = Some("readonly".to_string());
    cmd.permission_mode = Some("bypassPermissions".to_string());

    apply_prompt_command_config(&cmd, &mut config, false, "test").unwrap();

    let p = &config.permission_policy;
    assert_eq!(
        p.evaluate(None, PermissionKind::EditFs),
        PolicyDecision::Allow
    );
    assert_eq!(
        p.evaluate(None, PermissionKind::Shell),
        PolicyDecision::Allow
    );
    assert_eq!(
        p.evaluate(None, PermissionKind::Network),
        PolicyDecision::Allow
    );
    assert_eq!(
        p.evaluate(None, PermissionKind::Question),
        PolicyDecision::Allow
    );
    assert_eq!(
        p.evaluate(None, PermissionKind::Task),
        PolicyDecision::Allow
    );
    assert_eq!(
        p.evaluate(None, PermissionKind::WebFetch),
        PolicyDecision::Allow
    );
    assert_eq!(
        p.evaluate(None, PermissionKind::WebSearch),
        PolicyDecision::Allow
    );
    assert_eq!(
        p.evaluate(None, PermissionKind::CodeSearch),
        PolicyDecision::Allow
    );
    assert_eq!(p.evaluate(None, PermissionKind::Lsp), PolicyDecision::Allow);
    assert_eq!(
        p.evaluate(None, PermissionKind::Read),
        PolicyDecision::Allow
    );
    assert_eq!(
        p.evaluate(None, PermissionKind::ExternalDirectory),
        PolicyDecision::Allow
    );
    assert_eq!(
        p.evaluate(None, PermissionKind::DoomLoop),
        PolicyDecision::Allow
    );
}

#[test]
fn resolve_effective_permission_policy_preserves_ask_timeout_ms() {
    // arrange
    // act
    // assert
    let base = PermissionPolicy::default().with_ask_timeout_ms(99_999);
    let mut cmd = default_prompt_command();
    cmd.permission_mode = Some("bypassPermissions".to_string());

    let resolved = resolve_effective_permission_policy(&cmd, base).unwrap();

    assert_eq!(resolved.ask_timeout_ms(), 99_999);
    assert_eq!(
        resolved.evaluate(None, PermissionKind::EditFs),
        PolicyDecision::Allow
    );
    assert_eq!(
        resolved.evaluate(None, PermissionKind::Shell),
        PolicyDecision::Allow
    );
    assert_eq!(
        resolved.evaluate(None, PermissionKind::Network),
        PolicyDecision::Allow
    );
    assert_eq!(
        resolved.evaluate(None, PermissionKind::Question),
        PolicyDecision::Allow
    );
    assert_eq!(
        resolved.evaluate(None, PermissionKind::Task),
        PolicyDecision::Allow
    );
    assert_eq!(
        resolved.evaluate(None, PermissionKind::WebFetch),
        PolicyDecision::Allow
    );
    assert_eq!(
        resolved.evaluate(None, PermissionKind::WebSearch),
        PolicyDecision::Allow
    );
    assert_eq!(
        resolved.evaluate(None, PermissionKind::CodeSearch),
        PolicyDecision::Allow
    );
    assert_eq!(
        resolved.evaluate(None, PermissionKind::Lsp),
        PolicyDecision::Allow
    );
    assert_eq!(
        resolved.evaluate(None, PermissionKind::Read),
        PolicyDecision::Allow
    );
    assert_eq!(
        resolved.evaluate(None, PermissionKind::ExternalDirectory),
        PolicyDecision::Allow
    );
    assert_eq!(
        resolved.evaluate(None, PermissionKind::DoomLoop),
        PolicyDecision::Allow
    );
}

#[test]
fn apply_tool_overrides_sets_all_twelve_kinds_allow_and_deny() {
    // arrange
    // act
    // assert
    let all_kinds = [
        "edit",
        "bash",
        "network",
        "question",
        "task",
        "webfetch",
        "websearch",
        "codesearch",
        "lsp",
        "read",
        "external_directory",
        "doom_loop",
    ];
    let all_permission_kinds = [
        PermissionKind::EditFs,
        PermissionKind::Shell,
        PermissionKind::Network,
        PermissionKind::Question,
        PermissionKind::Task,
        PermissionKind::WebFetch,
        PermissionKind::WebSearch,
        PermissionKind::CodeSearch,
        PermissionKind::Lsp,
        PermissionKind::Read,
        PermissionKind::ExternalDirectory,
        PermissionKind::DoomLoop,
    ];

    let mut policy = PermissionPolicy::allow_all();
    let deny_list: Vec<String> = all_kinds.iter().map(|s| s.to_string()).collect();
    policy.apply_tool_overrides(&[], &deny_list).unwrap();

    for kind in &all_permission_kinds {
        assert_eq!(
            policy.evaluate(None, *kind),
            PolicyDecision::Deny,
            "expected Deny for {kind:?}"
        );
    }

    let allow_list: Vec<String> = all_kinds.iter().map(|s| s.to_string()).collect();
    policy.apply_tool_overrides(&allow_list, &[]).unwrap();

    for kind in &all_permission_kinds {
        assert_eq!(
            policy.evaluate(None, *kind),
            PolicyDecision::Allow,
            "expected Allow for {kind:?}"
        );
    }
}

#[test]
fn apply_tool_overrides_rejects_unknown_kind() {
    // arrange
    // act
    // assert
    let mut policy = PermissionPolicy::allow_all();

    let err = policy
        .apply_tool_overrides(&["nonexistent".to_string()], &[])
        .unwrap_err();
    assert!(
        err.contains("unknown permission kind `nonexistent`"),
        "{err}"
    );

    let err = policy
        .apply_tool_overrides(&[], &["bogus".to_string()])
        .unwrap_err();
    assert!(err.contains("unknown permission kind `bogus`"), "{err}");
}

#[test]
fn apply_tool_overrides_deny_wins_over_allow_for_same_kind() {
    // arrange
    // act
    // assert
    let mut policy = PermissionPolicy::allow_all();
    policy
        .apply_tool_overrides(&["edit".to_string()], &["edit".to_string()])
        .unwrap();

    assert_eq!(
        policy.evaluate(None, PermissionKind::EditFs),
        PolicyDecision::Deny
    );
}

#[test]
fn session_id_uuid_validation_rejects_non_uuid() {
    // arrange
    // act
    // assert
    assert!(Uuid::parse_str("not-a-uuid").is_err());
    assert!(Uuid::parse_str("").is_err());
    assert!(Uuid::parse_str("../etc/passwd").is_err());
    assert!(Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").is_ok());
    assert!(Uuid::parse_str("550e8400e29b41d4a716446655440000").is_ok());
}

fn event(payload: EventV1) -> EventEnvelopeV1 {
    event_with_correlation(payload, None)
}

fn draft_event(payload: EventV1, correlation_id: Option<&str>) -> EventEnvelopeWithoutSeqV1 {
    EventEnvelopeWithoutSeqV1 {
        schema_version: 1,
        event_id: "evt_1".to_string(),
        run_id: "run_1".into(),
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
            task_id: task_id.to_string().into(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:default:gpt-4o-mini".to_string()),
            metadata: None,
        }),
        Some(request_id),
    )
}

fn event_with_correlation(payload: EventV1, correlation_id: Option<&str>) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: 1,
        event_id: "evt_1".to_string(),
        seq: 1,
        run_id: "run_1".into(),
        mono_ms: 0,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
        correlation_id: correlation_id.map(ToOwned::to_owned),
        causation_id: None,
        stream_key: None,
        payload,
    }
}

struct SequenceProvider {
    responses: Vec<Vec<ProviderStreamEvent>>,
    index: Mutex<usize>,
}

impl SequenceProvider {
    fn new(responses: Vec<Vec<ProviderStreamEvent>>) -> Arc<Self> {
        Arc::new(Self {
            responses,
            index: Mutex::new(0),
        })
    }
}

#[async_trait]
impl Provider for SequenceProvider {
    fn request_budget_semantics(
        &self,
        request: &CompletionRequest,
        pending_prompt_index: usize,
    ) -> Result<
        harness_providers::ProviderBudgetSemantics,
        harness_providers::ProviderRequestCostError,
    > {
        harness_providers::generic_request_budget_semantics(request, pending_prompt_index)
    }

    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream {
        let response = {
            let mut guard = self.index.lock().unwrap_or_abort();
            let response = self.responses.get(*guard).cloned().unwrap_or_else(|| {
                vec![
                    ProviderStreamEvent::Start,
                    ProviderStreamEvent::TextDelta("Done".to_string()),
                    ProviderStreamEvent::Done { usage: None },
                ]
            });
            *guard += 1;
            response
        };

        let digest = request_digest(&req);
        let provider = MockProvider::new(std::collections::BTreeMap::from([(digest, response)]));
        provider.stream_completion(req).await
    }
}

fn tool_call_events(
    tool_call_id: &str,
    function_name: &str,
    arguments: serde_json::Value,
) -> Vec<ProviderStreamEvent> {
    vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::ToolCallComplete {
            tool_call_id: tool_call_id.to_string(),
            function_name: function_name.to_string(),
            arguments_json: arguments.to_string(),
        },
        ProviderStreamEvent::Done {
            usage: Some(CompletionUsage {
                prompt_tokens: 12,
                completion_tokens: 3,
                total_tokens: 15,
            }),
        },
    ]
}

fn text_events(text: &str) -> Vec<ProviderStreamEvent> {
    vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::TextDelta(text.to_string()),
        ProviderStreamEvent::Done {
            usage: Some(CompletionUsage {
                prompt_tokens: 5,
                completion_tokens: 1,
                total_tokens: 6,
            }),
        },
    ]
}

#[tokio::test]
async fn run_prompt_with_tool_call_bypass_permissions_allows_edit() {
    // arrange
    // act
    // assert
    let temp = tempfile::tempdir().unwrap_or_abort();
    std::fs::write(temp.path().join("test.txt"), "hello").unwrap_or_abort();

    let provider = SequenceProvider::new(vec![
        text_events("Edit test"),
        tool_call_events(
            "call_1",
            "edit",
            serde_json::json!({"path": "test.txt", "oldString": "hello", "newString": "world"}),
        ),
        text_events("File updated."),
    ]);

    let deps = crate::CliDeps::real()
        .with_current_dir(temp.path().to_path_buf())
        .with_env(
            "HARNESS_DATA_HOME",
            temp.path().join("data").to_string_lossy(),
        )
        .with_provider_override(provider);
    let context = deps.config_load_context().unwrap_or_abort();

    let mut cmd = default_prompt_command();
    cmd.mock = true;
    cmd.text = Some("edit test.txt".to_string());
    cmd.permission_mode = Some("bypassPermissions".to_string());

    let settings = resolve_settings(
        &cmd,
        None,
        Some(temp.path().join("sessions")),
        temp.path().to_path_buf(),
        &context,
        &deps,
    )
    .unwrap_or_abort();

    let mut stdout = std::io::sink();
    let outcome = run_prompt(&cmd, &settings, "edit test.txt", &mut stdout)
        .await
        .unwrap_or_abort();

    let events_body = std::fs::read_to_string(&outcome.events_path).unwrap_or_abort();
    assert!(
        events_body.contains("tool_call_finished"),
        "expected tool_call_finished event: {events_body}"
    );
    assert!(
        events_body.contains("\"status\":\"succeeded\""),
        "expected tool call to succeed: {events_body}"
    );

    let file_content = std::fs::read_to_string(temp.path().join("test.txt")).unwrap_or_abort();
    assert_eq!(file_content, "world", "expected file to be modified");
}

#[tokio::test]
async fn run_prompt_readonly_sandbox_rejects_unadvertised_edit() -> Result<(), String> {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    std::fs::write(temp.path().join("test.txt"), "hello").unwrap_or_abort();

    let provider = SequenceProvider::new(vec![
        text_events("Edit test"),
        tool_call_events(
            "call_1",
            "edit",
            serde_json::json!({"path": "test.txt", "oldString": "hello", "newString": "world"}),
        ),
        text_events("Okay, I won't edit."),
    ]);

    let deps = crate::CliDeps::real()
        .with_current_dir(temp.path().to_path_buf())
        .with_env(
            "HARNESS_DATA_HOME",
            temp.path().join("data").to_string_lossy(),
        )
        .with_provider_override(provider);
    let context = deps.config_load_context().unwrap_or_abort();

    let mut cmd = default_prompt_command();
    cmd.mock = true;
    cmd.text = Some("edit test.txt".to_string());
    cmd.sandbox = Some("readonly".to_string());

    let settings = resolve_settings(
        &cmd,
        None,
        Some(temp.path().join("sessions")),
        temp.path().to_path_buf(),
        &context,
        &deps,
    )
    .unwrap_or_abort();

    // act
    let mut stdout = std::io::sink();
    let error = match run_prompt(&cmd, &settings, "edit test.txt", &mut stdout).await {
        Ok(_) => return Err("readonly prompt unexpectedly accepted unadvertised edit".to_string()),
        Err(error) => error,
    };

    // assert
    assert!(
        error.contains("provider emitted unmapped tool function `edit`"),
        "expected fail-closed unmapped-tool error: {error}"
    );

    let file_content = std::fs::read_to_string(temp.path().join("test.txt")).unwrap_or_abort();
    assert_eq!(file_content, "hello", "expected file to be unmodified");
    Ok(())
}

#[tokio::test]
async fn run_prompt_resume_appends_turn_to_existing_session() {
    // arrange
    // act
    // assert
    let temp = tempfile::tempdir().unwrap_or_abort();
    let session_dir = temp.path().join("sessions");
    let run_id = "run_resume_test";
    let run_dir = session_dir.join(run_id);
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();

    let seed_events = [
        EventEnvelopeV1 {
            schema_version: 1,
            event_id: "evt-00000000000000000001".to_string(),
            seq: 1,
            run_id: run_id.into(),
            mono_ms: 0,
            ts: None,
            actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
            correlation_id: None,
            causation_id: None,
            stream_key: None,
            payload: EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".into(),
                workspace_root: temp.path().to_string_lossy().to_string(),
            }),
        },
        EventEnvelopeV1 {
            schema_version: 1,
            event_id: "evt-00000000000000000002".to_string(),
            seq: 2,
            run_id: run_id.into(),
            mono_ms: 1,
            ts: None,
            actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
            correlation_id: None,
            causation_id: None,
            stream_key: None,
            payload: EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_000001".to_string(),
                profile: "default".to_string(),
                parent_agent_id: None,
            }),
        },
        EventEnvelopeV1 {
            schema_version: 1,
            event_id: "evt-00000000000000000003".to_string(),
            seq: 3,
            run_id: run_id.into(),
            mono_ms: 2,
            ts: None,
            actor: EventActor::new(ActorKind::User, None),
            correlation_id: Some("req_000001".to_string()),
            causation_id: None,
            stream_key: None,
            payload: EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_000001".into(),
                text: "Original prompt".to_string(),
            }),
        },
        EventEnvelopeV1 {
            schema_version: 1,
            event_id: "evt-00000000000000000004".to_string(),
            seq: 4,
            run_id: run_id.into(),
            mono_ms: 3,
            ts: None,
            actor: EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            correlation_id: Some("req_000001".to_string()),
            causation_id: None,
            stream_key: None,
            payload: EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_000001".into(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "Original prompt".to_string(),
                request_digest: "seed-digest".to_string(),
                metadata: None,
            }),
        },
        EventEnvelopeV1 {
            schema_version: 1,
            event_id: "evt-00000000000000000005".to_string(),
            seq: 5,
            run_id: run_id.into(),
            mono_ms: 4,
            ts: None,
            actor: EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            correlation_id: Some("req_000001".to_string()),
            causation_id: None,
            stream_key: None,
            payload: EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "req_000001".into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("seed-output".to_string()),
                usage: None,
                metadata: None,
            }),
        },
        EventEnvelopeV1 {
            schema_version: 1,
            event_id: "evt-00000000000000000006".to_string(),
            seq: 6,
            run_id: run_id.into(),
            mono_ms: 5,
            ts: None,
            actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
            correlation_id: None,
            causation_id: None,
            stream_key: None,
            payload: EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        },
    ];

    let body = seed_events
        .iter()
        .map(|e| serde_json::to_string(e).unwrap_or_abort())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(run_dir.join("events.jsonl"), format!("{body}\n")).unwrap_or_abort();

    let provider = SequenceProvider::new(vec![text_events("Hello again")]);
    let deps = crate::CliDeps::real()
        .with_current_dir(temp.path().to_path_buf())
        .with_env(
            "HARNESS_DATA_HOME",
            temp.path().join("data").to_string_lossy(),
        )
        .with_provider_override(provider);
    let context = deps.config_load_context().unwrap_or_abort();

    let mut resume_cmd = default_prompt_command();
    resume_cmd.mock = true;
    resume_cmd.text = Some("second prompt".to_string());
    resume_cmd.resume = Some(run_id.to_string());

    let resume_settings = resolve_settings(
        &resume_cmd,
        None,
        Some(session_dir),
        temp.path().to_path_buf(),
        &context,
        &deps,
    )
    .unwrap_or_abort();

    let mut stdout = std::io::sink();
    let resume_outcome =
        match run_prompt(&resume_cmd, &resume_settings, "second prompt", &mut stdout).await {
            Ok(o) => o,
            Err(e) => panic!("resume run_prompt failed: {e}"),
        };

    let resume_events = std::fs::read_to_string(&resume_outcome.events_path).unwrap_or_abort();
    assert!(
        resume_events.contains("task_completed"),
        "expected task_completed in resumed session: {resume_events}"
    );
    assert!(
        resume_events.contains("Hello again"),
        "expected scripted response in resumed session: {resume_events}"
    );
}

struct CountingEventStore {
    inner: InMemoryEventStore,
    subscribe_calls: AtomicUsize,
    replay_calls: AtomicUsize,
    runtime_subscribed: tokio::sync::Notify,
}

impl CountingEventStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            subscribe_calls: AtomicUsize::new(0),
            replay_calls: AtomicUsize::new(0),
            runtime_subscribed: tokio::sync::Notify::new(),
        }
    }

    fn subscribe_calls(&self) -> usize {
        self.subscribe_calls.load(Ordering::SeqCst)
    }

    fn replay_calls(&self) -> usize {
        self.replay_calls.load(Ordering::SeqCst)
    }

    async fn wait_until_runtime_subscribed(&self) {
        if self.subscribe_calls() == 0 {
            self.runtime_subscribed.notified().await;
        }
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
        self.inner.subscribe(from_seq)
    }

    fn subscribe_runtime(&self, from_seq: u64) -> Result<RuntimeEventStream, EventStoreError> {
        self.subscribe_calls.fetch_add(1, Ordering::SeqCst);
        let stream = self.inner.subscribe_runtime(from_seq);
        self.runtime_subscribed.notify_waiters();
        stream
    }

    fn publish_live(&self, envelope: LiveEventEnvelope) {
        self.inner.publish_live(envelope);
    }
}
