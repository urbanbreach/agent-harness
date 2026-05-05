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
    spawn_coordinator, CoordinatorConfig, CoordinatorError, CoordinatorHandle, JobOutcome,
    JobProgressKind, ManualCompactionOutcome, RunInfo,
};
use harness_core::event::{
    ActorKind, AgentSpawnedEvent, EventActor, EventEnvelopeV1, EventV1, ExecutionTimingMetadata,
    HookExecutionMetadata, HookExecutionStatus, PermissionDecision as EventPermissionDecision,
    PermissionRequestedEvent, PermissionResolvedEvent, ProviderRequestStartedEvent,
    RunFinishedEvent, RunStartedEvent, TaskCancelledEvent, TaskCompletedEvent,
    TaskCompletionMetadata, TaskLineageMetadata, TaskResultLateEvent, TaskScheduleState,
    TaskScheduledEvent, ToolCallFinishedEvent, ToolCallMetadata, ToolCallRequestedEvent,
    ToolCallStatus, SCHEMA_VERSION,
};
use harness_core::perm::{PermissionDecision as RuntimePermissionDecision, PermissionPolicy};
use harness_core::proj::{inspect_resume_plan, ChildSessionTerminalState, LifecycleSegmentStatus};
use harness_core::redact::DefaultRedactor;
use harness_core::store::EventStoreError;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult};
use harness_providers::mock::{request_digest, MockProvider};
use harness_providers::{
    CompletionMessage, CompletionRequest, CompletionUsage, MessageRole, Provider,
    ProviderEventStream, ProviderStreamEvent, ProviderStreamFinishedMetadata,
    ProviderStreamStartMetadata, ProviderStreamThinkingMetadata,
};
use serde_json::json;
use tokio::sync::Notify;
use tokio_stream::StreamExt;

mod common;
use common::load_events;

struct TestShellTool;

#[async_trait]
impl Tool for TestShellTool {
    fn id(&self) -> &str {
        "shell.run"
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(
        &self,
        _ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::text(format!("ok {args_json}")))
    }
}

struct CountingShellTool {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for CountingShellTool {
    fn id(&self) -> &str {
        "shell.run"
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(
        &self,
        _ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ToolResult::text(format!("counted {args_json}")))
    }
}

struct FailingShellTool;

#[async_trait]
impl Tool for FailingShellTool {
    fn id(&self) -> &str {
        "shell.fail"
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(
        &self,
        _ctx: ToolContext,
        _args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        Err(ToolError::Execution("boom".to_string()))
    }
}

struct BlockingShellTool {
    release: Arc<Notify>,
}

#[async_trait]
impl Tool for BlockingShellTool {
    fn id(&self) -> &str {
        "shell.block"
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(
        &self,
        _ctx: ToolContext,
        _args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        self.release.notified().await;
        Ok(ToolResult::text("unblocked"))
    }
}

struct NamedShellTool {
    id: &'static str,
    output: &'static str,
    started: Option<Arc<Notify>>,
    release: Option<Arc<Notify>>,
}

#[async_trait]
impl Tool for NamedShellTool {
    fn id(&self) -> &str {
        self.id
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(
        &self,
        _ctx: ToolContext,
        _args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        if let Some(started) = &self.started {
            started.notify_one();
        }
        if let Some(release) = &self.release {
            release.notified().await;
        }
        Ok(ToolResult::text(self.output))
    }
}

#[derive(Clone)]
struct CapturingProvider {
    captured_requests: Arc<Mutex<Vec<CompletionRequest>>>,
    queued_responses: Arc<Mutex<VecDeque<String>>>,
}

impl CapturingProvider {
    fn new(responses: Vec<&str>) -> Self {
        Self {
            captured_requests: Arc::new(Mutex::new(Vec::new())),
            queued_responses: Arc::new(Mutex::new(
                responses.into_iter().map(str::to_string).collect(),
            )),
        }
    }

    fn requests(&self) -> Vec<CompletionRequest> {
        self.captured_requests
            .lock()
            .expect("capturing provider lock")
            .clone()
    }
}

#[async_trait]
impl Provider for CapturingProvider {
    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream {
        self.captured_requests
            .lock()
            .expect("capturing provider lock")
            .push(req);

        let response = self
            .queued_responses
            .lock()
            .expect("queued response lock")
            .pop_front()
            .unwrap_or_else(|| "ok".to_string());

        Box::pin(tokio_stream::iter(vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta(response),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 3,
                    completion_tokens: 3,
                    total_tokens: 6,
                },
            },
        ]))
    }
}

#[derive(Clone)]
struct DelayedCapturingProvider {
    inner: CapturingProvider,
    delay: Duration,
}

impl DelayedCapturingProvider {
    fn new(responses: Vec<&str>, delay: Duration) -> Self {
        Self {
            inner: CapturingProvider::new(responses),
            delay,
        }
    }

    fn requests(&self) -> Vec<CompletionRequest> {
        self.inner.requests()
    }
}

#[async_trait]
impl Provider for DelayedCapturingProvider {
    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream {
        let delay = self.delay;
        let stream = self
            .inner
            .stream_completion(req)
            .await
            .then(move |event| async move {
                tokio::time::sleep(delay).await;
                event
            });
        Box::pin(stream)
    }
}

#[derive(Clone)]
struct SequentialScriptedProvider {
    captured_requests: Arc<Mutex<Vec<CompletionRequest>>>,
    scripted_events: Arc<Vec<Vec<ProviderStreamEvent>>>,
    next_call_index: Arc<Mutex<usize>>,
}

impl SequentialScriptedProvider {
    fn new(scripted_events: Vec<Vec<ProviderStreamEvent>>) -> Self {
        Self {
            captured_requests: Arc::new(Mutex::new(Vec::new())),
            scripted_events: Arc::new(scripted_events),
            next_call_index: Arc::new(Mutex::new(0)),
        }
    }

    fn requests(&self) -> Vec<CompletionRequest> {
        self.captured_requests
            .lock()
            .expect("sequential scripted provider lock")
            .clone()
    }
}

#[async_trait]
impl Provider for SequentialScriptedProvider {
    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream {
        self.captured_requests
            .lock()
            .expect("sequential scripted provider lock")
            .push(req);

        let mut next_call_index = self
            .next_call_index
            .lock()
            .expect("sequential scripted call index");
        let call_index = *next_call_index;
        *next_call_index += 1;

        Box::pin(tokio_stream::iter(
            self.scripted_events
                .get(call_index)
                .cloned()
                .unwrap_or_else(|| {
                    vec![ProviderStreamEvent::Error {
                        message: format!("unexpected stream_completion call index {call_index}"),
                    }]
                }),
        ))
    }
}

#[tokio::test]
async fn coord_start_run_appends_run_started() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_coordinator(temp_dir.path());

    let run = coordinator
        .start_run("coord_start", PathBuf::from("/workspace/project"))
        .await
        .expect("start run");
    coordinator.stop_run().await.expect("stop run");

    assert!(run.run_dir.exists(), "run directory must exist");
    assert!(run.artifacts_dir.exists(), "artifacts directory must exist");
    assert!(run.events_path.exists(), "events log must exist");

    let events = load_events(&run.events_path);
    assert!(
        matches!(
            events.first().map(|event| &event.payload),
            Some(EventV1::RunStarted(_))
        ),
        "first event must be RunStarted"
    );
}

#[tokio::test]
async fn coord_spawn_agent_appends_agent_spawned() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_agent_coordinator(temp_dir.path(), Duration::from_millis(0));

    let run = coordinator
        .start_run("coord_spawn", PathBuf::from("/workspace/project"))
        .await
        .expect("start run");

    let actor = EventActor::new(ActorKind::Supervisor, Some("agent_supervisor".to_string()));
    let _agent_id = coordinator
        .spawn_agent(actor, "alpha", None)
        .await
        .expect("spawn agent");

    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    assert!(
        events
            .iter()
            .any(|event| matches!(event.payload, EventV1::AgentSpawned(_))),
        "expected AgentSpawned event"
    );
}

#[tokio::test]
async fn coord_stop_run_appends_run_finished() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_coordinator(temp_dir.path());

    let run = coordinator
        .start_run("coord_stop", PathBuf::from("/workspace/project"))
        .await
        .expect("start run");

    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    assert!(
        matches!(
            events.last().map(|event| &event.payload),
            Some(EventV1::RunFinished(_))
        ),
        "last event must be RunFinished"
    );
}

#[tokio::test]
async fn coord_event_store_subscribe_emits_live_events() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_coordinator(temp_dir.path());

    let _run = coordinator
        .start_run("coord_live_subscribe", PathBuf::from("/workspace/project"))
        .await
        .expect("start run");

    let store = coordinator.event_store().await.expect("get event store");
    let mut stream = store.subscribe(2).expect("subscribe from live boundary");

    coordinator.stop_run().await.expect("stop run");

    let event = tokio::time::timeout(Duration::from_secs(1), stream.next())
        .await
        .expect("event should arrive")
        .expect("stream should produce item")
        .expect("stream item should be valid");

    assert!(matches!(event.payload, EventV1::RunFinished(_)));
}

#[tokio::test]
async fn coord_worker_spawn_attempt_records_policy_violation_and_returns_error() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_agent_coordinator(temp_dir.path(), Duration::from_millis(0));

    let run = coordinator
        .start_run("coord_policy", PathBuf::from("/workspace/project"))
        .await
        .expect("start run");

    let actor = EventActor::new(ActorKind::Worker, Some("agent_worker".to_string()));
    let result = coordinator.spawn_agent(actor, "alpha", None).await;
    assert!(result.is_err(), "worker spawn must fail");

    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    assert!(
        events
            .iter()
            .any(|event| matches!(event.payload, EventV1::PolicyViolationDetected(_))),
        "expected PolicyViolationDetected event"
    );
}

#[tokio::test]
async fn coord_spawn_two_agents_respects_provider_concurrency_and_queues() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_agent_coordinator(temp_dir.path(), Duration::from_millis(25));

    let run = coordinator
        .start_run("coord_agents_queue", PathBuf::from("/workspace/project"))
        .await
        .expect("start run");

    let actor = EventActor::new(ActorKind::Supervisor, Some("agent_supervisor".to_string()));
    let spawn_a = coordinator.spawn_agent(actor.clone(), "alpha", None);
    let spawn_b = coordinator.spawn_agent(actor, "beta", None);
    let (_agent_a, _agent_b) = tokio::join!(spawn_a, spawn_b);

    tokio::time::sleep(Duration::from_millis(250)).await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let queued = events
        .iter()
        .filter(|event| {
            matches!(
                event.payload,
                EventV1::TaskScheduled(ref data)
                    if data.state == harness_core::event::TaskScheduleState::Queued
            )
        })
        .count();
    let completed = events
        .iter()
        .filter(|event| matches!(event.payload, EventV1::TaskCompleted(_)))
        .count();

    assert!(
        queued >= 1,
        "expected at least one queued task for concurrency limit 1"
    );
    assert_eq!(completed, 2, "both spawned agents should complete");
}

#[tokio::test]
async fn coord_spawn_unknown_profile_returns_error() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_coordinator(temp_dir.path());

    let run = coordinator
        .start_run("coord_unknown_profile", PathBuf::from("/workspace/project"))
        .await
        .expect("start run");

    let actor = EventActor::new(ActorKind::Supervisor, Some("agent_supervisor".to_string()));
    let err = coordinator
        .spawn_agent(actor, "missing_profile", None)
        .await
        .expect_err("unknown profile should fail");

    assert!(matches!(err, CoordinatorError::UnknownAgent(profile) if profile == "missing_profile"));

    coordinator.stop_run().await.expect("stop run");
    let events = load_events(&run.events_path);
    assert!(
        !events.iter().any(|event| matches!(
            &event.payload,
            EventV1::AgentSpawned(AgentSpawnedEvent { profile, .. }) if profile == "missing_profile"
        )),
        "unknown profiles should not emit AgentSpawned events"
    );
}

#[tokio::test]
async fn coordinator_runs_parallel_child_sessions_under_slot_limits() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = Arc::new(PromptScriptedProvider::new(
        BTreeMap::from([
            (
                "alpha-prompt".to_string(),
                vec![
                    ProviderStreamEvent::Start,
                    ProviderStreamEvent::TextDelta("alpha-ok".to_string()),
                    ProviderStreamEvent::Done {
                        usage: CompletionUsage {
                            prompt_tokens: 2,
                            completion_tokens: 1,
                            total_tokens: 3,
                        },
                    },
                ],
            ),
            (
                "beta-prompt".to_string(),
                vec![
                    ProviderStreamEvent::Start,
                    ProviderStreamEvent::TextDelta("beta-ok".to_string()),
                    ProviderStreamEvent::Done {
                        usage: CompletionUsage {
                            prompt_tokens: 2,
                            completion_tokens: 1,
                            total_tokens: 3,
                        },
                    },
                ],
            ),
        ]),
        Duration::from_millis(40),
    ));
    let coordinator = test_agent_coordinator_with_provider(temp_dir.path(), provider, 2);

    let run = coordinator
        .start_run(
            "coord_parallel_child_sessions_under_limits",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");

    let actor = supervisor_actor();
    let _alpha = coordinator
        .spawn_agent(actor.clone(), "alpha", None)
        .await
        .expect("spawn alpha");
    let _beta = coordinator
        .spawn_agent(actor.clone(), "beta", None)
        .await
        .expect("spawn beta");
    let _beta_two = coordinator
        .spawn_agent(actor, "beta", None)
        .await
        .expect("spawn second beta");

    tokio::time::sleep(Duration::from_millis(700)).await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let scheduled = events
        .iter()
        .enumerate()
        .filter_map(|(idx, event)| match &event.payload {
            EventV1::TaskScheduled(data)
                if data.queue_key.as_deref() == Some("provider_model:mock:model-1") =>
            {
                Some((idx, data.clone()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(
        scheduled.len(),
        4,
        "three tasks should yield one queued+restarted record"
    );

    let first_three_states = scheduled
        .iter()
        .take(3)
        .map(|(_, data)| data.state)
        .collect::<Vec<_>>();
    assert_eq!(
        first_three_states,
        vec![
            TaskScheduleState::Started,
            TaskScheduleState::Started,
            TaskScheduleState::Queued,
        ],
        "limit=2 should start two child sessions and deterministically queue the third"
    );

    let started = scheduled
        .iter()
        .filter(|(_, data)| data.state == TaskScheduleState::Started)
        .map(|(_, data)| data.task_id.clone())
        .collect::<Vec<_>>();
    let queued = scheduled
        .iter()
        .filter(|(_, data)| data.state == TaskScheduleState::Queued)
        .map(|(_, data)| data.task_id.clone())
        .collect::<Vec<_>>();

    assert_eq!(
        started.len(),
        3,
        "all three child tasks should eventually start"
    );
    assert_eq!(
        queued.len(),
        1,
        "exactly one child task should queue at saturation"
    );
    assert_eq!(
        started
            .iter()
            .filter(|task_id| *task_id == &queued[0])
            .count(),
        1,
        "queued task should later transition to started once a slot frees"
    );

    let scheduled_task_ids = scheduled
        .iter()
        .map(|(_, data)| data.task_id.clone())
        .collect::<BTreeSet<_>>();
    let completed = events
        .iter()
        .filter(|event| {
            matches!(&event.payload, EventV1::TaskCompleted(data) if scheduled_task_ids.contains(&data.task_id))
        })
        .count();
    assert_eq!(
        completed, 3,
        "all child sessions should complete under limit=2"
    );
}

#[tokio::test]
async fn coordinator_isolates_parallel_child_failures() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = Arc::new(PromptScriptedProvider::new(
        BTreeMap::from([
            (
                "alpha-prompt".to_string(),
                vec![
                    ProviderStreamEvent::Start,
                    ProviderStreamEvent::Error {
                        message: "alpha child failed".to_string(),
                    },
                ],
            ),
            (
                "beta-prompt".to_string(),
                vec![
                    ProviderStreamEvent::Start,
                    ProviderStreamEvent::TextDelta("beta child ok".to_string()),
                    ProviderStreamEvent::Done {
                        usage: CompletionUsage {
                            prompt_tokens: 2,
                            completion_tokens: 1,
                            total_tokens: 3,
                        },
                    },
                ],
            ),
        ]),
        Duration::from_millis(40),
    ));
    let coordinator = test_agent_coordinator_with_provider(temp_dir.path(), provider, 2);

    let run = coordinator
        .start_run(
            "coord_parallel_child_failure_isolation",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");

    let actor = supervisor_actor();
    let _alpha = coordinator
        .spawn_agent(actor.clone(), "alpha", None)
        .await
        .expect("spawn alpha");
    let _beta = coordinator
        .spawn_agent(actor, "beta", None)
        .await
        .expect("spawn beta");

    tokio::time::sleep(Duration::from_millis(500)).await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let scheduled_task_ids = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::TaskScheduled(data)
                if data.queue_key.as_deref() == Some("provider_model:mock:model-1")
                    && data.state == TaskScheduleState::Started =>
            {
                Some(data.task_id.clone())
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        scheduled_task_ids.len(),
        2,
        "both child sessions should start in parallel under limit=2"
    );

    let queued = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventV1::TaskScheduled(data)
                    if data.queue_key.as_deref() == Some("provider_model:mock:model-1")
                        && data.state == TaskScheduleState::Queued
            )
        })
        .count();
    assert_eq!(
        queued, 0,
        "no queueing expected with two slots and two children"
    );

    let completed = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::TaskCompleted(data) if scheduled_task_ids.contains(&data.task_id) => {
                Some(data.result_summary.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let cancelled = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::TaskCancelled(data) if scheduled_task_ids.contains(&data.task_id) => {
                Some(data.reason.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(completed.len(), 1, "one sibling should still complete");
    assert_eq!(cancelled.len(), 1, "one sibling failure should be isolated");
    assert!(
        completed
            .iter()
            .any(|summary| summary.contains("beta child ok")),
        "beta sibling should complete despite alpha failure"
    );
    assert!(
        cancelled
            .iter()
            .any(|reason| reason.contains("alpha child failed")),
        "alpha failure should be recorded without cancelling sibling"
    );
}

#[tokio::test]
async fn immediate_agent_turn_emits_single_started_event() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_agent_coordinator(temp_dir.path(), Duration::from_millis(5));

    let run = coordinator
        .start_run(
            "coord_agent_turn_started_immediate",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");

    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "alpha-prompt")
        .await
        .expect("request immediate turn");

    tokio::time::sleep(Duration::from_millis(120)).await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let scheduled: Vec<_> = events
        .iter()
        .enumerate()
        .filter_map(|(idx, event)| match &event.payload {
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str()) =>
            {
                Some((idx, event, data))
            }
            _ => None,
        })
        .collect();

    let started: Vec<_> = scheduled
        .iter()
        .filter(|(_, _, data)| data.state == TaskScheduleState::Started)
        .collect();
    let queued: Vec<_> = scheduled
        .iter()
        .filter(|(_, _, data)| data.state == TaskScheduleState::Queued)
        .collect();

    assert_eq!(
        started.len(),
        1,
        "immediate turns should emit one started event"
    );
    assert!(
        queued.is_empty(),
        "immediate turns should not emit queued events"
    );

    let (started_idx, started_event, started_data) = *started[0];
    assert_eq!(started_event.actor.kind, ActorKind::Worker);
    assert_eq!(
        started_event.actor.agent_id.as_deref(),
        Some(agent_id.as_str())
    );

    let provider_started_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ProviderRequestStarted(_) if event.correlation_id.as_deref() == Some(request_id.as_str())
            )
        })
        .expect("provider request started event");
    assert!(
        started_idx < provider_started_idx,
        "started scheduling event should precede provider execution"
    );

    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCompleted(data)
                if data.task_id == started_data.task_id
                    && event.correlation_id.as_deref() == Some(request_id.as_str())
        )
    }));
}

#[tokio::test]
async fn queued_agent_turn_emits_started_when_dequeued() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_agent_coordinator(temp_dir.path(), Duration::from_millis(25));

    let run = coordinator
        .start_run(
            "coord_agent_turn_started_queued",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");

    let alpha = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let beta = coordinator
        .spawn_agent_idle(supervisor_actor(), "beta", None)
        .await
        .expect("spawn idle beta");

    let _first_request_id = coordinator
        .request_agent_turn(supervisor_actor(), alpha, "alpha-prompt")
        .await
        .expect("request first turn");
    let queued_request_id = coordinator
        .request_agent_turn(supervisor_actor(), beta.clone(), "beta-prompt")
        .await
        .expect("request queued turn");

    tokio::time::sleep(Duration::from_millis(250)).await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let scheduled: Vec<_> = events
        .iter()
        .enumerate()
        .filter_map(|(idx, event)| match &event.payload {
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(queued_request_id.as_str()) =>
            {
                Some((idx, event, data))
            }
            _ => None,
        })
        .collect();

    assert_eq!(
        scheduled.len(),
        2,
        "queued turns should emit queued then started"
    );
    assert_eq!(scheduled[0].2.state, TaskScheduleState::Queued);
    assert_eq!(scheduled[1].2.state, TaskScheduleState::Started);
    assert_eq!(scheduled[0].2.task_id, scheduled[1].2.task_id);

    for (_, event, _) in &scheduled {
        assert_eq!(event.actor.kind, ActorKind::Worker);
        assert_eq!(event.actor.agent_id.as_deref(), Some(beta.as_str()));
    }

    let provider_started_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ProviderRequestStarted(_) if event.correlation_id.as_deref() == Some(queued_request_id.as_str())
            )
        })
        .expect("provider request started event");
    assert!(
        scheduled[1].0 < provider_started_idx,
        "dequeue-time started event should be emitted before execution begins"
    );

    let task_id = scheduled[0].2.task_id.clone();
    let completed_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if data.task_id == task_id
                        && event.correlation_id.as_deref() == Some(queued_request_id.as_str())
            )
        })
        .expect("task completed event");
    assert!(provider_started_idx < completed_idx);
}

#[tokio::test]
async fn same_agent_turn_queues_even_when_provider_model_has_free_slots() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_agent_coordinator_with_provider(
        temp_dir.path(),
        Arc::new(SlowMockProvider {
            inner: test_mock_provider(),
            delay: Duration::from_millis(150),
        }),
        2,
    );

    let run = coordinator
        .start_run(
            "coord_same_agent_turn_queues",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");

    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");

    let first_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first prompt")
        .await
        .expect("request first turn");
    let queued_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "queued prompt")
        .await
        .expect("request queued turn");

    tokio::time::sleep(Duration::from_millis(50)).await;

    let events = load_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(queued_request_id.as_str())
                    && data.state == TaskScheduleState::Queued
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ProviderRequestStarted(_)
                if event.correlation_id.as_deref() == Some(queued_request_id.as_str())
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(first_request_id.as_str())
                    && data.state == TaskScheduleState::Started
        )
    }));

    tokio::time::sleep(Duration::from_millis(350)).await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let queued_schedule_states = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(queued_request_id.as_str()) =>
            {
                Some(data.state)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        queued_schedule_states,
        vec![TaskScheduleState::Queued, TaskScheduleState::Started]
    );
}

