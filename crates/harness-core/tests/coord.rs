use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use harness_core::agent::AgentProfile;
use harness_core::clock::FakeClock;
use harness_core::config::{
    HookLifecycleEvent, HookRuntimeConfig, HooksConfig, LifecycleHookConfig, PermissionMode,
    ShellAllowlist,
};
use harness_core::coord::{
    spawn_coordinator, CoordinatorConfig, CoordinatorError, CoordinatorHandle, JobOutcome,
    JobProgressKind,
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
use harness_core::tool::ToolSurface;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult};
use harness_providers::mock::{request_digest, MockProvider};
use harness_providers::{
    CompletionMessage, CompletionRequest, CompletionUsage, MessageRole, Provider,
    ProviderEventStream, ProviderStreamEvent,
};
use serde_json::json;
use tokio::sync::Notify;
use tokio_stream::StreamExt;

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
    let coordinator = test_coordinator(temp_dir.path());

    let run = coordinator
        .start_run("coord_spawn", PathBuf::from("/workspace/project"))
        .await
        .expect("start run");

    let actor = EventActor::new(ActorKind::Supervisor, Some("agent_supervisor".to_string()));
    let _agent_id = coordinator
        .spawn_agent(actor, "worker", None)
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
    let coordinator = test_coordinator(temp_dir.path());

    let run = coordinator
        .start_run("coord_policy", PathBuf::from("/workspace/project"))
        .await
        .expect("start run");

    let actor = EventActor::new(ActorKind::Worker, Some("agent_worker".to_string()));
    let result = coordinator.spawn_agent(actor, "worker", None).await;
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
                EventV1::ProviderRequestStarted(data) if data.request_id == request_id
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
                EventV1::ProviderRequestStarted(data) if data.request_id == queued_request_id
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
    let mut request_ids = Vec::new();

    for event in &events {
        if let EventV1::ProviderRequestStarted(ref data) = event.payload {
            request_ids.push(data.request_id.clone());
        }
    }

    request_ids.sort();
    request_ids.dedup();
    assert_eq!(
        request_ids.len(),
        2,
        "expected one request_id per spawned agent"
    );

    for request_id in request_ids {
        let related: Vec<_> = events
            .iter()
            .filter(|event| match &event.payload {
                EventV1::ProviderRequestStarted(data) => data.request_id == request_id,
                EventV1::ProviderStreamDelta(data) => data.request_id == request_id,
                EventV1::ProviderRequestFinished(data) => data.request_id == request_id,
                _ => false,
            })
            .collect();

        assert!(
            !related.is_empty(),
            "request {} should have related provider events",
            request_id
        );
        assert!(related
            .iter()
            .all(|event| event.correlation_id.as_deref() == Some(request_id.as_str())));
    }
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
                cwd: Some(".".to_string()),
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
    assert_eq!(second_request_id, "req_000003");
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
                    profile: "explore".to_string(),
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
    let mut config = CoordinatorConfig::new(session_dir.to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.provider_model_concurrency = provider_model_concurrency;
    config.provider = provider;
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
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_000001".to_string(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "first question".to_string(),
                request_digest: "digest-req-1".to_string(),
            }),
        ),
        resume_fixture_event_with_actor_and_correlation(
            run_id,
            4,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("req_000001"),
            EventV1::ProviderStreamDelta(harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_000001".to_string(),
                delta: "calling tool".to_string(),
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
            }),
        ),
        resume_fixture_event_with_actor_and_correlation(
            run_id,
            6,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("req_000002"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_000002".to_string(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "tool result + continue".to_string(),
                request_digest: "digest-req-2".to_string(),
            }),
        ),
        resume_fixture_event_with_actor_and_correlation(
            run_id,
            7,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("req_000002"),
            EventV1::ProviderStreamDelta(harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_000002".to_string(),
                delta: "first final answer".to_string(),
            }),
        ),
        resume_fixture_event_with_actor_and_correlation(
            run_id,
            8,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("req_000002"),
            EventV1::ProviderRequestFinished(harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_000002".to_string(),
                finish_reason: "done".to_string(),
                output_digest: Some("digest-out-2".to_string()),
            }),
        ),
        resume_fixture_event_with_actor_and_correlation(
            run_id,
            9,
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
            10,
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
            max_iters: 12,
            temperature: Some(0.0),
            tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
            tool_surface: ToolSurface::Native,
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
            max_iters: 12,
            temperature: Some(0.0),
            tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
            tool_surface: ToolSurface::Native,
            toolset: vec![],
        },
    );
    profiles
}

fn test_mock_provider() -> MockProvider {
    let mut scripted = BTreeMap::new();

    for prompt in ["alpha-prompt", "beta-prompt"] {
        let request = CompletionRequest {
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
            max_iters: 12,
            temperature: Some(0.0),
            max_tokens: None,
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

fn load_events(events_path: &Path) -> Vec<EventEnvelopeV1> {
    let body = fs::read_to_string(events_path).expect("read events file");
    body.lines()
        .map(|line| serde_json::from_str::<EventEnvelopeV1>(line).expect("parse event jsonl line"))
        .collect()
}
