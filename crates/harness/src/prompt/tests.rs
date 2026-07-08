use crate::UnwrapOrAbort;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use harness_core::auth::{
    AuthProviderId, CredentialClock, CredentialStore, StoredCredential, SystemCredentialClock,
};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestFinishedEvent, RunFailedEvent,
    TaskCancelledEvent, TaskCompletedEvent, TaskCompletionMetadata, TaskLineageMetadata,
    TaskScheduleState, TaskScheduledEvent,
};
use harness_core::store::{
    EventEnvelopeWithoutSeqV1, EventStore, EventStoreError, EventStream, InMemoryEventStore,
};

use super::stream::{
    evaluate_prompt_completion, has_provider_error_finish, parse_wait_timeout_ms,
    wait_for_prompt_completion, PromptCompletionStatus, DEFAULT_WAIT_TIMEOUT,
};
use super::{resolve_settings, PromptCommand, PromptOutputFormat};

fn default_prompt_command() -> PromptCommand {
    PromptCommand {
        text: Some("hello".to_string()),
        stdin: false,
        message: Vec::new(),
        model: None,
        variant: None,
        thinking: false,
        mock: false,
        profile: None,
        resume: None,
        out: None,
        print_run_dir: false,
        format: PromptOutputFormat::Default,
    }
}

#[test]
fn no_config_prompt_without_provider_returns_connect_guidance() {
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
    assert_eq!(settings.default_profile, "build");
}

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

#[test]
fn evaluate_prompt_completion_waits_for_tool_task_completion() {
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