#[tokio::test]
async fn same_agent_blocked_turns_start_fifo() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_agent_coordinator_with_provider(
        temp_dir.path(),
        Arc::new(SlowMockProvider {
            inner: test_mock_provider(),
            delay: Duration::from_millis(40),
        }),
        2,
    );

    let run = coordinator
        .start_run("coord_same_agent_fifo", PathBuf::from("/workspace/project"))
        .await
        .expect("start run");

    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");

    let first_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first prompt")
        .await
        .expect("request first turn");
    let second_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "second prompt")
        .await
        .expect("request second turn");
    let third_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "third prompt")
        .await
        .expect("request third turn");

    let events = wait_for_events(&run.events_path, Duration::from_secs(2), |events| {
        provider_started_request_ids(events).len() >= 3
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    let started_request_ids = provider_started_request_ids(&events);
    let expected = vec![first_request_id, second_request_id, third_request_id];
    assert_eq!(started_request_ids, expected);
}

#[tokio::test]
async fn cancelling_promoted_same_agent_queued_turn_promotes_next_blocked_turn() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_agent_coordinator_with_provider(
        temp_dir.path(),
        Arc::new(SlowMockProvider {
            inner: test_mock_provider(),
            delay: Duration::from_millis(250),
        }),
        1,
    );

    let run = coordinator
        .start_run(
            "coord_same_agent_cancel_promoted",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");

    let alpha = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let beta = coordinator
        .spawn_agent_idle(supervisor_actor(), "beta", None)
        .await
        .expect("spawn idle beta");

    let _alpha_first = coordinator
        .request_agent_turn(supervisor_actor(), alpha.clone(), "alpha first")
        .await
        .expect("request alpha first");
    let beta_request_id = coordinator
        .request_agent_turn(supervisor_actor(), beta, "beta first")
        .await
        .expect("request beta");
    let alpha_second = coordinator
        .request_agent_turn(supervisor_actor(), alpha.clone(), "alpha second")
        .await
        .expect("request alpha second");
    let alpha_third = coordinator
        .request_agent_turn(supervisor_actor(), alpha, "alpha third")
        .await
        .expect("request alpha third");

    let events = wait_for_events(&run.events_path, Duration::from_secs(5), |events| {
        task_schedule_states_for_request(events, &alpha_second) == vec![TaskScheduleState::Queued]
            && events.iter().any(|event| {
                matches!(
                    &event.payload,
                    EventV1::TaskScheduled(data)
                        if event.correlation_id.as_deref() == Some(beta_request_id.as_str())
                            && data.state == TaskScheduleState::Started
                )
            })
    })
    .await;
    let alpha_second_task_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(alpha_second.as_str()) =>
            {
                Some(data.task_id.clone())
            }
            _ => None,
        })
        .expect("alpha second queued task id");

    coordinator
        .cancel_task(alpha_second_task_id, "skip promoted same-agent prompt")
        .await
        .expect("cancel promoted alpha turn");

    let events = wait_for_events(&run.events_path, Duration::from_secs(5), |events| {
        task_schedule_states_for_request(events, &alpha_third)
            == vec![TaskScheduleState::Queued, TaskScheduleState::Started]
            && events.iter().any(|event| {
                matches!(
                    &event.payload,
                    EventV1::ProviderRequestStarted(_)
                        if event.correlation_id.as_deref() == Some(alpha_third.as_str())
                )
            })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ProviderRequestStarted(_)
                if event.correlation_id.as_deref() == Some(alpha_second.as_str())
        )
    }));
}

#[tokio::test]
async fn queued_agent_turn_cancellation_preserves_owner_context() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_agent_coordinator(temp_dir.path(), Duration::from_millis(25));

    let run = coordinator
        .start_run(
            "coord_agent_turn_cancel_queued",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");

    let alpha = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let beta = coordinator
        .spawn_agent_idle(supervisor_actor(), "beta", None)
        .await
        .expect("spawn idle beta");

    let _running_request_id = coordinator
        .request_agent_turn(supervisor_actor(), alpha, "alpha-prompt")
        .await
        .expect("request running turn");
    let queued_request_id = coordinator
        .request_agent_turn(supervisor_actor(), beta.clone(), "beta-prompt")
        .await
        .expect("request queued turn");

    let task_id = load_events(&run.events_path)
        .into_iter()
        .find_map(|event| match event.payload {
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(queued_request_id.as_str())
                    && data.state == TaskScheduleState::Queued =>
            {
                Some(data.task_id)
            }
            _ => None,
        })
        .expect("queued agent task id");

    coordinator
        .cancel_task(task_id.clone(), "manual queued cancellation")
        .await
        .expect("cancel queued agent turn");

    tokio::time::sleep(Duration::from_millis(200)).await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let cancellations = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data) if data.task_id == task_id
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        cancellations.len(),
        1,
        "queued turn should emit one cancellation"
    );
    assert_task_event_context(
        cancellations[0],
        &EventActor::new(ActorKind::Worker, Some(beta)),
        &queued_request_id,
    );
    assert!(matches!(
        &cancellations[0].payload,
        EventV1::TaskCancelled(data) if data.reason == "manual queued cancellation"
    ));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskScheduled(data)
                if data.task_id == task_id && data.state == TaskScheduleState::Started
        )
    }));
}

#[tokio::test]
async fn running_agent_turn_cancellation_emits_single_owner_aware_terminal_event() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_agent_coordinator(temp_dir.path(), Duration::from_millis(100));

    let run = coordinator
        .start_run(
            "coord_agent_turn_cancel_running",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");

    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "alpha-prompt")
        .await
        .expect("request running turn");

    let task_id = load_events(&run.events_path)
        .into_iter()
        .find_map(|event| match event.payload {
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str()) =>
            {
                Some(data.task_id)
            }
            _ => None,
        })
        .expect("running agent task id");

    coordinator
        .cancel_task(task_id.clone(), "manual running cancellation")
        .await
        .expect("cancel running agent turn");

    tokio::time::sleep(Duration::from_millis(250)).await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let terminal_events = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data) if data.task_id == task_id
            ) || matches!(
                &event.payload,
                EventV1::TaskCompleted(data) if data.task_id == task_id
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        terminal_events.len(),
        1,
        "running turn should emit exactly one terminal event"
    );
    assert!(matches!(
        &terminal_events[0].payload,
        EventV1::TaskCancelled(data) if data.reason == "manual running cancellation"
    ));
    assert_task_event_context(
        terminal_events[0],
        &EventActor::new(ActorKind::Worker, Some(agent_id)),
        &request_id,
    );
    let cancelled = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data) if data.task_id == task_id
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(cancelled.len(), 1, "running turn should not double-cancel");
    assert!(cancelled
        .iter()
        .all(|event| event.actor.kind == ActorKind::Worker));
}

#[tokio::test]
async fn coord_agent_turn_provider_events_have_isolated_correlation_ids() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_agent_coordinator(temp_dir.path(), Duration::from_millis(5));

    let run = coordinator
        .start_run("coord_agent_corr", PathBuf::from("/workspace/project"))
        .await
        .expect("start run");

    let actor = EventActor::new(ActorKind::Supervisor, Some("agent_supervisor".to_string()));
    let _ = coordinator
        .spawn_agent(actor.clone(), "alpha", None)
        .await
        .expect("spawn alpha");
    let _ = coordinator
        .spawn_agent(actor, "beta", None)
        .await
        .expect("spawn beta");

    tokio::time::sleep(Duration::from_millis(200)).await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let mut provider_request_ids = Vec::new();

    for event in &events {
        if let EventV1::ProviderRequestStarted(ref data) = event.payload {
            provider_request_ids.push(data.request_id.clone());
        }
    }

    provider_request_ids.sort();
    provider_request_ids.dedup();
    assert_eq!(
        provider_request_ids.len(),
        2,
        "expected one provider request_id per spawned agent"
    );

    for provider_request_id in provider_request_ids {
        let related: Vec<_> = events
            .iter()
            .filter(|event| match &event.payload {
                EventV1::ProviderRequestStarted(data) => data.request_id == provider_request_id,
                EventV1::ProviderStreamDelta(data) => data.request_id == provider_request_id,
                EventV1::ProviderRequestFinished(data) => data.request_id == provider_request_id,
                _ => false,
            })
            .collect();

        assert!(
            !related.is_empty(),
            "request {} should have related provider events",
            provider_request_id
        );
        let turn_id = related[0]
            .correlation_id
            .as_deref()
            .expect("provider event correlation should be stable turn id");
        assert!(related
            .iter()
            .all(|event| event.correlation_id.as_deref() == Some(turn_id)));
        assert_ne!(
            turn_id, provider_request_id,
            "provider request id should be distinct from stable turn correlation id"
        );
    }
}

#[tokio::test]
async fn provider_single_call_returns_tool_intents_without_executing_tools() {
    let tool_call_count = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(CountingShellTool {
        calls: tool_call_count.clone(),
    }));
    let tool_registry = Arc::new(registry);

    let profile = AgentProfile {
        name: "alpha".to_string(),
        category: "deep".to_string(),
        model_ref: "mock:model-1".to_string(),
        system_prompt: "single-call-system".to_string(),
        temperature: Some(0.0),
        max_iters: Some(12),
        tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
        toolset: vec!["shell.run".to_string()],
    };
    let request = AgentRequest {
        agent_id: "agent_1".to_string(),
        prompt: "single provider call".to_string(),
        model_ref: "mock:model-1".to_string(),
        model_settings: AgentModelSettings::default(),
    };
    let tool_defs = build_provider_tool_defs(&profile, tool_registry.as_ref())
        .expect("build provider tool defs");
    let function_name = tool_defs.first().expect("tool def").function_name.clone();
    let messages =
        build_provider_context_messages(&profile, &ProviderContext::default(), &request.prompt);
    let expected_request = CompletionRequest {
        provider_id: Some("mock".to_string()),
        model_id: "model-1".to_string(),
        messages: messages.clone(),
        temperature: Some(0.0),
        max_tokens: None,
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        tools: Some(tool_defs.clone()),
        tool_choice: Some(harness_providers::ToolChoice::Auto),
        stream: true,
    };
    let mut scripted = BTreeMap::new();
    scripted.insert(
        request_digest(&expected_request),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::ReasoningDelta("thinking".to_string()),
            ProviderStreamEvent::TextDelta("I will call tools".to_string()),
            ProviderStreamEvent::ToolCallDelta {
                tool_call_id: "first_call".to_string(),
                function_name: Some(function_name.clone()),
                arguments_delta: "{".to_string(),
            },
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "first_call".to_string(),
                function_name: function_name.clone(),
                arguments_json: r#"{"cmd":"one"}"#.to_string(),
            },
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "second_call".to_string(),
                function_name: function_name.clone(),
                arguments_json: r#"{"cmd":"two"}"#.to_string(),
            },
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 2,
                    completion_tokens: 3,
                    total_tokens: 5,
                },
            },
        ],
    );
    let provider = Arc::new(MockProvider::new(scripted));
    let events = Arc::new(Mutex::new(Vec::<AgentRuntimeEvent>::new()));

    let response = stream_assistant_response_once(
        StreamAssistantResponseOnceRequest {
            provider,
            profile: &profile,
            model: AgentModelRef::parse(&request.model_ref),
            model_settings: request.model_settings.clone(),
            turn_request_id: "turn_1".to_string(),
            provider_request_id: "provider_call_1".to_string(),
            prompt_summary: &request.prompt,
            context: ProviderBoundaryContext::ProviderMessages {
                messages: &messages,
            },
            tool_defs: &tool_defs,
        },
        {
            let events = events.clone();
            move |event| {
                let events = events.clone();
                async move {
                    events.lock().expect("lock events").push(event);
                }
            }
        },
    )
    .await
    .expect("single provider call succeeds");

    assert_eq!(tool_call_count.load(Ordering::SeqCst), 0);
    assert_eq!(response.text, "I will call tools");
    assert_eq!(response.reasoning, "thinking");
    assert_eq!(response.stop_reason, "done");
    assert_eq!(response.tool_call_deltas.len(), 1);
    assert_eq!(
        response
            .tool_intents
            .iter()
            .map(|intent| (
                intent.tool_call_id.as_str(),
                intent.function_name.as_str(),
                intent.tool_id.as_str(),
                intent.arguments.clone(),
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                "first_call",
                function_name.as_str(),
                "shell.run",
                json!({"cmd": "one"}),
            ),
            (
                "second_call",
                function_name.as_str(),
                "shell.run",
                json!({"cmd": "two"}),
            ),
        ]
    );

    let events = events.lock().expect("lock events");
    assert!(events.iter().any(|event| matches!(
        event,
        AgentRuntimeEvent::ProviderRequestStarted(started)
            if started.request_id == "provider_call_1"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        AgentRuntimeEvent::ProviderReasoningDelta { request_id, delta }
            if request_id == "provider_call_1" && delta == "thinking"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        AgentRuntimeEvent::ProviderRequestFinished(finished)
            if finished.request_id == "provider_call_1" && finished.finish_reason == "done"
    )));
}

#[tokio::test]
async fn provider_calls_in_one_turn_have_unique_request_ids() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "provider_call_1".to_string(),
                function_name: "shell_run".to_string(),
                arguments_json: "{}".to_string(),
            },
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 2,
                    completion_tokens: 1,
                    total_tokens: 3,
                },
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("final answer".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 3,
                    completion_tokens: 2,
                    total_tokens: 5,
                },
            },
        ],
    ]);
    let coordinator = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(provider.clone()),
        test_tool_registry(),
        PermissionPolicy::new(
            PermissionMode::Deny,
            PermissionMode::Allow,
            PermissionMode::Deny,
        ),
        vec!["shell.run".to_string()],
        12,
    );

    let run = coordinator
        .start_run(
            "coord_provider_unique_request_ids_in_turn",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");

    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let turn_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "needs a tool")
        .await
        .expect("request agent turn");

    wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(turn_request_id.as_str())
                        && data.result_summary == "final answer"
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let provider_started = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::ProviderRequestStarted(data)
                if event.correlation_id.as_deref() == Some(turn_request_id.as_str()) =>
            {
                Some(data.request_id.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(
        provider_started.len(),
        2,
        "one logical agent turn with one tool result should make two provider calls"
    );
    let unique_provider_request_ids = provider_started.iter().collect::<BTreeSet<_>>();
    assert_eq!(
        unique_provider_request_ids.len(),
        provider_started.len(),
        "each provider call should have its own provider request id while event correlation remains on the stable agent turn request id"
    );
    for event in events.iter().filter(|event| {
        matches!(
            &event.payload,
            EventV1::ProviderRequestStarted(data)
                if event.correlation_id.as_deref() == Some(turn_request_id.as_str())
                    && provider_started.contains(&data.request_id)
        )
    }) {
        let EventV1::ProviderRequestStarted(data) = &event.payload else {
            unreachable!("filtered provider start events")
        };
        let metadata = data.metadata.as_ref().expect("provider start metadata");
        assert_eq!(metadata.turn_id.as_deref(), Some(turn_request_id.as_str()));
        assert_eq!(
            metadata.provider_call_id.as_deref(),
            Some(data.request_id.as_str())
        );
    }
    assert!(events.iter().all(|event| {
        let provider_request_id = match &event.payload {
            EventV1::ProviderRequestStarted(data) => Some(&data.request_id),
            EventV1::ProviderStreamDelta(data) => Some(&data.request_id),
            EventV1::ProviderRequestFinished(data) => Some(&data.request_id),
            _ => None,
        };

        provider_request_id.is_none_or(|request_id| {
            !provider_started.contains(request_id)
                || event.correlation_id.as_deref() == Some(turn_request_id.as_str())
        })
    }));
}

#[tokio::test]
async fn completed_tool_turn_preserves_tool_messages_for_followup_context() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "call_edit".to_string(),
                function_name: "shell_run".to_string(),
                arguments_json: r#"{"command":"touch docs/rust-language.md"}"#.to_string(),
            },
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 2,
                    completion_tokens: 1,
                    total_tokens: 3,
                },
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("I edited docs/rust-language.md.".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 3,
                    completion_tokens: 2,
                    total_tokens: 5,
                },
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("I used shell.run.".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 4,
                    completion_tokens: 2,
                    total_tokens: 6,
                },
            },
        ],
    ]);
    let coordinator = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(provider.clone()),
        test_tool_registry(),
        PermissionPolicy::new(
            PermissionMode::Allow,
            PermissionMode::Allow,
            PermissionMode::Allow,
        ),
        vec!["shell.run".to_string()],
        12,
    );

    let run = coordinator
        .start_run(
            "coord_completed_tool_turn_preserves_tool_messages",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");

    coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let first_request_id = coordinator
        .request_agent_turn(
            supervisor_actor(),
            "agent_000001",
            "edit docs/rust-language.md",
        )
        .await
        .expect("first turn");
    wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(first_request_id.as_str())
                        && data.result_summary == "I edited docs/rust-language.md."
            )
        })
    })
    .await;

    coordinator
        .request_agent_turn(supervisor_actor(), "agent_000001", "what tool did you use?")
        .await
        .expect("follow-up turn");
    wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data) if data.result_summary == "I used shell.run."
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        3,
        "tool turn plus follow-up should make three provider calls"
    );
    let followup_messages = &requests[2].messages;
    let tool_call_message = followup_messages
        .iter()
        .find(|message| {
            message
                .assistant_tool_calls
                .as_ref()
                .is_some_and(|calls| calls.iter().any(|call| call.tool_call_id == "call_edit"))
        })
        .expect("follow-up context should include prior assistant tool call");
    let calls = tool_call_message
        .assistant_tool_calls
        .as_ref()
        .expect("assistant tool calls");
    assert_eq!(calls[0].function_name, "shell_run");
    assert_eq!(
        calls[0].arguments_json,
        r#"{"command":"touch docs/rust-language.md"}"#
    );

    let tool_result_message = followup_messages
        .iter()
        .find(|message| {
            message.role == MessageRole::Tool
                && message.tool_call_id.as_deref() == Some("call_edit")
        })
        .expect("follow-up context should include prior tool result");
    assert_eq!(tool_result_message.name.as_deref(), Some("shell_run"));
    assert!(tool_result_message
        .content
        .contains("touch docs/rust-language.md"));
    assert!(followup_messages.iter().any(|message| {
        message.role == MessageRole::Assistant
            && message.content == "I edited docs/rust-language.md."
            && message.assistant_tool_calls.is_none()
    }));
}

#[tokio::test]
async fn resumed_tool_turn_preserves_tool_messages_for_followup_context() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let initial_provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "call_edit".to_string(),
                function_name: "shell_run".to_string(),
                arguments_json: r#"{"command":"touch docs/rust-language.md"}"#.to_string(),
            },
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 2,
                    completion_tokens: 1,
                    total_tokens: 3,
                },
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("I edited docs/rust-language.md.".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 3,
                    completion_tokens: 2,
                    total_tokens: 5,
                },
            },
        ],
    ]);
    let initial = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(initial_provider),
        test_tool_registry(),
        PermissionPolicy::new(
            PermissionMode::Allow,
            PermissionMode::Allow,
            PermissionMode::Allow,
        ),
        vec!["shell.run".to_string()],
        12,
    );

    let run = initial
        .start_run(
            "coord_resumed_tool_turn_preserves_tool_messages",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    initial
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let first_request_id = initial
        .request_agent_turn(
            supervisor_actor(),
            "agent_000001",
            "edit docs/rust-language.md",
        )
        .await
        .expect("first turn");
    wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(first_request_id.as_str())
                        && data.result_summary == "I edited docs/rust-language.md."
            )
        })
    })
    .await;
    initial.stop_run().await.expect("stop initial run");

    let resumed_provider = CapturingProvider::new(vec!["I used shell.run."]);
    let resumed =
        test_resume_coordinator_with_provider(temp_dir.path(), Arc::new(resumed_provider.clone()));
    resumed
        .resume_run(&run.run_id, "interactive")
        .await
        .expect("resume run");
    resumed
        .request_agent_turn(supervisor_actor(), "agent_000001", "what tool did you use?")
        .await
        .expect("follow-up turn");
    tokio::time::sleep(Duration::from_millis(120)).await;
    resumed.stop_run().await.expect("stop resumed run");

    let requests = resumed_provider.requests();
    assert_eq!(requests.len(), 1, "expected one resumed provider request");
    let followup_messages = &requests[0].messages;
    let tool_call_message = followup_messages
        .iter()
        .find(|message| {
            message
                .assistant_tool_calls
                .as_ref()
                .is_some_and(|calls| calls.iter().any(|call| call.function_name == "shell_run"))
        })
        .expect("resumed follow-up context should include prior assistant tool call");
    let calls = tool_call_message
        .assistant_tool_calls
        .as_ref()
        .expect("assistant tool calls");
    assert_eq!(calls[0].function_name, "shell_run");
    assert_eq!(
        calls[0].arguments_json,
        r#"{"command":"touch docs/rust-language.md"}"#
    );
    let reconstructed_tool_call_id = calls[0].tool_call_id.as_str();

    let tool_result_message = followup_messages
        .iter()
        .find(|message| {
            message.role == MessageRole::Tool
                && message.tool_call_id.as_deref() == Some(reconstructed_tool_call_id)
        })
        .expect("resumed follow-up context should include prior tool result");
    assert_eq!(tool_result_message.name.as_deref(), Some("shell_run"));
    assert!(tool_result_message
        .content
        .contains("touch docs/rust-language.md"));
}

#[tokio::test]
async fn provider_stream_metadata_persists_to_jsonl_events() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![vec![
        ProviderStreamEvent::Started {
            metadata: Some(ProviderStreamStartMetadata {
                provider_session_id: Some("session-observed-1".to_string()),
                provider_cache_id: Some("cache-observed-1".to_string()),
            }),
        },
        ProviderStreamEvent::ReasoningDelta("provider reasoning summary".to_string()),
        ProviderStreamEvent::TextDelta("metadata visible".to_string()),
        ProviderStreamEvent::DoneWithMetadata {
            usage: CompletionUsage {
                prompt_tokens: 12,
                completion_tokens: 4,
                total_tokens: 16,
            },
            metadata: Some(ProviderStreamFinishedMetadata {
                provider_response_id: Some("resp-observed-1".to_string()),
                provider_session_id: Some("session-observed-1".to_string()),
                provider_cache_id: Some("cache-observed-1".to_string()),
                provider_stop_reason: Some("stop".to_string()),
                cache_read_tokens: Some(7),
                cache_write_tokens: Some(3),
                assistant_message_id: Some("msg-observed-1".to_string()),
                thinking: Some(ProviderStreamThinkingMetadata {
                    summary: Some("provider supplied thinking summary".to_string()),
                    summary_digest: Some("thinking-digest-provider".to_string()),
                    signature: Some("thinking-signature-provider".to_string()),
                }),
            }),
        },
    ]]);
    let coordinator = test_agent_coordinator_with_provider(temp_dir.path(), Arc::new(provider), 1);

    let run = coordinator
        .start_run(
            "coord_provider_metadata_jsonl",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let turn_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "inspect provider metadata")
        .await
        .expect("request agent turn");

    wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(turn_request_id.as_str())
                        && data.result_summary == "metadata visible"
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let started = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ProviderRequestStarted(data)
                if event.correlation_id.as_deref() == Some(turn_request_id.as_str()) =>
            {
                Some(data)
            }
            _ => None,
        })
        .expect("provider request started event");
    let started_metadata = started.metadata.as_ref().expect("started metadata");
    assert_eq!(
        started_metadata.turn_id.as_deref(),
        Some(turn_request_id.as_str())
    );
    assert_eq!(
        started_metadata.provider_call_id.as_deref(),
        Some(started.request_id.as_str())
    );
    assert_eq!(
        started_metadata.provider_session_id.as_deref(),
        Some("session-observed-1")
    );
    assert_eq!(
        started_metadata.provider_cache_id.as_deref(),
        Some("cache-observed-1")
    );

    let finished = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ProviderRequestFinished(data)
                if event.correlation_id.as_deref() == Some(turn_request_id.as_str()) =>
            {
                Some(data)
            }
            _ => None,
        })
        .expect("provider request finished event");
    let finished_metadata = finished.metadata.as_ref().expect("finished metadata");
    assert_eq!(
        finished_metadata.turn_id.as_deref(),
        Some(turn_request_id.as_str())
    );
    assert_eq!(
        finished_metadata.provider_call_id.as_deref(),
        Some(started.request_id.as_str())
    );
    assert_eq!(
        finished_metadata.provider_response_id.as_deref(),
        Some("resp-observed-1")
    );
    assert_eq!(
        finished_metadata.provider_session_id.as_deref(),
        Some("session-observed-1")
    );
    assert_eq!(
        finished_metadata.provider_cache_id.as_deref(),
        Some("cache-observed-1")
    );
    assert_eq!(
        finished_metadata.provider_stop_reason.as_deref(),
        Some("stop")
    );
    assert_eq!(finished_metadata.cache_read_tokens, Some(7));
    assert_eq!(finished_metadata.cache_write_tokens, Some(3));
    let assistant_message = finished_metadata
        .assistant_message
        .as_ref()
        .expect("assistant message metadata");
    assert_eq!(
        assistant_message.message_id.as_deref(),
        Some("msg-observed-1")
    );
    assert!(assistant_message.text_digest.is_some());
    assert!(assistant_message.reasoning_digest.is_some());
    let thinking = finished_metadata
        .thinking
        .as_ref()
        .expect("thinking metadata");
    assert_eq!(
        thinking.summary.as_deref(),
        Some("provider supplied thinking summary")
    );
    assert_eq!(
        thinking.summary_digest.as_deref(),
        Some("thinking-digest-provider")
    );
    assert_eq!(
        thinking.signature.as_deref(),
        Some("thinking-signature-provider")
    );
}

#[tokio::test]
async fn provider_reasoning_metadata_persists_digest_without_raw_summary_fallback() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::ReasoningDelta("private reasoning text".to_string()),
        ProviderStreamEvent::TextDelta("visible answer".to_string()),
        ProviderStreamEvent::Done {
            usage: CompletionUsage {
                prompt_tokens: 2,
                completion_tokens: 2,
                total_tokens: 4,
            },
        },
    ]]);
    let coordinator = test_agent_coordinator_with_provider(temp_dir.path(), Arc::new(provider), 1);

    let run = coordinator
        .start_run(
            "coord_provider_reasoning_digest_only",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let turn_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "inspect reasoning metadata")
        .await
        .expect("request agent turn");

    wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(turn_request_id.as_str())
                        && data.result_summary == "visible answer"
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let thinking = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ProviderRequestFinished(data)
                if event.correlation_id.as_deref() == Some(turn_request_id.as_str()) =>
            {
                data.metadata.as_ref()?.thinking.as_ref()
            }
            _ => None,
        })
        .expect("thinking metadata");

    assert_eq!(thinking.summary, None);
    assert!(thinking.summary_digest.is_some());
    assert_eq!(thinking.signature, None);
}

#[tokio::test]
async fn no_tool_turn_appends_explicit_phase_barriers_in_order() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_agent_coordinator_with_provider(
        temp_dir.path(),
        Arc::new(CapturingProvider::new(vec!["phase complete"])),
        1,
    );

    let run = coordinator
        .start_run(
            "coord_no_tool_explicit_phase_order",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "phase order prompt")
        .await
        .expect("request agent turn");

    wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.result_summary == "phase complete"
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let scheduled_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::TaskScheduled(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.state == TaskScheduleState::Started
                        && data
                            .queue_key
                            .as_deref()
                            .is_some_and(|queue_key| queue_key.starts_with("provider_model:"))
            )
        })
        .expect("agent turn scheduled barrier");
    let provider_started_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ProviderRequestStarted(_)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
            )
        })
        .expect("provider start barrier");
    let provider_delta_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ProviderStreamDelta(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.delta == "phase complete"
            )
        })
        .expect("provider text delta");
    let provider_finished_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ProviderRequestFinished(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.finish_reason == "done"
            )
        })
        .expect("provider finish barrier");
    let assistant_message_finished_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::AssistantMessageFinished(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.tool_call_count == 0
            )
        })
        .expect("assistant message end barrier");
    let turn_completed_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.result_summary == "phase complete"
            )
        })
        .expect("turn end barrier");

    assert!(scheduled_idx < provider_started_idx);
    assert!(provider_started_idx < provider_delta_idx);
    assert!(provider_delta_idx < provider_finished_idx);
    assert!(provider_finished_idx < assistant_message_finished_idx);
    assert!(assistant_message_finished_idx < turn_completed_idx);
    assert!(!events.iter().any(|event| {
        event.correlation_id.as_deref() == Some(request_id.as_str())
            && matches!(event.payload, EventV1::ToolCallStarted(_))
    }));
}

#[tokio::test]
async fn tool_turn_does_not_preflight_until_assistant_message_end_is_durable() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("calling tool".to_string()),
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "phase_tool".to_string(),
                function_name: "shell_run".to_string(),
                arguments_json: "{}".to_string(),
            },
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 2,
                    completion_tokens: 1,
                    total_tokens: 3,
                },
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("tool phase done".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 3,
                    completion_tokens: 2,
                    total_tokens: 5,
                },
            },
        ],
    ]);
    let coordinator = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(provider),
        test_tool_registry(),
        PermissionPolicy::new(
            PermissionMode::Deny,
            PermissionMode::Allow,
            PermissionMode::Deny,
        ),
        vec!["shell.run".to_string()],
        12,
    );

    let run = coordinator
        .start_run(
            "coord_tool_waits_for_assistant_barrier",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "tool phase barrier")
        .await
        .expect("request agent turn");

    wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.result_summary == "tool phase done"
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let provider_started = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::ProviderRequestStarted(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str()) =>
            {
                Some(data.request_id.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(provider_started.len(), 2, "tool turn should continue once");

    let first_provider_finished_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ProviderRequestFinished(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.request_id == provider_started[0]
            )
        })
        .expect("provider finish barrier for first provider call");
    let assistant_message_finished_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::AssistantMessageFinished(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.request_id == provider_started[0]
                        && data.tool_call_count == 1
            )
        })
        .expect("assistant message end barrier for first provider call");
    let tool_requested_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallRequested(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.tool_id == "shell.run"
            )
        })
        .expect("tool preflight requested");
    let tool_started_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallStarted(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && !data.tool_call_id.is_empty()
            )
        })
        .expect("tool started");
    let tool_finished_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallFinished(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.status == ToolCallStatus::Succeeded
            )
        })
        .expect("tool result barrier");
    let second_provider_started_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ProviderRequestStarted(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.request_id == provider_started[1]
            )
        })
        .expect("follow-up provider start");

    assert!(first_provider_finished_idx < assistant_message_finished_idx);
    assert!(assistant_message_finished_idx < tool_requested_idx);
    assert!(assistant_message_finished_idx < tool_started_idx);
    assert!(tool_finished_idx < second_provider_started_idx);
}

#[tokio::test]
async fn queued_turn_recomputes_context_at_provider_start() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = DelayedCapturingProvider::new(
        vec!["first answer", "beta answer", "second answer"],
        Duration::from_millis(50),
    );
    let coordinator =
        test_agent_coordinator_with_provider(temp_dir.path(), Arc::new(provider.clone()), 1);

    let run = coordinator
        .start_run(
            "coord_queued_turn_recomputes_context",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let beta_agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "beta", None)
        .await
        .expect("spawn idle beta");

    let first_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .expect("first turn");
    wait_for_events(&run.events_path, Duration::from_secs(1), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(first_request_id.as_str())
                        && data.result_summary == "first answer"
            )
        })
    })
    .await;

    let beta_request_id = coordinator
        .request_agent_turn(supervisor_actor(), beta_agent_id, "beta question")
        .await
        .expect("beta turn holding provider slot");
    wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::ProviderRequestStarted(_)
                    if event.correlation_id.as_deref() == Some(beta_request_id.as_str())
            )
        })
    })
    .await;

    let second_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "second question")
        .await
        .expect("queued second turn");

    wait_for_events(&run.events_path, Duration::from_secs(1), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(second_request_id.as_str())
                        && data.result_summary == "second answer"
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    let requests = provider.requests();
    assert_eq!(requests.len(), 3, "expected all provider turns to run");
    let second_shape = requests[2]
        .messages
        .iter()
        .map(|message| (message.role.clone(), message.content.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        second_shape,
        vec![
            (MessageRole::System, "alpha-prompt".to_string()),
            (MessageRole::User, "first question".to_string()),
            (MessageRole::Assistant, "first answer".to_string()),
            (MessageRole::User, "second question".to_string()),
        ],
        "queued turn should use provider context recomputed after the earlier turn completed"
    );

    let events = load_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(second_request_id.as_str())
                    && data.state == TaskScheduleState::Queued
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCompleted(data)
                if event.correlation_id.as_deref() == Some(first_request_id.as_str())
                    && data.result_summary == "first answer"
        )
    }));
}

#[tokio::test]
async fn tool_results_project_in_assistant_source_order_after_out_of_order_completion() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let slow_started = Arc::new(Notify::new());
    let slow_release = Arc::new(Notify::new());
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "slow_call".to_string(),
                function_name: "shell_slow".to_string(),
                arguments_json: "{}".to_string(),
            },
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "fast_call".to_string(),
                function_name: "shell_fast".to_string(),
                arguments_json: "{}".to_string(),
            },
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 2,
                    completion_tokens: 1,
                    total_tokens: 3,
                },
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("ordered final".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 4,
                    completion_tokens: 2,
                    total_tokens: 6,
                },
            },
        ],
    ]);
    let coordinator = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(provider.clone()),
        named_tool_registry(vec![
            NamedShellTool {
                id: "shell.slow",
                output: "slow output",
                started: Some(slow_started.clone()),
                release: Some(slow_release.clone()),
            },
            NamedShellTool {
                id: "shell.fast",
                output: "fast output",
                started: None,
                release: None,
            },
        ]),
        PermissionPolicy::new(
            PermissionMode::Deny,
            PermissionMode::Allow,
            PermissionMode::Deny,
        ),
        vec!["shell.slow".to_string(), "shell.fast".to_string()],
        12,
    );

    let run = coordinator
        .start_run(
            "coord_source_order_after_out_of_order_tools",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "call slow then fast")
        .await
        .expect("request agent turn");

    tokio::time::timeout(Duration::from_millis(500), slow_started.notified())
        .await
        .expect("slow tool should start");
    let fast_completed_before_slow_release =
        tokio::time::timeout(Duration::from_millis(150), async {
            loop {
                let events = load_events(&run.events_path);
                if events.iter().any(|event| {
                    matches!(
                        &event.payload,
                        EventV1::TaskCompleted(data) if data.result_summary == "fast output"
                    )
                }) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .is_ok();

    slow_release.notify_waiters();
    wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data) if data.result_summary == "ordered final"
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    assert!(
        fast_completed_before_slow_release,
        "fast tool should be allowed to complete before the earlier slow tool is released"
    );

    let events = load_events(&run.events_path);
    let chronological_tool_finishes = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::ToolCallFinished(data)
                if matches!(
                    data.output_summary.as_deref(),
                    Some("fast output" | "slow output")
                ) =>
            {
                data.output_summary.clone()
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        chronological_tool_finishes,
        vec!["fast output".to_string(), "slow output".to_string()],
        "JSONL lifecycle events should remain chronological by tool completion order"
    );

    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        2,
        "tool turn should continue once with tool outputs"
    );
    let tool_messages = requests[1]
        .messages
        .iter()
        .filter(|message| message.role == MessageRole::Tool)
        .map(|message| (message.tool_call_id.clone(), message.content.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        tool_messages,
        vec![
            (Some("slow_call".to_string()), "slow output".to_string()),
            (Some("fast_call".to_string()), "fast output".to_string()),
        ],
        "provider context must preserve model source order, not tool completion order"
    );
}

#[tokio::test]
async fn duplicate_provider_tool_call_ids_fail_before_tool_start() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::ToolCallComplete {
            tool_call_id: "dup_call".to_string(),
            function_name: "shell_run".to_string(),
            arguments_json: "{}".to_string(),
        },
        ProviderStreamEvent::ToolCallComplete {
            tool_call_id: "dup_call".to_string(),
            function_name: "shell_run".to_string(),
            arguments_json: "{}".to_string(),
        },
        ProviderStreamEvent::Done {
            usage: CompletionUsage {
                prompt_tokens: 2,
                completion_tokens: 1,
                total_tokens: 3,
            },
        },
    ]]);
    let coordinator = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(provider),
        test_tool_registry(),
        PermissionPolicy::new(
            PermissionMode::Deny,
            PermissionMode::Allow,
            PermissionMode::Deny,
        ),
        vec!["shell.run".to_string()],
        12,
    );

    let run = coordinator
        .start_run(
            "coord_duplicate_provider_tool_call_ids",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "duplicate tools")
        .await
        .expect("request agent turn");

    let events = wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            event.correlation_id.as_deref() == Some(request_id.as_str())
                && matches!(
                    &event.payload,
                    EventV1::TaskCancelled(_) | EventV1::TaskCompleted(_)
                )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCancelled(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str())
                    && data.reason.contains("duplicate")
                    && data.reason.contains("tool_call_id")
        )
    }));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.payload, EventV1::ToolCallStarted(_))),
        "duplicate provider tool ids must be rejected before any tool starts"
    );
}

#[tokio::test]
async fn empty_provider_tool_call_id_fails_before_tool_start() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::ToolCallComplete {
            tool_call_id: String::new(),
            function_name: "shell_run".to_string(),
            arguments_json: "{}".to_string(),
        },
        ProviderStreamEvent::Done {
            usage: CompletionUsage {
                prompt_tokens: 2,
                completion_tokens: 1,
                total_tokens: 3,
            },
        },
    ]]);
    let coordinator = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(provider),
        test_tool_registry(),
        PermissionPolicy::new(
            PermissionMode::Deny,
            PermissionMode::Allow,
            PermissionMode::Deny,
        ),
        vec!["shell.run".to_string()],
        12,
    );

    let run = coordinator
        .start_run(
            "coord_empty_provider_tool_call_id",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "empty tool id")
        .await
        .expect("request agent turn");

    let events = wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            event.correlation_id.as_deref() == Some(request_id.as_str())
                && matches!(
                    &event.payload,
                    EventV1::TaskCancelled(_) | EventV1::TaskCompleted(_)
                )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCancelled(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str())
                    && data.reason.contains("invalid")
                    && data.reason.contains("tool_call_id")
        )
    }));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.payload, EventV1::ToolCallStarted(_))),
        "empty provider tool ids must be rejected before any tool starts"
    );
}

#[tokio::test]
async fn denied_or_pending_tool_never_starts_before_permission_resolution() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(test_mock_provider()),
        test_tool_registry(),
        PermissionPolicy::new(
            PermissionMode::Deny,
            PermissionMode::Deny,
            PermissionMode::Deny,
        ),
        vec!["shell.run".to_string()],
        12,
    );

    let run = coordinator
        .start_run(
            "coord_denied_tool_no_start",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let error = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "true"}),
        )
        .await
        .expect_err("denied request should fail");
    let CoordinatorError::PermissionDenied(tool_call_id) = error else {
        panic!("expected permission denial");
    };
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallFinished(data)
                if data.tool_call_id == tool_call_id && data.status == ToolCallStatus::Failed
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id == tool_call_id
        )
    }));

    let pending_temp_dir = tempfile::tempdir().expect("pending tempdir");
    let pending_coordinator = test_agent_tool_coordinator(
        pending_temp_dir.path(),
        Arc::new(test_mock_provider()),
        test_tool_registry(),
        PermissionPolicy::new(
            PermissionMode::Deny,
            PermissionMode::Ask,
            PermissionMode::Deny,
        )
        .with_ask_timeout_ms(5_000),
        vec!["shell.run".to_string()],
        12,
    );

    let pending_run = pending_coordinator
        .start_run(
            "coord_ask_pending_tool_no_start",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start pending run");
    let pending_tool_call_id = pending_coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "true"}),
        )
        .await
        .expect("ask request should be pending");

    let pending_events = wait_for_events(
        &pending_run.events_path,
        Duration::from_millis(500),
        |events| {
            events.iter().any(|event| {
                matches!(
                    &event.payload,
                    EventV1::PermissionRequested(data)
                        if data.tool_call_id.as_deref() == Some(pending_tool_call_id.as_str())
                )
            })
        },
    )
    .await;
    assert!(!pending_events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id == pending_tool_call_id
        )
    }));

    let pending_permission_id = pending_events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_deref() == Some(pending_tool_call_id.as_str()) =>
            {
                Some(data.permission_id.clone())
            }
            _ => None,
        })
        .expect("pending permission id");
    pending_coordinator
        .resolve_permission(
            pending_permission_id,
            RuntimePermissionDecision::Deny,
            Some("test cleanup".to_string()),
        )
        .await
        .expect("resolve pending permission");
    pending_coordinator
        .stop_run()
        .await
        .expect("stop pending run");
}

#[tokio::test]
async fn ask_pending_tool_call_never_emits_started_before_approval() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(test_mock_provider()),
        test_tool_registry(),
        PermissionPolicy::new(
            PermissionMode::Deny,
            PermissionMode::Ask,
            PermissionMode::Deny,
        )
        .with_ask_timeout_ms(5_000),
        vec!["shell.run".to_string()],
        12,
    );

    let run = coordinator
        .start_run(
            "coord_ask_pending_tool_no_start",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let tool_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "true"}),
        )
        .await
        .expect("ask request should be pending");

    let events = wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::PermissionRequested(data)
                    if data.tool_call_id.as_deref() == Some(tool_call_id.as_str())
            )
        })
    })
    .await;
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id == tool_call_id
        )
    }));

    let permission_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_deref() == Some(tool_call_id.as_str()) =>
            {
                Some(data.permission_id.clone())
            }
            _ => None,
        })
        .expect("permission id");
    coordinator
        .resolve_permission(
            permission_id,
            RuntimePermissionDecision::Deny,
            Some("test cleanup".to_string()),
        )
        .await
        .expect("resolve pending permission");
    coordinator.stop_run().await.expect("stop run");
}

#[tokio::test]
async fn cancelling_turn_waiting_for_permission_emits_turn_end_without_tool_start() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::ToolCallComplete {
            tool_call_id: "needs_permission".to_string(),
            function_name: "shell_run".to_string(),
            arguments_json: "{}".to_string(),
        },
        ProviderStreamEvent::Done {
            usage: CompletionUsage {
                prompt_tokens: 2,
                completion_tokens: 1,
                total_tokens: 3,
            },
        },
    ]]);
    let coordinator = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(provider),
        test_tool_registry(),
        PermissionPolicy::new(
            PermissionMode::Deny,
            PermissionMode::Ask,
            PermissionMode::Deny,
        )
        .with_ask_timeout_ms(5_000),
        vec!["shell.run".to_string()],
        12,
    );

    let run = coordinator
        .start_run(
            "coord_cancel_turn_waiting_permission",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "permission gated tool")
        .await
        .expect("request agent turn");

    let events = wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events
            .iter()
            .any(|event| matches!(event.payload, EventV1::PermissionRequested(_)))
    })
    .await;
    let agent_task_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str())
                    && data
                        .queue_key
                        .as_deref()
                        .is_some_and(|queue_key| queue_key.starts_with("provider_model:")) =>
            {
                Some(data.task_id.clone())
            }
            _ => None,
        })
        .expect("agent task id");
    let (permission_id, provider_tool_call_id) = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data) => Some((
                data.permission_id.clone(),
                data.tool_call_id.clone().expect("tool permission call id"),
            )),
            _ => None,
        })
        .expect("pending permission");

    coordinator
        .cancel_task(agent_task_id.clone(), "cancel while permission pending")
        .await
        .expect("cancel waiting turn");
    coordinator
        .resolve_permission(
            permission_id,
            RuntimePermissionDecision::Allow,
            Some("late approval".to_string()),
        )
        .await
        .expect("late resolve should be accepted without starting tool");
    tokio::time::sleep(Duration::from_millis(150)).await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCancelled(data)
                if data.task_id == agent_task_id
                    && data.reason == "cancel while permission pending"
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id == provider_tool_call_id
        )
    }));
}

#[tokio::test]
async fn late_tool_result_after_turn_cancellation_is_task_result_late() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let release = Arc::new(Notify::new());
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "blocking_tool".to_string(),
                function_name: "shell_block".to_string(),
                arguments_json: "{}".to_string(),
            },
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 2,
                    completion_tokens: 1,
                    total_tokens: 3,
                },
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta(
                "should not be requested after cancellation".to_string(),
            ),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 3,
                    completion_tokens: 2,
                    total_tokens: 5,
                },
            },
        ],
    ]);
    let coordinator = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(provider),
        lifecycle_tool_registry(release.clone()),
        PermissionPolicy::new(
            PermissionMode::Deny,
            PermissionMode::Allow,
            PermissionMode::Deny,
        ),
        vec!["shell.block".to_string()],
        12,
    );

    let run = coordinator
        .start_run(
            "coord_late_tool_after_turn_cancel",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "run blocking tool")
        .await
        .expect("request agent turn");

    let events = wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskScheduled(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.queue_key.as_deref() == Some("tool:shell.block")
            )
        })
    })
    .await;
    let agent_task_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str())
                    && data
                        .queue_key
                        .as_deref()
                        .is_some_and(|queue_key| queue_key.starts_with("provider_model:")) =>
            {
                Some(data.task_id.clone())
            }
            _ => None,
        })
        .expect("agent task id");
    let tool_task_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str())
                    && data.queue_key.as_deref() == Some("tool:shell.block") =>
            {
                Some(data.task_id.clone())
            }
            _ => None,
        })
        .expect("tool task id");

    coordinator
        .cancel_task(agent_task_id.clone(), "cancel turn during tool execution")
        .await
        .expect("cancel agent turn");
    let events = wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskResultLate(data) if data.task_id == tool_task_id
            )
        })
    })
    .await;
    release.notify_waiters();
    coordinator.stop_run().await.expect("stop run");

    let turn_terminal_events = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data) if data.task_id == agent_task_id
            ) || matches!(
                &event.payload,
                EventV1::TaskCompleted(data) if data.task_id == agent_task_id
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        turn_terminal_events.len(),
        1,
        "cancelled turn should have exactly one terminal event"
    );
    assert!(matches!(
        &turn_terminal_events[0].payload,
        EventV1::TaskCancelled(data)
            if data.reason == "cancel turn during tool execution"
    ));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskResultLate(data) if data.task_id == tool_task_id
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCompleted(data) if data.task_id == tool_task_id
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ProviderStreamDelta(data)
                if data.delta == "should not be requested after cancellation"
        )
    }));
}

#[tokio::test]
async fn cancelled_tool_task_records_late_result_without_completion() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let release = Arc::new(Notify::new());
    let clock = Arc::new(FakeClock::new());
    let coordinator = test_tool_lifecycle_coordinator(
        temp_dir.path(),
        clock,
        lifecycle_tool_registry(release.clone()),
        Duration::from_millis(50),
        15_000,
        5,
        1,
    );

    let run = coordinator
        .start_run(
            "coord_cancel_tool_late_result",
            temp_dir.path().to_path_buf(),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle agent");
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "alpha-prompt")
        .await
        .expect("request agent turn");
    let owner_actor = EventActor::new(ActorKind::Worker, Some(agent_id));
    tokio::time::sleep(Duration::from_millis(20)).await;
    coordinator
        .request_tool_call(
            owner_actor.clone(),
            Some("deep".to_string()),
            "shell.block",
            json!({"cmd": "wait"}),
        )
        .await
        .expect("request blocking tool");

    let events = wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskScheduled(data)
                    if data.queue_key.as_deref() == Some("tool:shell.block")
            )
        })
    })
    .await;
    let task_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskScheduled(data)
                if data.queue_key.as_deref() == Some("tool:shell.block") =>
            {
                Some(data.task_id.clone())
            }
            _ => None,
        })
        .expect("tool task id");
    coordinator
        .cancel_task(task_id.clone(), "manual cancellation")
        .await
        .expect("cancel tool task");
    release.notify_waiters();
    wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskResultLate(data) if data.task_id == task_id
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let late = events
        .iter()
        .find(|event| {
            matches!(
                &event.payload,
                EventV1::TaskResultLate(data) if data.task_id == task_id
            )
        })
        .expect("late result");
    assert_task_event_context(late, &owner_actor, &request_id);
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCompleted(data) if data.task_id == task_id
        )
    }));
}

#[tokio::test]
async fn provider_partial_output_then_error_is_not_successful_assistant_message() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::TextDelta("partial answer".to_string()),
        ProviderStreamEvent::Error {
            message: "provider exploded".to_string(),
        },
    ]]);
    let coordinator = test_agent_coordinator_with_provider(temp_dir.path(), Arc::new(provider), 1);

    let run = coordinator
        .start_run(
            "coord_provider_partial_output_error",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "partial then error")
        .await
        .expect("request agent turn");

    let events = wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.reason == "provider exploded"
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    let delta_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ProviderStreamDelta(data)
                    if data.delta == "partial answer"
                        && event.correlation_id.as_deref() == Some(request_id.as_str())
            )
        })
        .expect("partial delta event");
    let finished_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ProviderRequestFinished(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.finish_reason == "error"
                        && data.output_digest.is_none()
            )
        })
        .expect("provider error finish event");
    assert!(delta_idx < finished_idx);
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCompleted(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str())
                    && data.result_summary.contains("partial answer")
        )
    }));
}

#[tokio::test]
async fn records_provider_error_events_and_fails_agent_turn() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::TextDelta("partial answer".to_string()),
        ProviderStreamEvent::Error {
            message: "provider exploded".to_string(),
        },
    ]]);
    let coordinator = test_agent_coordinator_with_provider(temp_dir.path(), Arc::new(provider), 1);

    let run = coordinator
        .start_run(
            "coord_provider_error_fails_agent_turn",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "partial then error")
        .await
        .expect("request agent turn");

    let events = wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.reason == "provider exploded"
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ProviderStreamDelta(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str())
                    && data.delta == "partial answer"
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ProviderRequestFinished(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str())
                    && data.finish_reason == "error"
                    && data.output_digest.is_none()
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCompleted(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str())
                    && data.result_summary.contains("partial answer")
        )
    }));
}

#[tokio::test]
async fn failed_turn_context_preserves_provider_error_partial_output() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("partial answer".to_string()),
            ProviderStreamEvent::Error {
                message: "provider exploded".to_string(),
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("follow-up answer".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 4,
                    completion_tokens: 2,
                    total_tokens: 6,
                },
            },
        ],
    ]);
    let coordinator =
        test_agent_coordinator_with_provider(temp_dir.path(), Arc::new(provider.clone()), 1);

    let run = coordinator
        .start_run(
            "coord_failed_context_provider_error",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let failed_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "partial then error")
        .await
        .expect("request failing turn");
    wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data)
                    if event.correlation_id.as_deref() == Some(failed_request_id.as_str())
                        && data.reason == "provider exploded"
            )
        })
    })
    .await;

    let follow_up_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "continue after failure")
        .await
        .expect("request follow-up turn");
    wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(follow_up_request_id.as_str())
                        && data.result_summary == "follow-up answer"
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    let requests = provider.requests();
    let follow_up = requests.last().expect("follow-up provider request");
    let assistant_marker = follow_up
        .messages
        .iter()
        .find(|message| {
            message.role == MessageRole::Assistant
                && message
                    .content
                    .contains("Harness preserved an incomplete provider turn")
        })
        .expect("failed turn marker should be sent before follow-up prompt");
    assert!(assistant_marker.content.contains("Status: failed"));
    assert!(assistant_marker.content.contains("Stage: provider_error"));
    assert!(assistant_marker
        .content
        .contains("Reason: provider exploded"));
    assert!(assistant_marker.content.contains("partial answer"));
    assert!(follow_up.messages.iter().any(|message| {
        message.role == MessageRole::User && message.content == "partial then error"
    }));
    assert!(follow_up.messages.iter().any(|message| {
        message.role == MessageRole::User && message.content == "continue after failure"
    }));
}

#[tokio::test]
async fn failed_turn_context_preserves_cancelled_turn_marker() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let tool_started = Arc::new(Notify::new());
    let tool_release = Arc::new(Notify::new());
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("partial before cancellation".to_string()),
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "blocking_tool".to_string(),
                function_name: "shell_block".to_string(),
                arguments_json: "{}".to_string(),
            },
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 2,
                    completion_tokens: 1,
                    total_tokens: 3,
                },
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("after cancellation".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 4,
                    completion_tokens: 2,
                    total_tokens: 6,
                },
            },
        ],
    ]);
    let coordinator = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(provider.clone()),
        named_tool_registry(vec![NamedShellTool {
            id: "shell.block",
            output: "blocking output",
            started: Some(tool_started.clone()),
            release: Some(tool_release.clone()),
        }]),
        PermissionPolicy::new(
            PermissionMode::Deny,
            PermissionMode::Allow,
            PermissionMode::Deny,
        ),
        vec!["shell.block".to_string()],
        12,
    );

    let run = coordinator
        .start_run(
            "coord_failed_context_cancelled_marker",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let cancelled_request_id = coordinator
        .request_agent_turn(
            supervisor_actor(),
            agent_id.clone(),
            "cancel after assistant",
        )
        .await
        .expect("request cancellable turn");

    tokio::time::timeout(Duration::from_millis(500), tool_started.notified())
        .await
        .expect("blocking tool should start");
    let events = load_events(&run.events_path);
    let task_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(cancelled_request_id.as_str())
                    && data
                        .queue_key
                        .as_deref()
                        .is_some_and(|queue_key| queue_key.starts_with("provider_model:")) =>
            {
                Some(data.task_id.clone())
            }
            _ => None,
        })
        .expect("agent task id");
    coordinator
        .cancel_task(task_id.clone(), "operator cancelled")
        .await
        .expect("cancel running agent turn");
    tool_release.notify_waiters();
    wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data)
                    if data.task_id == task_id && data.reason == "operator cancelled"
            )
        })
    })
    .await;
    tokio::time::sleep(Duration::from_millis(120)).await;

    let follow_up_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "continue after cancellation")
        .await
        .expect("request follow-up turn");
    wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(follow_up_request_id.as_str())
                        && data.result_summary == "after cancellation"
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    let requests = provider.requests();
    let follow_up = requests.last().expect("follow-up provider request");
    let assistant_marker = follow_up
        .messages
        .iter()
        .find(|message| {
            message.role == MessageRole::Assistant
                && message
                    .content
                    .contains("Harness preserved an incomplete provider turn")
        })
        .expect("cancelled turn marker should be sent before follow-up prompt");
    assert!(assistant_marker.content.contains("Status: aborted"));
    assert!(assistant_marker.content.contains("Stage: cancelled"));
    assert!(assistant_marker
        .content
        .contains("Reason: operator cancelled"));
    assert!(assistant_marker
        .content
        .contains("partial before cancellation"));
}

#[tokio::test]
async fn failed_turn_context_preserves_tool_failure_without_orphan_tool_call() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("plain text before tool failure".to_string()),
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "failing_tool".to_string(),
                function_name: "shell_fail".to_string(),
                arguments_json: "{}".to_string(),
            },
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 2,
                    completion_tokens: 1,
                    total_tokens: 3,
                },
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("after tool failure".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 4,
                    completion_tokens: 2,
                    total_tokens: 6,
                },
            },
        ],
    ]);
    let coordinator = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(provider.clone()),
        Arc::new({
            let mut registry = ToolRegistry::new();
            registry.register(Arc::new(FailingShellTool));
            registry
        }),
        PermissionPolicy::new(
            PermissionMode::Deny,
            PermissionMode::Allow,
            PermissionMode::Deny,
        ),
        vec!["shell.fail".to_string()],
        12,
    );

    let run = coordinator
        .start_run(
            "coord_failed_context_tool_failure",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let failed_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "call failing tool")
        .await
        .expect("request failing tool turn");
    wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data)
                    if event.correlation_id.as_deref() == Some(failed_request_id.as_str())
                        && data.reason.contains("tool call `shell_fail` failed closed")
            )
        })
    })
    .await;

    let follow_up_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "continue after tool failure")
        .await
        .expect("request follow-up turn");
    wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(follow_up_request_id.as_str())
                        && data.result_summary == "after tool failure"
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    let requests = provider.requests();
    let follow_up = requests.last().expect("follow-up provider request");
    let assistant_marker = follow_up
        .messages
        .iter()
        .find(|message| {
            message.role == MessageRole::Assistant
                && message
                    .content
                    .contains("Harness preserved an incomplete provider turn")
        })
        .expect("tool-failure marker should be sent before follow-up prompt");
    assert!(assistant_marker.content.contains("Status: failed"));
    assert!(assistant_marker.content.contains("Stage: tool_failure"));
    assert!(assistant_marker
        .content
        .contains("plain text before tool failure"));
    assert!(!assistant_marker.content.contains("failing_tool"));
    assert!(assistant_marker.assistant_tool_calls.is_none());
    assert!(!follow_up
        .messages
        .iter()
        .any(|message| message.role == MessageRole::Tool));
}

#[tokio::test]
async fn failed_response_compaction_writes_checkpoint_after_provider_error() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta(format!(
                "partial provider output {}",
                "B".repeat(12_000)
            )),
            ProviderStreamEvent::Error {
                message: "provider exploded".to_string(),
            },
        ],
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider),
        1,
        CompactionRuntimeConfig {
            fallback_input_tokens: 2_000,
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_failed_response_compaction_provider_error",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .expect("first turn");
    wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data) if data.result_summary == "A".repeat(12_000)
            )
        })
    })
    .await;

    let failed_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "partial then error")
        .await
        .expect("failing turn");
    let events = wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::CompactionWritten(data)
                    if data.trigger_reason == "failed_response"
                        && data.through_request_id.as_deref() == Some(failed_request_id.as_str())
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    let cancelled_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data)
                    if event.correlation_id.as_deref() == Some(failed_request_id.as_str())
                        && data.reason == "provider exploded"
            )
        })
        .expect("original provider failure cancellation");
    let requested_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::CompactionRequested(data)
                    if data.trigger_reason == "failed_response"
                        && data.through_request_id.as_deref() == Some(failed_request_id.as_str())
            )
        })
        .expect("failed-response compaction requested");
    assert!(
        cancelled_idx < requested_idx,
        "terminal TaskCancelled must be durable before failed-response compaction starts"
    );

    let checkpoint = checkpoint_for_trigger(&run, &events, "failed_response");
    let failed_turn = checkpoint
        .recent_turns
        .iter()
        .find(|turn| !turn.status.is_completed())
        .expect("failed provider turn remains provider-visible");
    assert_eq!(
        failed_turn.status,
        harness_core::agent::ProviderConversationTurnStatus::Failed
    );
    assert_eq!(failed_turn.failure_stage.as_deref(), Some("provider_error"));
    assert_eq!(
        failed_turn.failure_reason.as_deref(),
        Some("provider exploded")
    );
    assert!(failed_turn
        .assistant_response
        .contains("partial provider output"));
}

#[tokio::test]
async fn aborted_response_compaction_preserves_abort_marker() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let tool_started = Arc::new(Notify::new());
    let tool_release = Arc::new(Notify::new());
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta(format!(
                "partial before cancellation {}",
                "C".repeat(12_000)
            )),
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "blocking_tool".to_string(),
                function_name: "shell_block".to_string(),
                arguments_json: "{}".to_string(),
            },
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 2,
                    completion_tokens: 1,
                    total_tokens: 3,
                },
            },
        ],
        provider_text_events("after cancellation"),
    ]);
    let coordinator = test_agent_tool_coordinator_with_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        named_tool_registry(vec![NamedShellTool {
            id: "shell.block",
            output: "blocking output",
            started: Some(tool_started.clone()),
            release: Some(tool_release.clone()),
        }]),
        PermissionPolicy::new(
            PermissionMode::Deny,
            PermissionMode::Allow,
            PermissionMode::Deny,
        ),
        vec!["shell.block".to_string()],
        12,
        CompactionRuntimeConfig {
            fallback_input_tokens: 2_000,
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_aborted_response_compaction_marker",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .expect("first turn");
    wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data) if data.result_summary == "A".repeat(12_000)
            )
        })
    })
    .await;

    let cancelled_request_id = coordinator
        .request_agent_turn(
            supervisor_actor(),
            agent_id.clone(),
            "cancel after assistant",
        )
        .await
        .expect("cancellable turn");
    tokio::time::timeout(Duration::from_millis(700), tool_started.notified())
        .await
        .expect("blocking tool should start");
    let task_id = load_events(&run.events_path)
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(cancelled_request_id.as_str())
                    && data
                        .queue_key
                        .as_deref()
                        .is_some_and(|queue_key| queue_key.starts_with("provider_model:")) =>
            {
                Some(data.task_id.clone())
            }
            _ => None,
        })
        .expect("agent task id");
    coordinator
        .cancel_task(task_id.clone(), "operator cancelled")
        .await
        .expect("cancel running agent turn");
    tool_release.notify_waiters();

    let events = wait_for_events(&run.events_path, Duration::from_millis(900), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::CompactionWritten(data)
                    if data.trigger_reason == "aborted_response"
                        && data.through_request_id.as_deref() == Some(cancelled_request_id.as_str())
            )
        })
    })
    .await;

    let follow_up_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "continue after cancellation")
        .await
        .expect("request follow-up turn");
    wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(follow_up_request_id.as_str())
                        && data.result_summary == "after cancellation"
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCancelled(data)
                if data.task_id == task_id && data.reason == "operator cancelled"
        )
    }));
    let checkpoint = checkpoint_for_trigger(&run, &events, "aborted_response");
    let aborted_turn = checkpoint
        .recent_turns
        .iter()
        .find(|turn| !turn.status.is_completed())
        .expect("aborted turn remains provider-visible");
    assert_eq!(
        aborted_turn.status,
        harness_core::agent::ProviderConversationTurnStatus::Aborted
    );
    assert_eq!(aborted_turn.failure_stage.as_deref(), Some("cancelled"));
    assert_eq!(
        aborted_turn.failure_reason.as_deref(),
        Some("operator cancelled")
    );
    assert!(aborted_turn
        .assistant_response
        .contains("partial before cancellation"));

    let requests = provider.requests();
    let follow_up = requests.last().expect("follow-up request");
    let marker = follow_up
        .messages
        .iter()
        .find(|message| {
            message.role == MessageRole::Assistant
                && message
                    .content
                    .contains("Harness preserved an incomplete provider turn")
        })
        .expect("aborted marker should remain in provider-visible context");
    assert!(marker.content.contains("Status: aborted"));
    assert!(marker.content.contains("Stage: cancelled"));
}

#[tokio::test]
async fn failed_response_compaction_failure_does_not_mask_original_error() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta(format!(
                "partial provider output {}",
                "B".repeat(12_000)
            )),
            ProviderStreamEvent::Error {
                message: "provider exploded".to_string(),
            },
        ],
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider),
        1,
        CompactionRuntimeConfig {
            fallback_input_tokens: 2_000,
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_failed_response_compaction_artifact_failure",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .expect("first turn");
    wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data) if data.result_summary == "A".repeat(12_000)
            )
        })
    })
    .await;
    fs::remove_dir_all(&run.artifacts_dir).expect("remove artifacts dir");
    fs::write(&run.artifacts_dir, "not a directory").expect("replace artifacts dir with file");

    let failed_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "partial then error")
        .await
        .expect("failing turn");
    let events = wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::CompactionFailed(data)
                    if data.trigger_reason == "failed_response"
                        && data.through_request_id.as_deref() == Some(failed_request_id.as_str())
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCancelled(data)
                if event.correlation_id.as_deref() == Some(failed_request_id.as_str())
                    && data.reason == "provider exploded"
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::CompactionWritten(data) if data.trigger_reason == "failed_response"
        )
    }));
}

#[tokio::test]
async fn critical_compaction_requested_hook_failure_records_compaction_failed() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta(format!(
                "partial provider output {}",
                "B".repeat(12_000)
            )),
            ProviderStreamEvent::Error {
                message: "provider exploded".to_string(),
            },
        ],
    ]);
    let hook_runtime_config = HookRuntimeConfig {
        hooks: HooksConfig {
            lifecycle: vec![LifecycleHookConfig {
                id: Some("failed-terminal-compaction-blocker".to_string()),
                event: HookLifecycleEvent::CompactionRequested,
                command: vec![
                    "bash".to_string(),
                    "-lc".to_string(),
                    "if [ \"${HARNESS_HOOK_OUTCOME:-}\" = failed_response ]; then printf 'blocked failed terminal compaction'; exit 23; fi; printf ok".to_string(),
                ],
                cwd: Some(".".to_string()),
                timeout_ms: 4_000,
                critical: true,
                env: BTreeMap::new(),
            }],
        },
        shell_allowlist: ShellAllowlist {
            executables: vec!["bash".to_string()],
            cwd_roots: vec![".".to_string()],
        },
        suppress_execution: false,
    };
    let coordinator = test_agent_coordinator_with_provider_compaction_and_hooks(
        temp_dir.path(),
        Arc::new(provider),
        1,
        CompactionRuntimeConfig {
            fallback_input_tokens: 2_000,
            ..CompactionRuntimeConfig::default()
        },
        hook_runtime_config,
    );

    let run = coordinator
        .start_run(
            "coord_failed_response_compaction_hook_failure",
            temp_dir.path().to_path_buf(),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .expect("first turn");
    wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data) if data.result_summary == "A".repeat(12_000)
            )
        })
    })
    .await;

    let failed_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "partial then error")
        .await
        .expect("failing turn");
    let events = wait_for_events(&run.events_path, Duration::from_millis(900), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::CompactionFailed(data)
                    if data.trigger_reason == "failed_response"
                        && data.through_request_id.as_deref() == Some(failed_request_id.as_str())
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCancelled(data)
                if event.correlation_id.as_deref() == Some(failed_request_id.as_str())
                    && data.reason == "provider exploded"
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::CompactionWritten(data) if data.trigger_reason == "failed_response"
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::CompactionWritten(data) if data.trigger_reason == "failed_response"
        )
    }));
}

#[tokio::test]
async fn profile_max_iters_does_not_cap_tool_loops() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "loop_call_1".to_string(),
                function_name: "shell_run".to_string(),
                arguments_json: "{}".to_string(),
            },
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 2,
                    completion_tokens: 1,
                    total_tokens: 3,
                },
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "loop_call_2".to_string(),
                function_name: "shell_run".to_string(),
                arguments_json: "{}".to_string(),
            },
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 2,
                    completion_tokens: 1,
                    total_tokens: 3,
                },
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("completed after former cap".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 12,
                    completion_tokens: 3,
                    total_tokens: 15,
                },
            },
        ],
    ]);
    let provider_handle = provider.clone();
    let coordinator = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(provider),
        test_tool_registry(),
        PermissionPolicy::new(
            PermissionMode::Deny,
            PermissionMode::Allow,
            PermissionMode::Deny,
        ),
        vec!["shell.run".to_string()],
        2,
    );

    let run = coordinator
        .start_run(
            "coord_profile_max_iters_not_enforced",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "loop past former cap")
        .await
        .expect("request agent turn");

    let events = wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.result_summary == "completed after former cap"
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCancelled(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str())
                    && data.reason.contains("max_iters")
        )
    }));

    let started_tools = events
        .iter()
        .filter(|event| matches!(event.payload, EventV1::ToolCallStarted(_)))
        .count();
    assert_eq!(
        started_tools, 2,
        "max_iters=2 should not stop the third provider phase after two tool loops"
    );

    let requests = provider_handle.requests();
    assert_eq!(requests.len(), 3, "expected all provider phases to run");
    let final_messages = &requests[2].messages;
    assert!(final_messages.iter().any(|message| {
        message.role == MessageRole::User && message.content == "loop past former cap"
    }));
    assert!(final_messages
        .iter()
        .any(|message| { message.role == MessageRole::Tool && message.content.contains("ok {}") }));
}

#[tokio::test]
async fn resume_existing_run_restores_sequence_and_ids() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_resume_ids";
    write_resume_fixture(
        temp_dir.path(),
        run_id,
        &[
            resume_fixture_event(
                run_id,
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            resume_fixture_event(
                run_id,
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000003".to_string(),
                    profile: "alpha".to_string(),
                    parent_agent_id: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000008".to_string(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "existing prompt".to_string(),
                    request_digest: "digest-existing".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                4,
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000005".to_string(),
                    tool_id: "shell.run".to_string(),
                    args_summary: "{\"cmd\":\"true\"}".to_string(),
                    args_digest: "digest-tool".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                5,
                EventV1::PermissionRequested(PermissionRequestedEvent {
                    permission_id: "perm_000004".to_string(),
                    kind: "shell".to_string(),
                    tool_call_id: Some("toolcall_000005".to_string()),
                    summary: "allow shell".to_string(),
                    request_digest: "digest-perm".to_string(),
                    timeout_ms: 1_000,
                    default_decision: EventPermissionDecision::Deny,
                }),
            ),
            resume_fixture_event(
                run_id,
                6,
                EventV1::PermissionResolved(PermissionResolvedEvent {
                    permission_id: "perm_000004".to_string(),
                    decision: EventPermissionDecision::Allow,
                    reason: Some("approved".to_string()),
                }),
            ),
            resume_fixture_event(
                run_id,
                7,
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000009".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("tool:shell.run".to_string()),
                }),
            ),
            resume_fixture_event(
                run_id,
                8,
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000009".to_string(),
                    result_summary: "done".to_string(),
                    result_digest: "digest-task".to_string(),
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

    let coordinator = test_resume_coordinator(temp_dir.path());
    let run = coordinator
        .resume_run(run_id, "interactive")
        .await
        .expect("resume run");

    let post_resume_events = load_events(&run.events_path);
    assert!(post_resume_events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::RunStarted(data)
                if event.seq == 10
                    && data.run_name == "interactive"
                    && data.workspace_root == "/workspace/project"
        )
    }));
    assert!(post_resume_events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::AgentSpawned(data)
                if event.seq == 11
                    && data.agent_id == "agent_000003"
                    && data.profile == "alpha"
        )
    }));

    let new_agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn resumed agent");
    assert_eq!(new_agent_id, "agent_000004");

    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), "agent_000003", "resume prompt")
        .await
        .expect("request resumed agent turn");
    assert_eq!(request_id, "req_000009");

    let tool_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "true"}),
        )
        .await
        .expect("request resumed tool call");
    assert_eq!(tool_call_id, "toolcall_000006");

    coordinator
        .resolve_permission("perm_000005", RuntimePermissionDecision::Allow, None)
        .await
        .expect("resolve resumed permission");

    tokio::time::sleep(Duration::from_millis(120)).await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::PermissionRequested(data)
                if data.permission_id == "perm_000005"
                    && data.tool_call_id.as_deref() == Some("toolcall_000006")
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskScheduled(data) if data.task_id == "task_000010"
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskScheduled(data) if data.task_id == "task_000011"
        )
    }));
}

#[tokio::test]
async fn resume_existing_run_reuses_same_run_id_and_directory() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_resume_same_dir";
    write_resume_fixture(
        temp_dir.path(),
        run_id,
        &[
            resume_fixture_event(
                run_id,
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            resume_fixture_event(
                run_id,
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "alpha".to_string(),
                    parent_agent_id: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "existing prompt".to_string(),
                    request_digest: "digest-existing".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                4,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "segment complete".to_string(),
                }),
            ),
        ],
    );

    let coordinator = test_resume_coordinator(temp_dir.path());
    let run = coordinator
        .resume_run(run_id, "interactive")
        .await
        .expect("resume run");

    assert_eq!(run.run_id, run_id);
    assert_eq!(run.run_dir, temp_dir.path().join(run_id));
    assert_eq!(
        run.events_path,
        temp_dir.path().join(run_id).join("events.jsonl")
    );

    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    assert_eq!(
        events.len(),
        7,
        "resume should append start+bindings+finish"
    );
    assert_eq!(events.last().map(|event| event.seq), Some(7));
}

#[tokio::test]
async fn resume_existing_run_restores_subagent_parent_lineage_for_hooks_and_replay() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_resume_subagent_parent_lineage";
    let workspace_root = temp_dir.path().display().to_string();
    let hook_output_path = temp_dir.path().join("resume-subagent-parent-hooks.txt");
    let hook_command = "printf '%s|agent=%s|parent=%s|request=%s\\n' \"$HARNESS_HOOK_EVENT\" \"${HARNESS_HOOK_AGENT_ID:-}\" \"${HARNESS_HOOK_PARENT_AGENT_ID:-}\" \"${HARNESS_HOOK_REQUEST_ID:-}\" >> \"$HOOK_OUTPUT_PATH\"";
    let hook_env = BTreeMap::from([(
        "HOOK_OUTPUT_PATH".to_string(),
        hook_output_path.display().to_string(),
    )]);
    let hook_runtime_config = HookRuntimeConfig {
        hooks: HooksConfig {
            lifecycle: vec![LifecycleHookConfig {
                id: Some("provider-started".to_string()),
                event: HookLifecycleEvent::ProviderRequestStarted,
                command: vec![
                    "bash".to_string(),
                    "-lc".to_string(),
                    hook_command.to_string(),
                ],
                cwd: None,
                timeout_ms: 4_000,
                critical: false,
                env: hook_env,
            }],
        },
        shell_allowlist: ShellAllowlist {
            executables: vec!["bash".to_string()],
            cwd_roots: vec![".".to_string()],
        },
        suppress_execution: false,
    };

    write_resume_fixture(
        temp_dir.path(),
        run_id,
        &[
            resume_fixture_event(
                run_id,
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: workspace_root.clone(),
                }),
            ),
            resume_fixture_event(
                run_id,
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "alpha".to_string(),
                    parent_agent_id: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                3,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000002".to_string(),
                    profile: "beta".to_string(),
                    parent_agent_id: Some("agent_000001".to_string()),
                }),
            ),
            resume_fixture_event(
                run_id,
                4,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "existing prompt".to_string(),
                    request_digest: "digest-existing".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                5,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "segment complete".to_string(),
                }),
            ),
        ],
    );

    let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.provider = Arc::new(CapturingProvider::new(vec!["resumed child answer"]));
    config.agent_profiles = agent_profiles();
    config.hook_runtime_config = hook_runtime_config;

    let clock = Arc::new(FakeClock::new());
    let redactor = Arc::new(DefaultRedactor::default());
    let coordinator = spawn_coordinator(config, clock, redactor);

    let run = coordinator
        .resume_run(run_id, "interactive")
        .await
        .expect("resume run");

    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), "agent_000002", "resume child prompt")
        .await
        .expect("request resumed child turn");
    assert_eq!(request_id, "req_000002");

    tokio::time::sleep(Duration::from_millis(120)).await;
    coordinator.stop_run().await.expect("stop resumed run");

    let hook_output = fs::read_to_string(&hook_output_path).expect("hook output");
    assert!(hook_output.lines().any(|line| {
        line.starts_with("provider_request_started|")
            && line.contains("agent=agent_000002")
            && line.contains("parent=agent_000001")
            && line.contains("request=req_000002")
    }));

    let plan = inspect_resume_plan(&run.run_dir);
    let child = plan
        .child_sessions
        .get("agent_000002")
        .expect("projected resumed child session");
    assert_eq!(child.parent_session_id.as_deref(), Some("agent_000001"));
}

#[tokio::test]
async fn resume_existing_run_restores_agent_profile_bindings() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_resume_agents";
    write_resume_fixture(
        temp_dir.path(),
        run_id,
        &[
            resume_fixture_event(
                run_id,
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            resume_fixture_event(
                run_id,
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "alpha".to_string(),
                    parent_agent_id: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "existing prompt".to_string(),
                    request_digest: "digest-existing".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                4,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "segment complete".to_string(),
                }),
            ),
        ],
    );

    let coordinator = test_resume_coordinator(temp_dir.path());
    coordinator
        .resume_run(run_id, "interactive")
        .await
        .expect("resume run");

    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), "agent_000001", "continue")
        .await
        .expect("known resumed agent should accept turn requests");
    assert_eq!(request_id, "req_000002");

    coordinator.stop_run().await.expect("stop run");
}

#[tokio::test]
async fn resume_existing_run_persists_bindings_for_future_reresume() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_resume_reresume";
    write_resume_fixture(
        temp_dir.path(),
        run_id,
        &[
            resume_fixture_event(
                run_id,
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            resume_fixture_event(
                run_id,
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "alpha".to_string(),
                    parent_agent_id: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "existing prompt".to_string(),
                    request_digest: "digest-existing".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                4,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "segment complete".to_string(),
                }),
            ),
        ],
    );

    let first = test_resume_coordinator(temp_dir.path());
    let run = first
        .resume_run(run_id, "interactive")
        .await
        .expect("first resume should succeed");
    first
        .request_agent_turn(supervisor_actor(), "agent_000001", "follow up")
        .await
        .expect("restored agent should accept turn in resumed segment");
    tokio::time::sleep(Duration::from_millis(120)).await;
    first.stop_run().await.expect("stop first resumed segment");

    let plan_after_first_resume = inspect_resume_plan(&run.run_dir);
    assert_eq!(
        plan_after_first_resume.latest_lifecycle_status,
        LifecycleSegmentStatus::Finished
    );
    assert_eq!(
        plan_after_first_resume
            .known_agents
            .get("agent_000001")
            .map(String::as_str),
        Some("alpha")
    );
    assert!(
        plan_after_first_resume.is_resumable,
        "resumed segment should remain resumable after stop"
    );

    let second = test_resume_coordinator(temp_dir.path());
    second
        .resume_run(run_id, "interactive")
        .await
        .expect("second resume should succeed from persisted bindings");
    let second_request_id = second
        .request_agent_turn(supervisor_actor(), "agent_000001", "second resume turn")
        .await
        .expect("restored agent should be present after second resume");
    assert_eq!(second_request_id, "req_000004");
    second
        .stop_run()
        .await
        .expect("stop second resumed segment");
}

#[tokio::test]
async fn resume_existing_run_remains_resumable_after_open_and_quit_without_prompt() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_resume_open_quit";
    write_resume_fixture(
        temp_dir.path(),
        run_id,
        &[
            resume_fixture_event(
                run_id,
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            resume_fixture_event(
                run_id,
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "alpha".to_string(),
                    parent_agent_id: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "existing prompt".to_string(),
                    request_digest: "digest-existing".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                4,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "segment complete".to_string(),
                }),
            ),
        ],
    );

    let first = test_resume_coordinator(temp_dir.path());
    let run = first
        .resume_run(run_id, "interactive")
        .await
        .expect("first resume should succeed");
    first
        .stop_run()
        .await
        .expect("stop resumed segment without new prompt");

    let plan_after_quit = inspect_resume_plan(&run.run_dir);
    assert_eq!(
        plan_after_quit.latest_lifecycle_status,
        LifecycleSegmentStatus::Finished
    );
    assert!(
        plan_after_quit.is_resumable,
        "open-and-quit resumed segments should remain resumable"
    );
    assert_eq!(
        plan_after_quit.provider_model.as_deref(),
        Some("mock/model-1")
    );

    let second = test_resume_coordinator(temp_dir.path());
    second
        .resume_run(run_id, "interactive")
        .await
        .expect("second resume should succeed after open-and-quit");
    let request_id = second
        .request_agent_turn(supervisor_actor(), "agent_000001", "second segment prompt")
        .await
        .expect("resumed agent should accept prompt after re-resume");
    assert_eq!(request_id, "req_000002");
    second
        .stop_run()
        .await
        .expect("stop second resumed segment");
}

#[tokio::test]
async fn resume_existing_run_rejects_missing_historical_profile_binding() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_resume_missing_profile";
    let events_path = write_resume_fixture(
        temp_dir.path(),
        run_id,
        &[
            resume_fixture_event(
                run_id,
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            resume_fixture_event(
                run_id,
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "gamma".to_string(),
                    parent_agent_id: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "existing prompt".to_string(),
                    request_digest: "digest-existing".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                4,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "segment complete".to_string(),
                }),
            ),
        ],
    );

    let before = load_events(&events_path);
    let coordinator = test_resume_coordinator(temp_dir.path());
    let error = coordinator
        .resume_run(run_id, "interactive")
        .await
        .expect_err("missing profile binding should fail closed");

    let CoordinatorError::ResumeRestoreFailed {
        run_id: restored_run_id,
        reason,
    } = error
    else {
        panic!("expected resume restore failure");
    };
    assert_eq!(restored_run_id, run_id);
    assert!(
        reason
            .contains("historical agent `agent_000001` references missing profile binding `gamma`"),
        "unexpected restore failure reason: {reason}"
    );

    let after = load_events(&events_path);
    assert_eq!(after.len(), before.len(), "resume failure must not append");
    assert_eq!(after.last().map(|event| event.seq), Some(4));
}

#[tokio::test]
async fn resume_existing_run_rejects_second_writer_lock() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let first = test_resume_coordinator(temp_dir.path());
    let run = first
        .start_run("interactive", PathBuf::from("/workspace/project"))
        .await
        .expect("start first run");

    let second = test_resume_coordinator(temp_dir.path());
    let error = second
        .resume_run(&run.run_id, "interactive")
        .await
        .expect_err("second writer must fail lock acquisition");

    assert!(matches!(
        error,
        CoordinatorError::EventStore(EventStoreError::AcquireWriterLock { .. })
    ));

    first.stop_run().await.expect("stop first run");
}

#[tokio::test]
async fn resume_existing_run_does_not_append_on_restore_failure() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_resume_invalid_agent";
    let events_path = write_resume_fixture(
        temp_dir.path(),
        run_id,
        &[
            resume_fixture_event(
                run_id,
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            resume_fixture_event(
                run_id,
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_invalid".to_string(),
                    profile: "alpha".to_string(),
                    parent_agent_id: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "existing prompt".to_string(),
                    request_digest: "digest-existing".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                4,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "segment complete".to_string(),
                }),
            ),
        ],
    );

    let before = load_events(&events_path);
    let coordinator = test_resume_coordinator(temp_dir.path());
    let error = coordinator
        .resume_run(run_id, "interactive")
        .await
        .expect_err("invalid restore metadata should fail closed");

    assert!(matches!(
        error,
        CoordinatorError::ResumeRestoreFailed { .. }
    ));

    let after = load_events(&events_path);
    assert_eq!(after.len(), before.len(), "resume failure must not append");
    assert_eq!(after.last().map(|event| event.seq), Some(4));
}

#[tokio::test]
async fn resume_restores_interactive_provider_context() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_resume_context";
    write_resumable_history_fixture(temp_dir.path(), run_id);

    let provider = CapturingProvider::new(vec!["second answer"]);
    let coordinator =
        test_resume_coordinator_with_provider(temp_dir.path(), Arc::new(provider.clone()));

    coordinator
        .resume_run(run_id, "interactive")
        .await
        .expect("resume run");
    coordinator
        .request_agent_turn(supervisor_actor(), "agent_000001", "second question")
        .await
        .expect("submit resumed prompt");
    tokio::time::sleep(Duration::from_millis(120)).await;
    coordinator.stop_run().await.expect("stop resumed run");

    let requests = provider.requests();
    assert_eq!(requests.len(), 1, "expected one resumed provider request");

    let shape = requests[0]
        .messages
        .iter()
        .map(|message| (message.role.clone(), message.content.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        shape,
        vec![
            (MessageRole::System, "alpha-prompt".to_string()),
            (MessageRole::User, "first question".to_string()),
            (MessageRole::Assistant, "first answer".to_string()),
            (MessageRole::User, "second question".to_string()),
        ]
    );
}

#[tokio::test]
async fn resumed_turn_matches_uninterrupted_conversation_request_shape() {
    let uninterrupted_dir = tempfile::tempdir().expect("tempdir");
    let uninterrupted_provider = CapturingProvider::new(vec!["first answer", "second answer"]);
    let uninterrupted = test_resume_coordinator_with_provider(
        uninterrupted_dir.path(),
        Arc::new(uninterrupted_provider.clone()),
    );

    uninterrupted
        .start_run("interactive", PathBuf::from("/workspace/project"))
        .await
        .expect("start uninterrupted run");
    uninterrupted
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn uninterrupted agent");
    uninterrupted
        .request_agent_turn(supervisor_actor(), "agent_000001", "first question")
        .await
        .expect("first uninterrupted turn");
    tokio::time::sleep(Duration::from_millis(80)).await;
    uninterrupted
        .request_agent_turn(supervisor_actor(), "agent_000001", "second question")
        .await
        .expect("second uninterrupted turn");
    tokio::time::sleep(Duration::from_millis(120)).await;
    uninterrupted
        .stop_run()
        .await
        .expect("stop uninterrupted run");

    let uninterrupted_shape = uninterrupted_provider
        .requests()
        .last()
        .expect("second uninterrupted request")
        .messages
        .iter()
        .map(|message| (message.role.clone(), message.content.clone()))
        .collect::<Vec<_>>();

    let resumed_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_resume_matches_uninterrupted";
    write_resumable_history_fixture(resumed_dir.path(), run_id);
    let resumed_provider = CapturingProvider::new(vec!["second answer"]);
    let resumed = test_resume_coordinator_with_provider(
        resumed_dir.path(),
        Arc::new(resumed_provider.clone()),
    );

    resumed
        .resume_run(run_id, "interactive")
        .await
        .expect("resume run");
    resumed
        .request_agent_turn(supervisor_actor(), "agent_000001", "second question")
        .await
        .expect("resumed second turn");
    tokio::time::sleep(Duration::from_millis(120)).await;
    resumed.stop_run().await.expect("stop resumed run");

    let resumed_shape = resumed_provider
        .requests()
        .last()
        .expect("resumed request")
        .messages
        .iter()
        .map(|message| (message.role.clone(), message.content.clone()))
        .collect::<Vec<_>>();

    assert_eq!(
        resumed_shape, uninterrupted_shape,
        "resumed turns should use the same provider request shape as uninterrupted conversations"
    );
}

#[tokio::test]
async fn resume_restores_multi_turn_historical_context_with_final_task_output() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_resume_multi_turn_context";
    write_resumable_multi_turn_history_fixture(temp_dir.path(), run_id);

    let provider = CapturingProvider::new(vec!["second answer"]);
    let coordinator =
        test_resume_coordinator_with_provider(temp_dir.path(), Arc::new(provider.clone()));

    coordinator
        .resume_run(run_id, "interactive")
        .await
        .expect("resume run");
    coordinator
        .request_agent_turn(supervisor_actor(), "agent_000001", "second question")
        .await
        .expect("submit resumed prompt");
    tokio::time::sleep(Duration::from_millis(120)).await;
    coordinator.stop_run().await.expect("stop resumed run");

    let requests = provider.requests();
    assert_eq!(requests.len(), 1, "expected one resumed provider request");

    let shape = requests[0]
        .messages
        .iter()
        .map(|message| (message.role.clone(), message.content.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        shape,
        vec![
            (MessageRole::System, "alpha-prompt".to_string()),
            (MessageRole::User, "first question".to_string()),
            (MessageRole::Assistant, "first final answer".to_string()),
            (MessageRole::User, "second question".to_string()),
        ]
    );
}

#[tokio::test]
async fn overflow_retry_compacts_context_and_retries_with_summary() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("A".repeat(12_000)),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 100,
                    completion_tokens: 100,
                    total_tokens: 200,
                },
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("B".repeat(12_000)),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 100,
                    completion_tokens: 100,
                    total_tokens: 200,
                },
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::Error {
                message: "prompt token count of 128713 exceeds the limit of 128000".to_string(),
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("recovered answer".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 64,
                    completion_tokens: 8,
                    total_tokens: 72,
                },
            },
        ],
    ]);
    let coordinator =
        test_agent_coordinator_with_provider(temp_dir.path(), Arc::new(provider.clone()), 1);

    let run = coordinator
        .start_run(
            "coord_overflow_retry_compaction",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");

    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .expect("first turn");
    tokio::time::sleep(Duration::from_millis(80)).await;
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "second question")
        .await
        .expect("second turn");
    tokio::time::sleep(Duration::from_millis(80)).await;
    let third_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "third question")
        .await
        .expect("third turn with overflow retry");
    tokio::time::sleep(Duration::from_millis(180)).await;
    coordinator.stop_run().await.expect("stop run");

    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        4,
        "third turn should retry once after compaction"
    );
    let retried_messages = requests
        .last()
        .expect("retried provider request")
        .messages
        .iter()
        .map(|message| (message.role.clone(), message.content.clone()))
        .collect::<Vec<_>>();
    assert!(retried_messages.iter().any(|(role, content)| {
        *role == MessageRole::Assistant
            && content.contains("Checkpoint recap generated by the harness for older turns")
    }));
    assert!(retried_messages
        .iter()
        .any(|(role, content)| { *role == MessageRole::User && content == "third question" }));

    let events = load_events(&run.events_path);
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, EventV1::CompactionApplied(_))));
    let provider_finishes = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventV1::ProviderRequestFinished(_)
                    if event.correlation_id.as_deref() == Some(third_request_id.as_str())
            )
        })
        .count();
    assert_eq!(
        provider_finishes, 2,
        "overflow retry should emit error then success finishes"
    );
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCompleted(payload)
                if event.correlation_id.as_deref() == Some(third_request_id.as_str())
                    && payload.result_summary == "recovered answer"
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCompleted(payload)
                if payload.result_summary == "A".repeat(12_000)
        )
    }));
}

#[tokio::test]
async fn overflow_retry_can_compact_a_single_large_preserved_turn() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("A".repeat(12_000)),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 100,
                    completion_tokens: 100,
                    total_tokens: 200,
                },
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::Error {
                message: "prompt token count of 128713 exceeds the limit of 128000".to_string(),
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("recovered answer".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 64,
                    completion_tokens: 8,
                    total_tokens: 72,
                },
            },
        ],
    ]);
    let coordinator =
        test_agent_coordinator_with_provider(temp_dir.path(), Arc::new(provider.clone()), 1);

    let run = coordinator
        .start_run(
            "coord_overflow_retry_single_large_turn",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");

    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .expect("first turn");
    tokio::time::sleep(Duration::from_millis(80)).await;
    let second_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "second question")
        .await
        .expect("second turn with overflow retry");
    tokio::time::sleep(Duration::from_millis(180)).await;
    coordinator.stop_run().await.expect("stop run");

    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        3,
        "single preserved turn should still retry once after summary-only compaction"
    );
    let retried_messages = requests
        .last()
        .expect("retried provider request")
        .messages
        .iter()
        .map(|message| (message.role.clone(), message.content.clone()))
        .collect::<Vec<_>>();
    assert!(retried_messages.iter().any(|(role, content)| {
        *role == MessageRole::Assistant
            && content.contains("Checkpoint recap generated by the harness for older turns")
    }));
    assert!(!retried_messages
        .iter()
        .any(|(role, content)| { *role == MessageRole::User && content == "first question" }));
    assert!(retried_messages
        .iter()
        .any(|(role, content)| { *role == MessageRole::User && content == "second question" }));

    let events = load_events(&run.events_path);
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, EventV1::CompactionApplied(_))));
    let provider_finishes = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventV1::ProviderRequestFinished(_)
                    if event.correlation_id.as_deref() == Some(second_request_id.as_str())
            )
        })
        .count();
    assert_eq!(
        provider_finishes, 2,
        "overflow retry should emit error then success finishes"
    );
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCompleted(payload)
                if event.correlation_id.as_deref() == Some(second_request_id.as_str())
                    && payload.result_summary == "recovered answer"
        )
    }));
}

#[tokio::test]
async fn overflow_retry_does_not_resend_same_context_when_compaction_is_noop() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("first answer".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 32,
                    completion_tokens: 8,
                    total_tokens: 40,
                },
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::Error {
                message: "prompt token count of 128713 exceeds the limit of 128000".to_string(),
            },
        ],
    ]);
    let coordinator =
        test_agent_coordinator_with_provider(temp_dir.path(), Arc::new(provider.clone()), 1);

    let run = coordinator
        .start_run(
            "coord_overflow_retry_noop_compaction",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");

    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .expect("first turn");
    tokio::time::sleep(Duration::from_millis(80)).await;
    let second_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "second question")
        .await
        .expect("second turn with overflow");
    tokio::time::sleep(Duration::from_millis(120)).await;
    coordinator.stop_run().await.expect("stop run");

    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        2,
        "overflow retry should not resend when compaction cannot shrink context"
    );

    let events = load_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::CompactionFailed(payload)
                if payload.agent_id == "agent_000001"
                    && payload.trigger_reason == "overflow_retry"
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCancelled(payload)
                if event.correlation_id.as_deref() == Some(second_request_id.as_str())
                    && payload.reason.contains("no checkpoint reduced the active provider context")
        )
    }));
}

#[tokio::test]
async fn compaction_trigger_pre_prompt_occurs_before_provider_request_started() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let current_prompt = "C".repeat(12_000);
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        provider_text_events(&"B".repeat(12_000)),
        provider_text_events("third answer"),
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        1,
        CompactionRuntimeConfig {
            fallback_input_tokens: 10_000,
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_pre_prompt_compaction_order",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .expect("first turn");
    tokio::time::sleep(Duration::from_millis(80)).await;
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "second question")
        .await
        .expect("second turn");
    tokio::time::sleep(Duration::from_millis(80)).await;
    let third_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, &current_prompt)
        .await
        .expect("third turn");
    tokio::time::sleep(Duration::from_millis(180)).await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let pre_prompt_written_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::CompactionWritten(payload) if payload.trigger_reason == "pre_prompt"
            )
        })
        .expect("pre-prompt compaction written event");
    let provider_started_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ProviderRequestStarted(_) if event.correlation_id.as_deref() == Some(third_request_id.as_str())
            )
        })
        .expect("third provider request started event");
    assert!(
        pre_prompt_written_idx < provider_started_idx,
        "pre-prompt checkpoint must be written before the third provider request is constructed"
    );
}

#[tokio::test]
async fn compaction_trigger_pre_prompt_attempts_once_per_turn() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let current_prompt = "C".repeat(12_000);
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        provider_text_events(&"B".repeat(12_000)),
        provider_text_events("third answer"),
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        1,
        CompactionRuntimeConfig {
            fallback_input_tokens: 10_000,
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_pre_prompt_compaction_attempts_once",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    for question in ["first question", "second question"] {
        coordinator
            .request_agent_turn(supervisor_actor(), agent_id.clone(), question)
            .await
            .expect("turn request");
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id, &current_prompt)
        .await
        .expect("third turn");
    tokio::time::sleep(Duration::from_millis(180)).await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let pre_prompt_writes = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventV1::CompactionWritten(payload) if payload.trigger_reason == "pre_prompt"
            )
        })
        .count();
    assert_eq!(
        pre_prompt_writes, 1,
        "pre-prompt compaction should write at most one checkpoint for a turn"
    );
    assert_eq!(
        provider.requests().len(),
        3,
        "provider execution should continue once with the uncompacted context"
    );
}

#[tokio::test]
async fn compaction_trigger_pre_prompt_runtime_uses_checkpointed_prior_context() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let current_prompt = "C".repeat(12_000);
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        provider_text_events(&"B".repeat(12_000)),
        provider_text_events("third answer"),
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        1,
        CompactionRuntimeConfig {
            fallback_input_tokens: 10_000,
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_pre_prompt_compaction_prior_context",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    for question in ["first question", "second question"] {
        coordinator
            .request_agent_turn(supervisor_actor(), agent_id.clone(), question)
            .await
            .expect("turn request");
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id, &current_prompt)
        .await
        .expect("third turn");
    tokio::time::sleep(Duration::from_millis(180)).await;
    coordinator.stop_run().await.expect("stop run");

    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        3,
        "third turn should not need an overflow retry"
    );
    let third_messages = requests
        .last()
        .expect("third provider request")
        .messages
        .iter()
        .map(|message| (message.role.clone(), message.content.clone()))
        .collect::<Vec<_>>();
    assert!(third_messages.iter().any(|(role, content)| {
        *role == MessageRole::Assistant
            && content.contains("Checkpoint recap generated by the harness for older turns")
    }));
    assert!(third_messages
        .iter()
        .any(|(role, content)| { *role == MessageRole::User && content == &current_prompt }));
    assert!(!third_messages
        .iter()
        .any(|(role, content)| *role == MessageRole::User && content == "first question"));

    let events = load_events(&run.events_path);
    let written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) if payload.trigger_reason == "pre_prompt" => {
                Some(payload.clone())
            }
            _ => None,
        })
        .expect("pre-prompt compaction written event");
    assert_eq!(written.tokens_before, None);
    let checkpoint_path = run.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(&checkpoint_path).expect("read checkpoint artifact");
    let checkpoint_json: serde_json::Value =
        serde_json::from_str(&checkpoint_body).expect("parse checkpoint artifact json");
    let recent_turns = checkpoint_json
        .get("recent_turns")
        .and_then(serde_json::Value::as_array)
        .expect("checkpoint recent turns");
    assert!(recent_turns.iter().all(|turn| {
        turn.get("user_prompt").and_then(serde_json::Value::as_str) != Some(current_prompt.as_str())
    }));
}

#[tokio::test]
async fn compaction_no_loop_guards_cover_pre_prompt_overflow_and_failed_response() {
    let pre_prompt_dir = tempfile::tempdir().expect("pre-prompt tempdir");
    let pre_prompt_current_prompt = "C".repeat(12_000);
    let pre_prompt_provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        provider_text_events(&"B".repeat(12_000)),
        provider_text_events("third answer after pre-prompt no-shrink"),
    ]);
    let pre_prompt = test_agent_coordinator_with_provider_and_compaction(
        pre_prompt_dir.path(),
        Arc::new(pre_prompt_provider.clone()),
        1,
        CompactionRuntimeConfig {
            fallback_input_tokens: 10_000,
            ..CompactionRuntimeConfig::default()
        },
    );
    let pre_prompt_run = pre_prompt
        .start_run(
            "coord_no_loop_pre_prompt",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start pre-prompt run");
    let pre_prompt_agent = pre_prompt
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn pre-prompt agent");
    for question in ["first question", "second question"] {
        pre_prompt
            .request_agent_turn(supervisor_actor(), pre_prompt_agent.clone(), question)
            .await
            .expect("pre-prompt setup turn");
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
    let pre_prompt_request_id = pre_prompt
        .request_agent_turn(
            supervisor_actor(),
            pre_prompt_agent,
            &pre_prompt_current_prompt,
        )
        .await
        .expect("pre-prompt no-shrink turn");
    let pre_prompt_events = wait_for_events(
        &pre_prompt_run.events_path,
        Duration::from_millis(900),
        |events| {
            events.iter().any(|event| {
                matches!(
                    &event.payload,
                    EventV1::TaskCompleted(payload)
                        if event.correlation_id.as_deref() == Some(pre_prompt_request_id.as_str())
                            && payload.result_summary == "third answer after pre-prompt no-shrink"
                )
            })
        },
    )
    .await;
    pre_prompt.stop_run().await.expect("stop pre-prompt run");
    let pre_prompt_attempt_count = pre_prompt_events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventV1::CompactionWritten(payload) if payload.trigger_reason == "pre_prompt"
            ) || matches!(
                &event.payload,
                EventV1::CompactionFailed(payload) if payload.trigger_reason == "pre_prompt"
            )
        })
        .count();
    assert_eq!(
        pre_prompt_attempt_count,
        1,
        "pre-prompt compaction should attempt at most once before provider execution; requested={}, written={}",
        pre_prompt_events
            .iter()
            .filter(|event| matches!(
                &event.payload,
                EventV1::CompactionRequested(payload) if payload.trigger_reason == "pre_prompt"
            ))
            .count(),
        pre_prompt_events
            .iter()
            .filter(|event| matches!(
                &event.payload,
                EventV1::CompactionWritten(payload) if payload.trigger_reason == "pre_prompt"
            ))
            .count()
    );
    assert_eq!(
        pre_prompt_provider.requests().len(),
        3,
        "pre-prompt no-shrink must not loop provider execution"
    );

    let overflow_dir = tempfile::tempdir().expect("overflow tempdir");
    let overflow_provider = SequentialScriptedProvider::new(vec![
        provider_text_events("first answer"),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::Error {
                message: "prompt token count of 128713 exceeds the limit of 128000".to_string(),
            },
        ],
    ]);
    let overflow = test_agent_coordinator_with_provider(
        overflow_dir.path(),
        Arc::new(overflow_provider.clone()),
        1,
    );
    let overflow_run = overflow
        .start_run(
            "coord_no_loop_overflow",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start overflow run");
    let overflow_agent = overflow
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn overflow agent");
    overflow
        .request_agent_turn(supervisor_actor(), overflow_agent.clone(), "first question")
        .await
        .expect("overflow setup turn");
    tokio::time::sleep(Duration::from_millis(80)).await;
    let overflow_request_id = overflow
        .request_agent_turn(supervisor_actor(), overflow_agent, "second question")
        .await
        .expect("overflow no-shrink turn");
    let overflow_events = wait_for_events(&overflow_run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(payload)
                    if event.correlation_id.as_deref() == Some(overflow_request_id.as_str())
                        && payload.reason.contains("no checkpoint reduced the active provider context")
            )
        })
    })
    .await;
    overflow.stop_run().await.expect("stop overflow run");
    assert_eq!(
        overflow_events
            .iter()
            .filter(|event| matches!(
                &event.payload,
                EventV1::CompactionFailed(payload) if payload.trigger_reason == "overflow_retry"
            ))
            .count(),
        1,
        "overflow retry no-shrink should record exactly one failed compaction attempt"
    );
    assert_eq!(
        overflow_provider.requests().len(),
        2,
        "overflow no-shrink must not resend the same context"
    );
    assert!(overflow_events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCancelled(payload)
                if event.correlation_id.as_deref() == Some(overflow_request_id.as_str())
                    && payload.reason.contains("prompt token count")
        )
    }));

    let failed_dir = tempfile::tempdir().expect("failed-response tempdir");
    let failed_provider = SequentialScriptedProvider::new(vec![
        provider_text_events("first answer"),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta(format!(
                "partial provider output {}",
                "B".repeat(35_100)
            )),
            ProviderStreamEvent::Error {
                message: "provider exploded".to_string(),
            },
        ],
    ]);
    let failed = test_agent_coordinator_with_provider_and_compaction(
        failed_dir.path(),
        Arc::new(failed_provider.clone()),
        1,
        CompactionRuntimeConfig {
            fallback_input_tokens: 10_000,
            ..CompactionRuntimeConfig::default()
        },
    );
    let failed_run = failed
        .start_run(
            "coord_no_loop_failed_response",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start failed-response run");
    let failed_agent = failed
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn failed-response agent");
    failed
        .request_agent_turn(supervisor_actor(), failed_agent.clone(), "first question")
        .await
        .expect("failed-response setup turn");
    wait_for_events(
        &failed_run.events_path,
        Duration::from_millis(700),
        |events| {
            events.iter().any(|event| {
                matches!(
                    &event.payload,
                    EventV1::TaskCompleted(payload) if payload.result_summary == "first answer"
                )
            })
        },
    )
    .await;
    let failed_request_id = failed
        .request_agent_turn(supervisor_actor(), failed_agent, "partial then error")
        .await
        .expect("failed-response no-shrink turn");
    let failed_events = wait_for_events(
        &failed_run.events_path,
        Duration::from_millis(900),
        |events| {
            events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(payload)
                    if event.correlation_id.as_deref() == Some(failed_request_id.as_str())
                        && payload.reason == "provider exploded"
            )
        }) && events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::CompactionFailed(payload)
                    if payload.trigger_reason == "failed_response"
                        && payload.through_request_id.as_deref() == Some(failed_request_id.as_str())
            )
        })
        },
    )
    .await;
    failed.stop_run().await.expect("stop failed-response run");
    assert_eq!(
        failed_events
            .iter()
            .filter(|event| matches!(
                &event.payload,
                EventV1::CompactionFailed(payload)
                    if payload.trigger_reason == "failed_response"
                        && payload.through_request_id.as_deref() == Some(failed_request_id.as_str())
            ))
            .count(),
        1,
        "failed-response no-shrink should record exactly one failed compaction attempt"
    );
    assert_eq!(
        failed_provider.requests().len(),
        2,
        "failed-response no-shrink must not retry the provider turn"
    );
    assert!(failed_events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCancelled(payload)
                if event.correlation_id.as_deref() == Some(failed_request_id.as_str())
                    && payload.reason == "provider exploded"
        )
    }));
}

#[tokio::test]
async fn manual_compaction_writes_checkpoint_and_manual_events() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("A".repeat(12_000)),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 100,
                    completion_tokens: 100,
                    total_tokens: 200,
                },
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("B".repeat(12_000)),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 100,
                    completion_tokens: 100,
                    total_tokens: 200,
                },
            },
        ],
    ]);
    let coordinator =
        test_agent_coordinator_with_provider(temp_dir.path(), Arc::new(provider.clone()), 1);

    let run = coordinator
        .start_run(
            "coord_manual_compaction",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");

    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .expect("first turn");
    tokio::time::sleep(Duration::from_millis(80)).await;
    let second_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "second question")
        .await
        .expect("second turn");
    tokio::time::sleep(Duration::from_millis(80)).await;

    let outcome = coordinator
        .compact_agent_context(agent_id, Some(second_request_id.clone()), "manual")
        .await
        .expect("manual compaction succeeds");
    let ManualCompactionOutcome::CheckpointWritten {
        checkpoint_id,
        tokens_before_estimate,
        tokens_after_estimate,
    } = outcome
    else {
        panic!("expected checkpoint to be written");
    };
    assert!(tokens_before_estimate.is_some());
    assert!(tokens_after_estimate.is_some());
    assert!(tokens_after_estimate < tokens_before_estimate);

    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::CompactionRequested(payload)
                if payload.agent_id == "agent_000001"
                    && payload.trigger_reason == "manual"
                    && payload.through_request_id.as_deref() == Some(second_request_id.as_str())
                    && payload.checkpoint_id == checkpoint_id
        )
    }));
    let written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) if payload.trigger_reason == "manual" => {
                Some(payload.clone())
            }
            _ => None,
        })
        .expect("manual compaction written event");
    assert_eq!(written.checkpoint_id, checkpoint_id);
    assert_eq!(written.tokens_before_estimate, tokens_before_estimate);
    assert_eq!(written.tokens_after_estimate, tokens_after_estimate);
    assert_eq!(written.compacted_turns, Some(1));
    assert_eq!(
        written.through_request_id.as_deref(),
        Some(second_request_id.as_str())
    );
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::CompactionApplied(payload)
                if payload.checkpoint_id == checkpoint_id
                    && payload.through_request_id.as_deref() == Some(second_request_id.as_str())
                    && payload.tokens_before_estimate == tokens_before_estimate
                    && payload.tokens_after_estimate == tokens_after_estimate
        )
    }));
    let checkpoint_path = run.run_dir.join(&written.artifact_path);
    assert!(
        checkpoint_path.exists(),
        "checkpoint artifact should be written"
    );
}

#[tokio::test]
async fn manual_compaction_after_four_small_turns_writes_checkpoint_with_latest_turn() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(
        [
            "first answer",
            "second answer",
            "third answer",
            "fourth answer",
        ]
        .into_iter()
        .map(|answer| {
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::TextDelta(answer.to_string()),
                ProviderStreamEvent::Done {
                    usage: CompletionUsage {
                        prompt_tokens: 100,
                        completion_tokens: 100,
                        total_tokens: 200,
                    },
                },
            ]
        })
        .collect(),
    );
    let coordinator =
        test_agent_coordinator_with_provider(temp_dir.path(), Arc::new(provider.clone()), 1);

    let run = coordinator
        .start_run(
            "coord_manual_compaction_forced_checkpoint",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");

    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    for question in [
        "first small question",
        "second small question",
        "third small question",
        "fourth small question",
    ] {
        coordinator
            .request_agent_turn(supervisor_actor(), agent_id.clone(), question)
            .await
            .expect("turn request");
        tokio::time::sleep(Duration::from_millis(80)).await;
    }

    let outcome = coordinator
        .compact_agent_context(agent_id, Some("req_000004".to_string()), "manual")
        .await
        .expect("manual compaction succeeds");
    let ManualCompactionOutcome::CheckpointWritten { checkpoint_id, .. } = outcome else {
        panic!("expected manual compaction to force a checkpoint");
    };

    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) if payload.trigger_reason == "manual" => {
                Some(payload.clone())
            }
            _ => None,
        })
        .expect("manual compaction written event");
    assert_eq!(written.checkpoint_id, checkpoint_id);
    assert_eq!(written.preserved_turns, 1);

    let checkpoint_path = run.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(&checkpoint_path).expect("read checkpoint artifact");
    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_str(&checkpoint_body).expect("parse checkpoint artifact");
    assert_eq!(
        checkpoint.metadata.trigger_reason.as_deref(),
        Some("manual")
    );
    assert_eq!(checkpoint.recent_turns.len(), 1);
    assert_eq!(
        checkpoint.recent_turns[0].user_prompt,
        "fourth small question"
    );
    assert_eq!(
        checkpoint.recent_turns[0].assistant_response,
        "fourth answer"
    );
    assert!(checkpoint.summary.contains("first small question"));
    assert!(checkpoint.summary.contains("second small question"));
    assert!(checkpoint.summary.contains("third small question"));
    assert!(!checkpoint.summary.contains("fourth small question"));
}

#[tokio::test]
async fn manual_compaction_after_two_turns_summarizes_first_and_preserves_latest() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(
        ["first answer", "second answer"]
            .into_iter()
            .map(|answer| {
                vec![
                    ProviderStreamEvent::Start,
                    ProviderStreamEvent::TextDelta(answer.to_string()),
                    ProviderStreamEvent::Done {
                        usage: CompletionUsage {
                            prompt_tokens: 100,
                            completion_tokens: 100,
                            total_tokens: 200,
                        },
                    },
                ]
            })
            .collect(),
    );
    let coordinator =
        test_agent_coordinator_with_provider(temp_dir.path(), Arc::new(provider.clone()), 1);

    let run = coordinator
        .start_run(
            "coord_manual_compaction_two_turns",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");

    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    for question in ["first question", "second question"] {
        coordinator
            .request_agent_turn(supervisor_actor(), agent_id.clone(), question)
            .await
            .expect("turn request");
        tokio::time::sleep(Duration::from_millis(80)).await;
    }

    let outcome = coordinator
        .compact_agent_context(agent_id, Some("req_000002".to_string()), "manual")
        .await
        .expect("manual compaction succeeds");
    let ManualCompactionOutcome::CheckpointWritten { checkpoint_id, .. } = outcome else {
        panic!("expected manual compaction to force a checkpoint");
    };

    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) if payload.trigger_reason == "manual" => {
                Some(payload.clone())
            }
            _ => None,
        })
        .expect("manual compaction written event");
    assert_eq!(written.checkpoint_id, checkpoint_id);
    assert_eq!(written.trigger_reason, "manual");
    assert_eq!(written.preserved_turns, 1);

    let checkpoint_path = run.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(&checkpoint_path).expect("read checkpoint artifact");
    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_str(&checkpoint_body).expect("parse checkpoint artifact");
    assert_eq!(
        checkpoint.metadata.trigger_reason.as_deref(),
        Some("manual")
    );
    assert_eq!(checkpoint.recent_turns.len(), 1);
    assert_eq!(checkpoint.recent_turns[0].user_prompt, "second question");
    assert_eq!(
        checkpoint.recent_turns[0].assistant_response,
        "second answer"
    );
    assert!(checkpoint.summary.contains("first question"));
    assert!(checkpoint.summary.contains("first answer"));
    assert!(!checkpoint.summary.contains("second question"));
    assert!(!checkpoint.summary.contains("second answer"));
}

#[tokio::test]
async fn manual_compaction_uses_optional_model_backed_summary_without_provider_events() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let model_summary = structured_model_summary("model kept the goal", "model next step");
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        provider_text_events(&"B".repeat(12_000)),
        provider_text_events(&model_summary),
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        1,
        CompactionRuntimeConfig {
            model_backed: true,
            model_ref: Some("mock:model-1".to_string()),
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_manual_model_backed_compaction",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    for question in ["first question", "second question"] {
        coordinator
            .request_agent_turn(supervisor_actor(), agent_id.clone(), question)
            .await
            .expect("turn request");
        tokio::time::sleep(Duration::from_millis(80)).await;
    }

    coordinator
        .compact_agent_context(agent_id, Some("req_000002".to_string()), "manual")
        .await
        .expect("manual compaction succeeds");
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let provider_started_count = events
        .iter()
        .filter(|event| matches!(event.payload, EventV1::ProviderRequestStarted(_)))
        .count();
    assert_eq!(
        provider_started_count, 2,
        "compaction model calls stay out of events"
    );
    assert_eq!(
        provider.requests().len(),
        3,
        "two turns plus one summary model call"
    );
    let checkpoint = manual_checkpoint(&run, &events);
    assert_eq!(checkpoint.summary.trim(), model_summary.trim());
    let source = checkpoint.summary_source.expect("summary source metadata");
    assert_eq!(source.strategy, "model_backed_summary");
    assert!(source.model_backed);
    assert!(!source.deterministic_fallback);
}

#[tokio::test]
async fn model_backed_compaction_falls_back_for_invalid_summary_and_records_metadata() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        provider_text_events(&"B".repeat(12_000)),
        provider_text_events("not a structured checkpoint"),
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        1,
        CompactionRuntimeConfig {
            model_backed: true,
            model_ref: Some("mock:model-1".to_string()),
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_model_compaction_fallback",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    for question in ["first question", "second question"] {
        coordinator
            .request_agent_turn(supervisor_actor(), agent_id.clone(), question)
            .await
            .expect("turn request");
        tokio::time::sleep(Duration::from_millis(80)).await;
    }

    coordinator
        .compact_agent_context(agent_id, Some("req_000002".to_string()), "manual")
        .await
        .expect("manual compaction succeeds");
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let checkpoint = manual_checkpoint(&run, &events);
    let source = checkpoint.summary_source.expect("summary source metadata");
    assert_eq!(source.strategy, "model_backed_deterministic_fallback");
    assert!(source.model_backed);
    assert!(source.deterministic_fallback);
    assert!(checkpoint.summary.contains("## Goal"));
    assert!(checkpoint.summary.contains("first question"));
    assert!(!checkpoint.summary.contains("not a structured checkpoint"));
}

#[tokio::test]
async fn hook_summary_override_takes_precedence_over_model_backed_compaction() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        provider_text_events(&"B".repeat(12_000)),
    ]);
    let hook_runtime_config = HookRuntimeConfig {
        hooks: HooksConfig {
            lifecycle: vec![LifecycleHookConfig {
                id: Some("compaction-summary".to_string()),
                event: HookLifecycleEvent::CompactionRequested,
                command: vec![
                    "bash".to_string(),
                    "-lc".to_string(),
                    "printf 'compaction_summary: hook supplied checkpoint recap'".to_string(),
                ],
                cwd: Some(".".to_string()),
                timeout_ms: 4_000,
                critical: false,
                env: BTreeMap::new(),
            }],
        },
        shell_allowlist: ShellAllowlist {
            executables: vec!["bash".to_string()],
            cwd_roots: vec![".".to_string()],
        },
        suppress_execution: false,
    };
    let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.provider_model_concurrency = 1;
    config.provider = Arc::new(provider.clone());
    config.agent_profiles = agent_profiles();
    config.hook_runtime_config = hook_runtime_config;
    config.compaction = CompactionRuntimeConfig {
        model_backed: true,
        model_ref: Some("mock:model-1".to_string()),
        ..CompactionRuntimeConfig::default()
    };
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = coordinator
        .start_run(
            "coord_hook_compaction_summary_precedence",
            temp_dir.path().to_path_buf(),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    for question in ["first question", "second question"] {
        coordinator
            .request_agent_turn(supervisor_actor(), agent_id.clone(), question)
            .await
            .expect("turn request");
        tokio::time::sleep(Duration::from_millis(80)).await;
    }

    coordinator
        .compact_agent_context(agent_id, Some("req_000002".to_string()), "manual")
        .await
        .expect("manual compaction succeeds");
    coordinator.stop_run().await.expect("stop run");

    assert_eq!(
        provider.requests().len(),
        2,
        "hook override prevents model summary call"
    );
    let events = load_events(&run.events_path);
    let checkpoint = manual_checkpoint(&run, &events);
    assert_eq!(checkpoint.summary, "hook supplied checkpoint recap");
    let source = checkpoint.summary_source.expect("summary source metadata");
    assert_eq!(source.strategy, "hook_supplied_summary");
    assert!(!source.model_backed);
    assert!(!source.deterministic_fallback);
}

#[tokio::test]
async fn overflow_retry_split_oversized_latest_turn_preserves_suffix_context() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let oversized_answer = "B".repeat(12_000);
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events("first compacted answer"),
        provider_text_events(&oversized_answer),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::Error {
                message: "prompt token count of 128713 exceeds the limit of 128000".to_string(),
            },
        ],
        provider_text_events("recovered answer"),
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        1,
        CompactionRuntimeConfig {
            split_oversized_turns: true,
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_overflow_retry_split_tail",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .expect("first turn");
    tokio::time::sleep(Duration::from_millis(80)).await;
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "second question")
        .await
        .expect("second turn");
    tokio::time::sleep(Duration::from_millis(80)).await;
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "third question")
        .await
        .expect("third turn triggers overflow retry");
    tokio::time::sleep(Duration::from_millis(180)).await;
    coordinator.stop_run().await.expect("stop run");

    let retried_messages = provider
        .requests()
        .last()
        .expect("retried request")
        .messages
        .clone();
    assert!(retried_messages.iter().any(|message| {
        message.role == MessageRole::User
            && message
                .content
                .contains("preserved suffix of an oversized latest turn")
            && message
                .content
                .contains("earlier prefix is summarized in the checkpoint")
    }));
    assert!(retried_messages.iter().any(|message| {
        message.role == MessageRole::Assistant && message.content.len() < 12_000
    }));

    let events = load_events(&run.events_path);
    let checkpoint = overflow_checkpoint(&run, &events);
    assert_eq!(checkpoint.recent_turns.len(), 1);
    assert!(checkpoint
        .summary
        .contains("earlier prefix of an oversized latest turn"));
    assert!(checkpoint
        .facts
        .compacted_turns
        .iter()
        .any(|fact| fact.user_excerpt.contains("first question")));
    assert!(checkpoint.facts.compacted_turns.iter().any(|fact| fact
        .user_excerpt
        .contains("earlier prefix of an oversized latest turn")));
    assert!(checkpoint.recent_turns[0]
        .user_prompt
        .contains("preserved suffix of an oversized latest turn"));
    let tail_boundary = checkpoint.tail_boundary.as_ref().expect("tail boundary");
    assert_eq!(tail_boundary.mode, "split_oversized_turn_tail");
    assert!(tail_boundary
        .split_prefix_summary
        .as_deref()
        .is_some_and(|summary| summary.contains('B')));
    assert!(checkpoint.summary.contains("Split prefix summary"));
    assert!(checkpoint
        .summary
        .contains("Source facts: split prefix summary"));
}

#[tokio::test]
async fn model_backed_overflow_split_uses_model_prefix_summary() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let oversized_answer = format!(
        "MODEL_PREFIX_ANCHOR {} MODEL_SUFFIX_ANCHOR",
        "M".repeat(12_000)
    );
    let model_prefix_summary = "## Original Request\nSummarize the latest oversized model-backed turn.\n\n## Early Progress\n- MODEL_PREFIX_SUMMARY captured early progress from the prefix.\n\n## Context for Suffix\n- Continue from the retained suffix using MODEL_PREFIX_SUMMARY.";
    let model_checkpoint_summary = structured_split_model_summary(
        "model split goal",
        "continue after split",
        model_prefix_summary,
    );
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events("first compacted answer"),
        provider_text_events(&oversized_answer),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::Error {
                message: "prompt token count of 128713 exceeds the limit of 128000".to_string(),
            },
        ],
        provider_text_events(model_prefix_summary),
        provider_text_events(&model_checkpoint_summary),
        provider_text_events("recovered answer"),
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        1,
        CompactionRuntimeConfig {
            model_backed: true,
            model_ref: Some("mock:model-1".to_string()),
            split_oversized_turns: true,
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_model_backed_overflow_split_prefix",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .expect("first turn");
    tokio::time::sleep(Duration::from_millis(80)).await;
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "second question")
        .await
        .expect("second turn");
    tokio::time::sleep(Duration::from_millis(80)).await;
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "third question")
        .await
        .expect("third turn triggers overflow retry");
    tokio::time::sleep(Duration::from_millis(180)).await;
    coordinator.stop_run().await.expect("stop run");

    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        6,
        "two turns, failed turn, prefix summary, checkpoint summary, retry"
    );
    let prefix_request = &requests[3];
    assert!(prefix_request
        .messages
        .iter()
        .any(|message| message.content.contains("This is the PREFIX of a turn")));
    assert!(prefix_request
        .messages
        .iter()
        .any(|message| message.content.contains("MODEL_PREFIX_ANCHOR")));
    assert!(requests[4]
        .messages
        .iter()
        .any(|message| message.content.contains("MODEL_PREFIX_SUMMARY")));

    let events = load_events(&run.events_path);
    let checkpoint = overflow_checkpoint(&run, &events);
    let tail_boundary = checkpoint.tail_boundary.as_ref().expect("tail boundary");
    assert_eq!(tail_boundary.mode, "split_oversized_turn_tail");
    assert!(tail_boundary
        .split_prefix_summary
        .as_deref()
        .is_some_and(|summary| summary.contains("MODEL_PREFIX_SUMMARY")));
    assert!(tail_boundary
        .note
        .as_deref()
        .is_some_and(|note| note.contains("Split prefix summary source: model_backed.")));
    assert!(checkpoint.summary.contains("MODEL_PREFIX_SUMMARY"));
    let source = checkpoint.summary_source.expect("summary source metadata");
    assert_eq!(source.strategy, "model_backed_summary");
    assert!(source.model_backed);
    assert!(!source.deterministic_fallback);
}

#[tokio::test]
async fn model_backed_overflow_split_summary_without_prefix_content_falls_back() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let oversized_answer = format!(
        "MISSING_PREFIX_CONTENT_ANCHOR {} MISSING_SUFFIX_ANCHOR",
        "P".repeat(12_000)
    );
    let model_prefix_summary = "## Original Request\nSummarize the latest oversized turn.\n\n## Early Progress\n- MISSING_MODEL_PREFIX_SUMMARY captured early work.\n\n## Context for Suffix\n- Continue with MISSING_MODEL_PREFIX_SUMMARY.";
    let invalid_checkpoint_summary = structured_split_model_summary(
        "invalid split goal",
        "continue after invalid split",
        "label present but actual prefix content omitted",
    );
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events("first compacted answer"),
        provider_text_events(&oversized_answer),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::Error {
                message: "prompt token count of 128713 exceeds the limit of 128000".to_string(),
            },
        ],
        provider_text_events(model_prefix_summary),
        provider_text_events(&invalid_checkpoint_summary),
        provider_text_events("recovered answer"),
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        1,
        CompactionRuntimeConfig {
            model_backed: true,
            model_ref: Some("mock:model-1".to_string()),
            split_oversized_turns: true,
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_model_backed_overflow_split_missing_prefix_content",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .expect("first turn");
    tokio::time::sleep(Duration::from_millis(80)).await;
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "second question")
        .await
        .expect("second turn");
    tokio::time::sleep(Duration::from_millis(80)).await;
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "third question")
        .await
        .expect("third turn triggers overflow retry");
    tokio::time::sleep(Duration::from_millis(180)).await;
    coordinator.stop_run().await.expect("stop run");

    assert_eq!(
        provider.requests().len(),
        6,
        "invalid checkpoint summary still allows deterministic compaction and retry"
    );
    let events = load_events(&run.events_path);
    let checkpoint = overflow_checkpoint(&run, &events);
    let source = checkpoint.summary_source.expect("summary source metadata");
    assert_eq!(source.strategy, "model_backed_deterministic_fallback");
    assert!(source.model_backed);
    assert!(source.deterministic_fallback);
    assert!(checkpoint.summary.contains("MISSING_PREFIX_CONTENT_ANCHOR"));
    assert!(!checkpoint
        .summary
        .contains("label present but actual prefix content omitted"));
    assert!(checkpoint
        .tail_boundary
        .as_ref()
        .and_then(|boundary| boundary.split_prefix_summary.as_deref())
        .is_some_and(|summary| summary.contains("MISSING_PREFIX_CONTENT_ANCHOR")));
}

#[tokio::test]
async fn model_backed_overflow_split_empty_prefix_summary_falls_back_deterministically() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let oversized_answer = format!(
        "FALLBACK_PREFIX_ANCHOR {} FALLBACK_SUFFIX_ANCHOR",
        "N".repeat(12_000)
    );
    let deterministic_prefix_excerpt = test_compaction_excerpt(&oversized_answer);
    let model_checkpoint_summary = structured_split_model_summary(
        "fallback split goal",
        "continue after fallback split",
        &deterministic_prefix_excerpt,
    );
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events("first compacted answer"),
        provider_text_events(&oversized_answer),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::Error {
                message: "prompt token count of 128713 exceeds the limit of 128000".to_string(),
            },
        ],
        provider_text_events(""),
        provider_text_events(&model_checkpoint_summary),
        provider_text_events("recovered answer"),
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        1,
        CompactionRuntimeConfig {
            model_backed: true,
            model_ref: Some("mock:model-1".to_string()),
            split_oversized_turns: true,
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_model_backed_overflow_split_prefix_fallback",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .expect("first turn");
    tokio::time::sleep(Duration::from_millis(80)).await;
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "second question")
        .await
        .expect("second turn");
    tokio::time::sleep(Duration::from_millis(80)).await;
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "third question")
        .await
        .expect("third turn triggers overflow retry");
    tokio::time::sleep(Duration::from_millis(180)).await;
    coordinator.stop_run().await.expect("stop run");

    assert_eq!(
        provider.requests().len(),
        6,
        "empty prefix output still falls through to checkpoint summary and retry"
    );
    let events = load_events(&run.events_path);
    let checkpoint = overflow_checkpoint(&run, &events);
    let tail_boundary = checkpoint.tail_boundary.as_ref().expect("tail boundary");
    assert_eq!(tail_boundary.mode, "split_oversized_turn_tail");
    assert!(tail_boundary
        .split_prefix_summary
        .as_deref()
        .is_some_and(|summary| summary.contains("FALLBACK_PREFIX_ANCHOR")));
    let note = tail_boundary.note.as_deref().expect("tail note");
    assert!(note.contains("Split prefix summary source: model_backed_deterministic_fallback."));
    assert!(note.contains("model split prefix summary was empty"));
    assert!(checkpoint.summary.contains("FALLBACK_PREFIX_ANCHOR"));
    let source = checkpoint.summary_source.expect("summary source metadata");
    assert_eq!(source.strategy, "model_backed_summary");
    assert!(source.model_backed);
    assert!(!source.deterministic_fallback);
}

#[tokio::test]
async fn overflow_auto_retry_can_be_disabled_by_compaction_config() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events("first answer"),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::Error {
                message: "prompt token count of 128713 exceeds the limit of 128000".to_string(),
            },
        ],
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        1,
        CompactionRuntimeConfig {
            auto_retry_overflow: false,
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_overflow_retry_disabled",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .expect("first turn");
    tokio::time::sleep(Duration::from_millis(80)).await;
    let second_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "second question")
        .await
        .expect("second turn");
    tokio::time::sleep(Duration::from_millis(120)).await;
    coordinator.stop_run().await.expect("stop run");

    assert_eq!(provider.requests().len(), 2);
    let events = load_events(&run.events_path);
    assert!(events
        .iter()
        .all(|event| !matches!(event.payload, EventV1::CompactionRequested(_))));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCancelled(payload)
                if event.correlation_id.as_deref() == Some(second_request_id.as_str())
                    && payload.reason.contains("prompt token count")
        )
    }));
}

#[tokio::test]
async fn manual_compaction_returns_noop_when_context_has_single_turn() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::TextDelta("first answer".to_string()),
        ProviderStreamEvent::Done {
            usage: CompletionUsage {
                prompt_tokens: 32,
                completion_tokens: 8,
                total_tokens: 40,
            },
        },
    ]]);
    let coordinator =
        test_agent_coordinator_with_provider(temp_dir.path(), Arc::new(provider.clone()), 1);

    let run = coordinator
        .start_run(
            "coord_manual_compaction_noop",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");

    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let first_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .expect("first turn");
    tokio::time::sleep(Duration::from_millis(80)).await;

    let outcome = coordinator
        .compact_agent_context(agent_id, Some(first_request_id), "manual")
        .await
        .expect("manual noop succeeds");
    assert_eq!(outcome, ManualCompactionOutcome::NoOp);

    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    assert!(events.iter().all(|event| {
        !matches!(
            event.payload,
            EventV1::CompactionRequested(_)
                | EventV1::CompactionWritten(_)
                | EventV1::CompactionApplied(_)
        )
    }));
}

#[tokio::test]
async fn resume_rejects_missing_user_message_when_prompt_summary_is_truncated() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_resume_truncated_prompt_summary";
    let events_path = write_resume_fixture(
        temp_dir.path(),
        run_id,
        &[
            resume_fixture_event(
                run_id,
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            resume_fixture_event(
                run_id,
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "alpha".to_string(),
                    parent_agent_id: None,
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                3,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "truncated historical prompt…".to_string(),
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
                    request_id: "req_000001".to_string(),
                    delta: "first answer".to_string(),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                5,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000001".to_string(),
                    result_summary: "first answer".to_string(),
                    result_digest: "digest-task-1".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                6,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "segment complete".to_string(),
                }),
            ),
        ],
    );

    let before = load_events(&events_path);
    let coordinator = test_resume_coordinator(temp_dir.path());
    let error = coordinator
        .resume_run(run_id, "interactive")
        .await
        .expect_err("truncated prompt summaries must fail closed");

    let CoordinatorError::ResumeRestoreFailed {
        run_id: restored_run_id,
        reason,
    } = error
    else {
        panic!("expected resume restore failure");
    };
    assert_eq!(restored_run_id, run_id);
    assert!(
        reason.contains("prompt_summary is truncated"),
        "unexpected restore failure reason: {reason}"
    );

    let after = load_events(&events_path);
    assert_eq!(after.len(), before.len(), "resume failure must not append");
    assert_eq!(after.last().map(|event| event.seq), Some(6));
}

#[tokio::test]
async fn tool_task_lifecycle_events_preserve_owner_actor() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = Arc::new(FakeClock::new());
    let coordinator = test_tool_lifecycle_coordinator(
        temp_dir.path(),
        clock,
        lifecycle_tool_registry(Arc::new(Notify::new())),
        Duration::from_millis(100),
        15_000,
        5,
        2,
    );

    let run = coordinator
        .start_run("tool_task_owner", temp_dir.path().to_path_buf())
        .await
        .expect("start run");

    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle agent");
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "alpha-prompt")
        .await
        .expect("request agent turn");
    let owner_actor = EventActor::new(ActorKind::Worker, Some(agent_id));
    tokio::time::sleep(Duration::from_millis(20)).await;

    coordinator
        .request_tool_call(
            owner_actor.clone(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "true"}),
        )
        .await
        .expect("request successful tool call");
    coordinator
        .request_tool_call(
            owner_actor.clone(),
            Some("deep".to_string()),
            "shell.fail",
            json!({"cmd": "false"}),
        )
        .await
        .expect("request failing tool call");

    tokio::time::sleep(Duration::from_millis(400)).await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let tool_task_ids = tool_task_ids(&events);
    assert_eq!(tool_task_ids.len(), 2, "expected two tool task ids");

    let scheduled_events = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventV1::TaskScheduled(data) if tool_task_ids.contains(&data.task_id)
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        scheduled_events.len(),
        2,
        "expected two tool TaskScheduled events"
    );
    for event in scheduled_events {
        assert_task_event_context(event, &owner_actor, &request_id);
    }

    let terminal_events = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data) if tool_task_ids.contains(&data.task_id)
            ) || matches!(
                &event.payload,
                EventV1::TaskCancelled(data) if tool_task_ids.contains(&data.task_id)
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        terminal_events.len(),
        2,
        "expected two tool terminal events"
    );

    let completed = terminal_events
        .iter()
        .filter(|event| matches!(&event.payload, EventV1::TaskCompleted(_)))
        .count();
    let cancelled = terminal_events
        .iter()
        .filter(|event| matches!(&event.payload, EventV1::TaskCancelled(_)))
        .count();
    assert_eq!(completed, 1, "expected one tool completion");
    assert_eq!(cancelled, 1, "expected one tool cancellation");

    for event in terminal_events {
        assert_task_event_context(event, &owner_actor, &request_id);
    }
}

#[tokio::test]
async fn stale_tool_task_late_result_preserves_owner_actor() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = Arc::new(FakeClock::new());
    let coordinator = test_tool_lifecycle_coordinator(
        temp_dir.path(),
        clock.clone(),
        lifecycle_tool_registry(Arc::new(Notify::new())),
        Duration::from_millis(100),
        10,
        5,
        1,
    );

    let run = coordinator
        .start_run("stale_tool_task_owner", temp_dir.path().to_path_buf())
        .await
        .expect("start run");

    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle agent");
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "alpha-prompt")
        .await
        .expect("request agent turn");
    let owner_actor = EventActor::new(ActorKind::Worker, Some(agent_id));
    tokio::time::sleep(Duration::from_millis(20)).await;

    coordinator
        .request_tool_call(
            owner_actor.clone(),
            Some("deep".to_string()),
            "shell.block",
            json!({"cmd": "wait"}),
        )
        .await
        .expect("request blocking tool call");

    tokio::time::sleep(Duration::from_millis(50)).await;
    let task_id = load_events(&run.events_path)
        .into_iter()
        .find_map(|event| match event.payload {
            EventV1::TaskScheduled(data)
                if data.queue_key.as_deref() == Some("tool:shell.block") =>
            {
                Some(data.task_id)
            }
            _ => None,
        })
        .expect("blocking tool task id");
    coordinator
        .job_progress(task_id.clone(), JobProgressKind::Heartbeat)
        .await
        .expect("refresh tool heartbeat before cancellation");
    coordinator
        .cancel_task(task_id.clone(), "manual cancellation")
        .await
        .expect("cancel tool task");
    coordinator
        .job_finished(
            task_id.clone(),
            JobOutcome::Cancelled {
                reason: "job cancelled".to_string(),
            },
        )
        .await
        .expect("record late tool result");

    clock.advance(25);
    tokio::time::sleep(Duration::from_millis(200)).await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let cancelled_event = events
        .iter()
        .find(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data) if data.task_id == task_id
            )
        })
        .expect("cancelled event");
    assert_task_event_context(cancelled_event, &owner_actor, &request_id);

    let late_event = events
        .iter()
        .find(|event| {
            matches!(
                &event.payload,
                EventV1::TaskResultLate(data) if data.task_id == task_id
            )
        })
        .expect("late result event");
    assert_task_event_context(late_event, &owner_actor, &request_id);
}

#[tokio::test]
async fn critical_hook_failure_fails_closed_and_records_metadata() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let hook_output_path = temp_dir.path().join("hook-finish.txt");
    let hook_runtime_config = HookRuntimeConfig {
        hooks: HooksConfig {
            lifecycle: vec![
                LifecycleHookConfig {
                    id: Some("tool-start-timeout".to_string()),
                    event: HookLifecycleEvent::ToolCallStarted,
                    command: vec![
                        "bash".to_string(),
                        "-lc".to_string(),
                        "sleep 0.05".to_string(),
                    ],
                    cwd: Some(".".to_string()),
                    timeout_ms: 10,
                    critical: false,
                    env: BTreeMap::new(),
                },
                LifecycleHookConfig {
                    id: Some("tool-finish-critical".to_string()),
                    event: HookLifecycleEvent::ToolCallFinished,
                    command: vec![
                        "bash".to_string(),
                        "-lc".to_string(),
                        "printf '%s|%s|%s|%s' \"$PWD\" \"$HOOK_CUSTOM\" \"$HARNESS_HOOK_EVENT\" \"$HARNESS_HOOK_TOOL_ID\" > \"$HOOK_OUTPUT_PATH\"; exit 23".to_string(),
                    ],
                    cwd: Some(".".to_string()),
                    timeout_ms: 4_000,
                    critical: true,
                    env: BTreeMap::from([
                        ("HOOK_CUSTOM".to_string(), "from-config".to_string()),
                        (
                            "HOOK_OUTPUT_PATH".to_string(),
                            hook_output_path.display().to_string(),
                        ),
                    ]),
                },
            ],
        },
        shell_allowlist: ShellAllowlist {
            executables: vec!["bash".to_string()],
            cwd_roots: vec![".".to_string()],
        },
        suppress_execution: false,
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
            "critical_hook_failure_fails_closed_and_records_metadata",
            temp_dir.path().to_path_buf(),
        )
        .await
        .expect("start run");

    let tool_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "true"}),
        )
        .await
        .expect("request tool call");

    tokio::time::sleep(Duration::from_millis(250)).await;
    coordinator.stop_run().await.expect("stop run");

    let hook_output = fs::read_to_string(&hook_output_path).expect("hook output file");
    assert!(
        hook_output.starts_with(&temp_dir.path().display().to_string()),
        "hook should execute from workspace-root cwd: {hook_output}"
    );
    assert!(hook_output.contains("from-config|tool_call_finished|shell.run"));

    let events = load_events(&run.events_path);
    let task_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskScheduled(data) if data.queue_key.as_deref() == Some("tool:shell.run") => {
                Some(data.task_id.clone())
            }
            _ => None,
        })
        .expect("tool task id");

    assert!(
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data) if data.task_id == task_id
            )
        }),
        "critical finish hook should fail closed and cancel the task"
    );
    assert!(
        !events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data) if data.task_id == task_id
            )
        }),
        "critical finish hook must prevent successful task completion"
    );

    let tool_finished = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallFinished(data) if data.tool_call_id == tool_call_id => Some(data),
            _ => None,
        })
        .expect("tool finished event");
    assert_eq!(tool_finished.status, ToolCallStatus::Failed);
    let hook_executions = tool_finished
        .metadata
        .as_ref()
        .map(|metadata| metadata.hook_executions.clone())
        .expect("hook metadata on tool finish");
    assert_eq!(hook_executions.len(), 2, "expected both hooks recorded");
    assert_eq!(hook_executions[0].hook_name, "tool-start-timeout");
    assert_eq!(hook_executions[0].status, HookExecutionStatus::Failed);
    assert_eq!(
        hook_executions[0].hook_event.as_deref(),
        Some("tool_call_started")
    );
    assert_eq!(
        hook_executions[0].output_summary.as_deref(),
        Some("no output")
    );
    assert_eq!(hook_executions[1].hook_name, "tool-finish-critical");
    assert_eq!(hook_executions[1].status, HookExecutionStatus::Failed);
    assert_eq!(
        hook_executions[1].hook_event.as_deref(),
        Some("tool_call_finished")
    );
    assert_eq!(
        hook_executions[1].output_summary.as_deref(),
        Some("no output")
    );
}

#[tokio::test]
async fn noncritical_hook_failure_records_metadata_without_cancelling_task() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let hook_output_path = temp_dir.path().join("hook-finish-noncritical.txt");
    let hook_runtime_config = HookRuntimeConfig {
        hooks: HooksConfig {
            lifecycle: vec![LifecycleHookConfig {
                id: Some("tool-finish-noncritical".to_string()),
                event: HookLifecycleEvent::ToolCallFinished,
                command: vec![
                    "bash".to_string(),
                    "-lc".to_string(),
                    "printf '%s|%s|%s' \"$PWD\" \"$HARNESS_HOOK_EVENT\" \"$HARNESS_HOOK_TOOL_ID\" > \"$HOOK_OUTPUT_PATH\"; exit 17"
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
        },
        suppress_execution: false,
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
            "noncritical_hook_failure_records_metadata_without_cancelling_task",
            temp_dir.path().to_path_buf(),
        )
        .await
        .expect("start run");

    let tool_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "true"}),
        )
        .await
        .expect("request tool call");

    tokio::time::sleep(Duration::from_millis(250)).await;
    coordinator.stop_run().await.expect("stop run");

    let hook_output = fs::read_to_string(&hook_output_path).expect("hook output file");
    assert!(
        hook_output.starts_with(&temp_dir.path().display().to_string()),
        "hook should execute from workspace-root cwd: {hook_output}"
    );
    assert!(hook_output.contains("tool_call_finished|shell.run"));

    let events = load_events(&run.events_path);
    let task_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskScheduled(data) if data.queue_key.as_deref() == Some("tool:shell.run") => {
                Some(data.task_id.clone())
            }
            _ => None,
        })
        .expect("tool task id");

    assert!(
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data) if data.task_id == task_id
            )
        }),
        "non-critical hook failure should keep the task completion intact"
    );
    assert!(
        !events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data) if data.task_id == task_id
            )
        }),
        "non-critical hook failure should not cancel the task"
    );

    let tool_finished = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallFinished(data) if data.tool_call_id == tool_call_id => Some(data),
            _ => None,
        })
        .expect("tool finished event");
    assert_eq!(tool_finished.status, ToolCallStatus::Succeeded);
    let hook_executions = tool_finished
        .metadata
        .as_ref()
        .map(|metadata| metadata.hook_executions.clone())
        .expect("hook metadata on tool finish");
    assert_eq!(
        hook_executions.len(),
        1,
        "expected one failed hook recorded"
    );
    assert_eq!(hook_executions[0].hook_name, "tool-finish-noncritical");
    assert_eq!(hook_executions[0].status, HookExecutionStatus::Failed);
    assert_eq!(
        hook_executions[0].hook_event.as_deref(),
        Some("tool_call_finished")
    );
    assert_eq!(
        hook_executions[0].output_summary.as_deref(),
        Some("no output")
    );
}

#[test]
fn hook_runner_blocks_critical_and_reports_noncritical_failures() {
    critical_hook_failure_fails_closed_and_records_metadata();
    noncritical_hook_failure_records_metadata_without_cancelling_task();
}

#[tokio::test]
async fn lifecycle_hooks_cover_provider_subagent_and_permission_events() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let hook_output_path = temp_dir.path().join("hook-lifecycle-events.txt");
    let hook_command = "printf '%s|agent=%s|parent=%s|request=%s|permission=%s|tool_call=%s|provider=%s|outcome=%s\\n' \"$HARNESS_HOOK_EVENT\" \"${HARNESS_HOOK_AGENT_ID:-}\" \"${HARNESS_HOOK_PARENT_AGENT_ID:-}\" \"${HARNESS_HOOK_REQUEST_ID:-}\" \"${HARNESS_HOOK_PERMISSION_ID:-}\" \"${HARNESS_HOOK_TOOL_CALL_ID:-}\" \"${HARNESS_HOOK_PROVIDER_ID:-}\" \"${HARNESS_HOOK_OUTCOME:-}\" >> \"$HOOK_OUTPUT_PATH\"";
    let hook_env = BTreeMap::from([(
        "HOOK_OUTPUT_PATH".to_string(),
        hook_output_path.display().to_string(),
    )]);
    let hook_runtime_config = HookRuntimeConfig {
        hooks: HooksConfig {
            lifecycle: vec![
                LifecycleHookConfig {
                    id: Some("subagent-spawned".to_string()),
                    event: HookLifecycleEvent::SubagentSpawned,
                    command: vec![
                        "bash".to_string(),
                        "-lc".to_string(),
                        hook_command.to_string(),
                    ],
                    cwd: Some(".".to_string()),
                    timeout_ms: 4_000,
                    critical: false,
                    env: hook_env.clone(),
                },
                LifecycleHookConfig {
                    id: Some("provider-started".to_string()),
                    event: HookLifecycleEvent::ProviderRequestStarted,
                    command: vec![
                        "bash".to_string(),
                        "-lc".to_string(),
                        hook_command.to_string(),
                    ],
                    cwd: Some(".".to_string()),
                    timeout_ms: 4_000,
                    critical: false,
                    env: hook_env.clone(),
                },
                LifecycleHookConfig {
                    id: Some("provider-finished".to_string()),
                    event: HookLifecycleEvent::ProviderRequestFinished,
                    command: vec![
                        "bash".to_string(),
                        "-lc".to_string(),
                        hook_command.to_string(),
                    ],
                    cwd: Some(".".to_string()),
                    timeout_ms: 4_000,
                    critical: false,
                    env: hook_env.clone(),
                },
                LifecycleHookConfig {
                    id: Some("subagent-finished".to_string()),
                    event: HookLifecycleEvent::SubagentFinished,
                    command: vec![
                        "bash".to_string(),
                        "-lc".to_string(),
                        hook_command.to_string(),
                    ],
                    cwd: Some(".".to_string()),
                    timeout_ms: 4_000,
                    critical: false,
                    env: hook_env.clone(),
                },
                LifecycleHookConfig {
                    id: Some("permission-requested".to_string()),
                    event: HookLifecycleEvent::PermissionRequested,
                    command: vec![
                        "bash".to_string(),
                        "-lc".to_string(),
                        hook_command.to_string(),
                    ],
                    cwd: Some(".".to_string()),
                    timeout_ms: 4_000,
                    critical: false,
                    env: hook_env.clone(),
                },
                LifecycleHookConfig {
                    id: Some("permission-resolved".to_string()),
                    event: HookLifecycleEvent::PermissionResolved,
                    command: vec![
                        "bash".to_string(),
                        "-lc".to_string(),
                        hook_command.to_string(),
                    ],
                    cwd: Some(".".to_string()),
                    timeout_ms: 4_000,
                    critical: false,
                    env: hook_env,
                },
            ],
        },
        shell_allowlist: ShellAllowlist {
            executables: vec!["bash".to_string()],
            cwd_roots: vec![".".to_string()],
        },
        suppress_execution: false,
    };

    let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Deny,
        PermissionMode::Ask,
        PermissionMode::Deny,
    )
    .with_ask_timeout_ms(5_000);
    config.tool_registry = lifecycle_tool_registry(Arc::new(Notify::new()));
    config.provider = Arc::new(SlowMockProvider {
        inner: test_mock_provider(),
        delay: Duration::from_millis(5),
    });
    config.agent_profiles = agent_profiles();
    config.hook_runtime_config = hook_runtime_config;

    let clock = Arc::new(FakeClock::new());
    let redactor = Arc::new(DefaultRedactor::default());
    let coordinator = spawn_coordinator(config, clock, redactor);

    let _run = coordinator
        .start_run(
            "lifecycle_hooks_cover_provider_subagent_and_permission_events",
            temp_dir.path().to_path_buf(),
        )
        .await
        .expect("start run");

    let subagent_id = coordinator
        .spawn_agent(
            supervisor_actor(),
            "alpha",
            Some("agent_parent_001".to_string()),
        )
        .await
        .expect("spawn subagent");
    assert_eq!(subagent_id, "agent_000001");

    tokio::time::sleep(Duration::from_millis(180)).await;

    let tool_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "true"}),
        )
        .await
        .expect("request tool call");
    assert_eq!(tool_call_id, "toolcall_000001");

    coordinator
        .resolve_permission(
            "perm_000001",
            RuntimePermissionDecision::Allow,
            Some("approved".to_string()),
        )
        .await
        .expect("resolve permission");

    tokio::time::sleep(Duration::from_millis(250)).await;
    coordinator.stop_run().await.expect("stop run");

    let hook_output = fs::read_to_string(&hook_output_path).expect("hook output");
    let lines = hook_output.lines().collect::<Vec<_>>();

    assert!(lines.iter().any(|line| {
        line.starts_with("subagent_spawned|")
            && line.contains("agent=agent_000001")
            && line.contains("parent=agent_parent_001")
            && line.contains("outcome=spawned")
    }));
    assert!(lines.iter().any(|line| {
        line.starts_with("provider_request_started|")
            && line.contains("agent=agent_000001")
            && line.contains("request=req_000001")
            && line.contains("provider=mock")
    }));
    assert!(lines.iter().any(|line| {
        line.starts_with("provider_request_finished|") && line.contains("request=req_000001")
    }));
    assert!(lines.iter().any(|line| {
        line.starts_with("subagent_finished|")
            && line.contains("agent=agent_000001")
            && line.contains("parent=agent_parent_001")
            && line.contains("outcome=succeeded")
    }));
    assert!(lines.iter().any(|line| {
        line.starts_with("permission_requested|")
            && line.contains("permission=perm_000001")
            && line.contains("tool_call=toolcall_000001")
            && line.contains("outcome=requested")
    }));
    assert!(lines.iter().any(|line| {
        line.starts_with("permission_resolved|")
            && line.contains("permission=perm_000001")
            && line.contains("tool_call=toolcall_000001")
            && line.contains("outcome=allow")
    }));
}

#[test]
fn replay_reconstructs_parallel_child_sessions_and_timings() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_replay_parallel_child_sessions";

    let lineage_a = TaskLineageMetadata {
        parent_tool_call_id: Some("toolcall_000201".to_string()),
        parent_task_id: Some("task_000201".to_string()),
        parent_request_id: Some("req_000010".to_string()),
        parent_session_id: Some("agent_000001".to_string()),
        child_session_id: Some("agent_000101".to_string()),
        child_request_id: Some("req_000101".to_string()),
        child_provider_id: Some("mock".to_string()),
        child_model_id: Some("model-a".to_string()),
    };
    let lineage_b = TaskLineageMetadata {
        parent_tool_call_id: Some("toolcall_000202".to_string()),
        parent_task_id: Some("task_000202".to_string()),
        parent_request_id: Some("req_000010".to_string()),
        parent_session_id: Some("agent_000001".to_string()),
        child_session_id: Some("agent_000102".to_string()),
        child_request_id: Some("req_000102".to_string()),
        child_provider_id: Some("mock".to_string()),
        child_model_id: Some("model-b".to_string()),
    };

    write_resume_fixture(
        temp_dir.path(),
        run_id,
        &[
            resume_fixture_event(
                run_id,
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            resume_fixture_event(
                run_id,
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "alpha".to_string(),
                    parent_agent_id: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                3,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000101".to_string(),
                    profile: "build".to_string(),
                    parent_agent_id: Some("agent_000001".to_string()),
                }),
            ),
            resume_fixture_event(
                run_id,
                4,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000102".to_string(),
                    profile: "librarian".to_string(),
                    parent_agent_id: Some("agent_000001".to_string()),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                5,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000010"),
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000201".to_string(),
                    tool_id: "agent.spawn".to_string(),
                    args_summary: "{\"task\":\"child A\"}".to_string(),
                    args_digest: "digest-tool-a-req".to_string(),
                    metadata: Some(ToolCallMetadata {
                        canonical_tool_id: Some("agent.spawn".to_string()),
                        alias_source_tool_id: None,
                        lineage: Some(lineage_a.clone()),
                        artifact_refs: Vec::new(),
                        timing: None,
                        hook_executions: Vec::new(),
                    }),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                6,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000010"),
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000201".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("child A scheduled".to_string()),
                    output_digest: Some("digest-tool-a-out".to_string()),
                    output_json: None,
                    metadata: Some(ToolCallMetadata {
                        canonical_tool_id: Some("agent.spawn".to_string()),
                        alias_source_tool_id: None,
                        lineage: Some(lineage_a),
                        artifact_refs: Vec::new(),
                        timing: Some(ExecutionTimingMetadata {
                            started_mono_ms: Some(5),
                            finished_mono_ms: Some(6),
                            elapsed_ms: Some(1),
                        }),
                        hook_executions: Vec::new(),
                    }),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                7,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000010"),
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000202".to_string(),
                    tool_id: "agent.spawn".to_string(),
                    args_summary: "{\"task\":\"child B\"}".to_string(),
                    args_digest: "digest-tool-b-req".to_string(),
                    metadata: Some(ToolCallMetadata {
                        canonical_tool_id: Some("agent.spawn".to_string()),
                        alias_source_tool_id: None,
                        lineage: Some(lineage_b.clone()),
                        artifact_refs: Vec::new(),
                        timing: None,
                        hook_executions: Vec::new(),
                    }),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                8,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000010"),
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000202".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("child B scheduled".to_string()),
                    output_digest: Some("digest-tool-b-out".to_string()),
                    output_json: None,
                    metadata: Some(ToolCallMetadata {
                        canonical_tool_id: Some("agent.spawn".to_string()),
                        alias_source_tool_id: None,
                        lineage: Some(lineage_b),
                        artifact_refs: Vec::new(),
                        timing: Some(ExecutionTimingMetadata {
                            started_mono_ms: Some(7),
                            finished_mono_ms: Some(8),
                            elapsed_ms: Some(1),
                        }),
                        hook_executions: Vec::new(),
                    }),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                9,
                EventActor::new(ActorKind::Worker, Some("agent_000101".to_string())),
                Some("req_000101"),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000301".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:mock:model-a".to_string()),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                10,
                EventActor::new(ActorKind::Worker, Some("agent_000101".to_string())),
                Some("req_000101"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000101".to_string(),
                    provider_id: "mock".to_string(),
                    model_id: "model-a".to_string(),
                    prompt_summary: "child a prompt".to_string(),
                    request_digest: "digest-child-a".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                11,
                EventActor::new(ActorKind::Worker, Some("agent_000101".to_string())),
                Some("req_000101"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000301".to_string(),
                    result_summary: "child a done".to_string(),
                    result_digest: "digest-child-a-done".to_string(),
                    metadata: Some(TaskCompletionMetadata {
                        lineage: None,
                        task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
                        timing: Some(ExecutionTimingMetadata {
                            started_mono_ms: Some(9),
                            finished_mono_ms: Some(60),
                            elapsed_ms: Some(51),
                        }),
                        hook_executions: Vec::new(),
                    }),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                12,
                EventActor::new(ActorKind::Worker, Some("agent_000102".to_string())),
                Some("req_000102"),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000302".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:mock:model-b".to_string()),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                13,
                EventActor::new(ActorKind::Worker, Some("agent_000102".to_string())),
                Some("req_000102"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000102".to_string(),
                    provider_id: "mock".to_string(),
                    model_id: "model-b".to_string(),
                    prompt_summary: "child b prompt".to_string(),
                    request_digest: "digest-child-b".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                14,
                EventActor::new(ActorKind::Worker, Some("agent_000102".to_string())),
                Some("req_000102"),
                EventV1::TaskCancelled(TaskCancelledEvent {
                    task_id: "task_000302".to_string(),
                    reason: "cancelled while running".to_string(),
                    task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                15,
                EventActor::new(ActorKind::Worker, Some("agent_000102".to_string())),
                Some("req_000102"),
                EventV1::TaskResultLate(TaskResultLateEvent {
                    task_id: "task_000302".to_string(),
                    result_digest: "digest-child-b-late".to_string(),
                }),
            ),
            resume_fixture_event(
                run_id,
                16,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "segment complete".to_string(),
                }),
            ),
        ],
    );

    let plan = inspect_resume_plan(&temp_dir.path().join(run_id));

    assert_eq!(
        plan.latest_lifecycle_status,
        LifecycleSegmentStatus::Finished,
        "replay should preserve final lifecycle status"
    );

    let child_a = plan
        .child_sessions
        .get("agent_000101")
        .expect("child A projection");
    assert_eq!(child_a.parent_session_id.as_deref(), Some("agent_000001"));
    assert_eq!(
        child_a.parent_tool_call_id.as_deref(),
        Some("toolcall_000201")
    );
    assert_eq!(
        child_a.latest_child_request_id.as_deref(),
        Some("req_000101")
    );
    assert_eq!(child_a.provider_id.as_deref(), Some("mock"));
    assert_eq!(child_a.model_id.as_deref(), Some("model-a"));
    assert_eq!(
        child_a.terminal_state,
        Some(ChildSessionTerminalState::Completed)
    );
    assert_eq!(
        child_a.timing.as_ref().and_then(|timing| timing.elapsed_ms),
        Some(51)
    );

    let child_b = plan
        .child_sessions
        .get("agent_000102")
        .expect("child B projection");
    assert_eq!(child_b.parent_session_id.as_deref(), Some("agent_000001"));
    assert_eq!(
        child_b.parent_tool_call_id.as_deref(),
        Some("toolcall_000202")
    );
    assert_eq!(
        child_b.latest_child_request_id.as_deref(),
        Some("req_000102")
    );
    assert_eq!(child_b.provider_id.as_deref(), Some("mock"));
    assert_eq!(child_b.model_id.as_deref(), Some("model-b"));
    assert_eq!(
        child_b.terminal_state,
        Some(ChildSessionTerminalState::LateResult)
    );
    assert_eq!(
        child_b.timing.as_ref().and_then(|timing| timing.elapsed_ms),
        Some(3),
        "late-result terminal timing should be derived from scheduled start"
    );
}

#[test]
fn replay_suppresses_hooks_but_preserves_hook_history() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_replay_hook_suppression";
    let side_effect_path = temp_dir.path().join("hook-side-effect.txt");
    let side_effect_digest = format!("digest:{}", side_effect_path.display());

    let hook_execution = HookExecutionMetadata {
        hook_name: "after_task".to_string(),
        status: HookExecutionStatus::Succeeded,
        hook_event: Some("task_completed".to_string()),
        command_digest: Some(side_effect_digest),
        output_digest: Some("hook-output-digest".to_string()),
        output_summary: Some("hook already executed live".to_string()),
        duration_ms: Some(12),
    };

    let lineage = TaskLineageMetadata {
        parent_tool_call_id: Some("toolcall_000401".to_string()),
        parent_task_id: Some("task_000401".to_string()),
        parent_request_id: Some("req_000401".to_string()),
        parent_session_id: Some("agent_000001".to_string()),
        child_session_id: Some("agent_000401".to_string()),
        child_request_id: Some("req_000401".to_string()),
        child_provider_id: Some("mock".to_string()),
        child_model_id: Some("model-hook".to_string()),
    };

    write_resume_fixture(
        temp_dir.path(),
        run_id,
        &[
            resume_fixture_event(
                run_id,
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            resume_fixture_event(
                run_id,
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "alpha".to_string(),
                    parent_agent_id: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                3,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000401".to_string(),
                    profile: "hook-runner".to_string(),
                    parent_agent_id: Some("agent_000001".to_string()),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                4,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000401"),
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000401".to_string(),
                    tool_id: "agent.spawn".to_string(),
                    args_summary: "{\"task\":\"run with hooks\"}".to_string(),
                    args_digest: "digest-hook-req".to_string(),
                    metadata: Some(ToolCallMetadata {
                        canonical_tool_id: Some("agent.spawn".to_string()),
                        alias_source_tool_id: None,
                        lineage: Some(lineage.clone()),
                        artifact_refs: Vec::new(),
                        timing: None,
                        hook_executions: vec![hook_execution.clone()],
                    }),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                5,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000401"),
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000401".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("hook already executed live".to_string()),
                    output_digest: Some("digest-hook-finish".to_string()),
                    output_json: Some(json!({
                        "hook_executions": [
                            {
                                "hook_name": "after_task",
                                "status": "succeeded",
                                "hook_event": "task_completed",
                                "command_digest": "hook-command-digest",
                                "output_digest": "hook-output-digest",
                                "output_summary": "hook already executed live",
                                "duration_ms": 12
                            }
                        ]
                    })),
                    metadata: Some(ToolCallMetadata {
                        canonical_tool_id: Some("agent.spawn".to_string()),
                        alias_source_tool_id: None,
                        lineage: Some(lineage.clone()),
                        artifact_refs: Vec::new(),
                        timing: Some(ExecutionTimingMetadata {
                            started_mono_ms: Some(4),
                            finished_mono_ms: Some(5),
                            elapsed_ms: Some(1),
                        }),
                        hook_executions: vec![hook_execution.clone()],
                    }),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                6,
                EventActor::new(ActorKind::Worker, Some("agent_000401".to_string())),
                Some("req_000401"),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000401".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:mock:model-hook".to_string()),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                7,
                EventActor::new(ActorKind::Worker, Some("agent_000401".to_string())),
                Some("req_000401"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000401".to_string(),
                    result_summary: "child done".to_string(),
                    result_digest: "digest-child-hook".to_string(),
                    metadata: Some(TaskCompletionMetadata {
                        lineage: Some(lineage),
                        task_scope: Some(harness_core::event::TaskTerminalScope::ToolCall),
                        timing: Some(ExecutionTimingMetadata {
                            started_mono_ms: Some(6),
                            finished_mono_ms: Some(7),
                            elapsed_ms: Some(1),
                        }),
                        hook_executions: vec![hook_execution.clone()],
                    }),
                }),
            ),
            resume_fixture_event(
                run_id,
                8,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "segment complete".to_string(),
                }),
            ),
        ],
    );

    let plan = inspect_resume_plan(&temp_dir.path().join(run_id));

    assert!(
        !side_effect_path.exists(),
        "replay must not execute historical hook side effects"
    );

    let tool_call_hooks = plan
        .tool_calls
        .get("toolcall_000401")
        .and_then(|snapshot| snapshot.metadata.as_ref())
        .map(|metadata| metadata.hook_executions.clone())
        .expect("projected tool-call hook metadata");
    assert_eq!(tool_call_hooks, vec![hook_execution.clone()]);

    let completed_task_hooks = plan
        .completed_tasks
        .get("task_000401")
        .and_then(|snapshot| snapshot.metadata.as_ref())
        .map(|metadata| metadata.hook_executions.clone())
        .expect("projected task hook metadata");
    assert_eq!(completed_task_hooks, vec![hook_execution.clone()]);

    let child = plan
        .child_sessions
        .get("agent_000401")
        .expect("projected child session for hook replay");
    assert_eq!(
        child.terminal_state,
        Some(ChildSessionTerminalState::Completed)
    );
    assert_eq!(child.hook_executions, vec![hook_execution]);
}

#[test]
fn replay_suppresses_hook_execution_but_preserves_hook_events() {
    replay_suppresses_hooks_but_preserves_hook_history();
}

#[tokio::test]
async fn hook_runner_is_suppressed_in_replay_and_deterministic_modes() {
    replay_suppresses_hooks_but_preserves_hook_history();
    deterministic_runs_suppress_live_hook_execution().await;
}

async fn deterministic_runs_suppress_live_hook_execution() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
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
        .expect("start run");

    let tool_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "true"}),
        )
        .await
        .expect("request tool call");

    tokio::time::sleep(Duration::from_millis(250)).await;
    coordinator.stop_run().await.expect("stop run");

    assert!(
        !hook_output_path.exists(),
        "deterministic suppression should prevent live hook side effects"
    );

    let events = load_events(&run.events_path);
    let tool_finished = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallFinished(data) if data.tool_call_id == tool_call_id => Some(data),
            _ => None,
        })
        .expect("tool finished event");
    let hook_executions = tool_finished
        .metadata
        .as_ref()
        .map(|metadata| metadata.hook_executions.clone())
        .expect("hook metadata on tool finish");
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

fn provider_text_events(text: &str) -> Vec<ProviderStreamEvent> {
    vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::TextDelta(text.to_string()),
        ProviderStreamEvent::Done {
            usage: CompletionUsage {
                prompt_tokens: 100,
                completion_tokens: 100,
                total_tokens: 200,
            },
        },
    ]
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
        .expect("compaction written event");
    let checkpoint_path = run.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(&checkpoint_path).expect("read checkpoint artifact");
    serde_json::from_str(&checkpoint_body).expect("parse checkpoint artifact")
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
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Deny,
        PermissionMode::Ask,
        PermissionMode::Deny,
    )
    .with_ask_timeout_ms(5_000);
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
                Some(data.task_id.clone())
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
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("timed out waiting for expected events")
}

fn write_resumable_history_fixture(session_dir: &Path, run_id: &str) {
    let events = vec![
        resume_fixture_event(
            run_id,
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".to_string(),
                workspace_root: "/workspace/project".to_string(),
            }),
        ),
        resume_fixture_event(
            run_id,
            2,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_000001".to_string(),
                profile: "alpha".to_string(),
                parent_agent_id: None,
            }),
        ),
        resume_fixture_event_with_actor_and_correlation(
            run_id,
            3,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("req_000001"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_000001".to_string(),
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
                request_id: "req_000001".to_string(),
                delta: "first answer".to_string(),
            }),
        ),
        resume_fixture_event_with_actor_and_correlation(
            run_id,
            5,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("req_000001"),
            EventV1::ProviderRequestFinished(harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_000001".to_string(),
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
                task_id: "task_000001".to_string(),
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
                run_name: "interactive".to_string(),
                workspace_root: "/workspace/project".to_string(),
            }),
        ),
        resume_fixture_event(
            run_id,
            2,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_000001".to_string(),
                profile: "alpha".to_string(),
                parent_agent_id: None,
            }),
        ),
        resume_fixture_event_with_actor_and_correlation(
            run_id,
            3,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("req_000001"),
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_000001".to_string(),
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
                request_id: "req_000001".to_string(),
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
                request_id: "req_000001".to_string(),
                delta: "calling tool".to_string(),
            }),
        ),
        resume_fixture_event_with_actor_and_correlation(
            run_id,
            6,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("req_000001"),
            EventV1::ProviderRequestFinished(harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_000001".to_string(),
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
                task_id: "task_000002".to_string(),
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
                task_id: "task_000002".to_string(),
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
                request_id: "req_000002".to_string(),
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
                request_id: "req_000002".to_string(),
                delta: "first final answer".to_string(),
            }),
        ),
        resume_fixture_event_with_actor_and_correlation(
            run_id,
            11,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("req_000002"),
            EventV1::ProviderRequestFinished(harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_000002".to_string(),
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
                task_id: "task_000001".to_string(),
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
    fs::create_dir_all(run_dir.join("artifacts")).expect("create fixture artifacts directory");

    let mut body = String::new();
    for event in events {
        let line = serde_json::to_string(event).expect("serialize fixture event");
        body.push_str(&line);
        body.push('\n');
    }

    let events_path = run_dir.join("events.jsonl");
    fs::write(&events_path, body).expect("write fixture events");
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
        run_id: run_id.to_string(),
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
        "alpha".to_string(),
        AgentProfile {
            name: "alpha".to_string(),
            category: "deep".to_string(),
            model_ref: "mock:model-1".to_string(),
            system_prompt: "alpha-prompt".to_string(),
            max_iters: Some(12),
            temperature: Some(0.0),
            tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
            toolset: vec![],
        },
    );
    profiles.insert(
        "beta".to_string(),
        AgentProfile {
            name: "beta".to_string(),
            category: "deep".to_string(),
            model_ref: "mock:model-1".to_string(),
            system_prompt: "beta-prompt".to_string(),
            max_iters: Some(12),
            temperature: Some(0.0),
            tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
            toolset: vec![],
        },
    );
    profiles
}

fn test_mock_provider() -> MockProvider {
    let mut scripted = BTreeMap::new();

    for prompt in ["alpha-prompt", "beta-prompt"] {
        let request = CompletionRequest {
            provider_id: Some("mock".to_string()),
            model_id: "model-1".to_string(),
            messages: vec![
                CompletionMessage {
                    role: MessageRole::System,
                    content: prompt.to_string(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: None,
                },
                CompletionMessage {
                    role: MessageRole::User,
                    content: prompt.to_string(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: None,
                },
            ],
            temperature: Some(0.0),
            max_tokens: None,
            variant: None,
            reasoning_effort: None,
            text_verbosity: None,
            reasoning_summary: None,
            tools: None,
            tool_choice: None,
            stream: true,
        };

        scripted.insert(
            request_digest(&request),
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::TextDelta(format!("{prompt}-delta")),
                ProviderStreamEvent::Done {
                    usage: CompletionUsage {
                        prompt_tokens: 2,
                        completion_tokens: 1,
                        total_tokens: 3,
                    },
                },
            ],
        );
    }

    MockProvider::new(scripted)
}

#[derive(Clone)]
struct SlowMockProvider {
    inner: MockProvider,
    delay: Duration,
}

#[async_trait]
impl Provider for SlowMockProvider {
    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream {
        let delay = self.delay;
        let stream = self
            .inner
            .stream_completion(req)
            .await
            .then(move |event| async move {
                tokio::time::sleep(delay).await;
                event
            });
        Box::pin(stream)
    }
}

#[derive(Clone)]
struct PromptScriptedProvider {
    scripts: BTreeMap<String, Vec<ProviderStreamEvent>>,
    delay: Duration,
}

impl PromptScriptedProvider {
    fn new(scripts: BTreeMap<String, Vec<ProviderStreamEvent>>, delay: Duration) -> Self {
        Self { scripts, delay }
    }
}

#[async_trait]
impl Provider for PromptScriptedProvider {
    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream {
        let prompt = req
            .messages
            .iter()
            .rev()
            .find(|message| message.role == MessageRole::User)
            .map(|message| message.content.clone())
            .unwrap_or_default();

        let events = self.scripts.get(&prompt).cloned().unwrap_or_else(|| {
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::TextDelta("ok".to_string()),
                ProviderStreamEvent::Done {
                    usage: CompletionUsage {
                        prompt_tokens: 1,
                        completion_tokens: 1,
                        total_tokens: 2,
                    },
                },
            ]
        });

        let delay = self.delay;
        let stream = tokio_stream::iter(events).then(move |event| async move {
            tokio::time::sleep(delay).await;
            event
        });
        Box::pin(stream)
    }
}
