use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use harness_core::agent::AgentProfile;
use harness_core::clock::FakeClock;
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, TaskScheduleState,
};
use harness_core::redact::DefaultRedactor;
use harness_providers::mock::{request_digest, MockProvider};
use harness_providers::{
    CompletionMessage, CompletionRequest, CompletionUsage, MessageRole, Provider,
    ProviderEventStream, ProviderStreamEvent,
};
use tokio_stream::StreamExt;

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

fn test_coordinator(session_dir: &Path) -> CoordinatorHandle {
    let mut config = CoordinatorConfig::new(session_dir.to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 32;
    let clock = Arc::new(FakeClock::new());
    let redactor = Arc::new(DefaultRedactor::default());
    spawn_coordinator(config, clock, redactor)
}

fn test_agent_coordinator(session_dir: &Path, delay: Duration) -> CoordinatorHandle {
    let mut config = CoordinatorConfig::new(session_dir.to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.provider_model_concurrency = 1;
    config.provider = Arc::new(SlowMockProvider {
        inner: test_mock_provider(),
        delay,
    });
    config.agent_profiles = agent_profiles();

    let clock = Arc::new(FakeClock::new());
    let redactor = Arc::new(DefaultRedactor::default());
    spawn_coordinator(config, clock, redactor)
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
                },
                CompletionMessage {
                    role: MessageRole::User,
                    content: prompt.to_string(),
                    name: None,
                    tool_call_id: None,
                },
            ],
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

    assert_eq!(started.len(), 1);
    assert!(queued.is_empty());

    let (started_idx, started_event, started_data) = *started[0];
    assert_eq!(started_event.actor.kind, ActorKind::Worker);
    assert_eq!(started_event.actor.agent_id.as_deref(), Some(agent_id.as_str()));

    let provider_started_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ProviderRequestStarted(data) if data.request_id == request_id
            )
        })
        .expect("provider request started event");
    assert!(started_idx < provider_started_idx);
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

    assert_eq!(scheduled.len(), 2);
    assert_eq!(scheduled[0].2.state, TaskScheduleState::Queued);
    assert_eq!(scheduled[1].2.state, TaskScheduleState::Started);
    assert_eq!(scheduled[0].2.task_id, scheduled[1].2.task_id);

    for (_, event, _) in &scheduled {
        assert_eq!(event.actor.kind, ActorKind::Worker);
        assert_eq!(event.actor.agent_id.as_deref(), Some(beta.as_str()));
    }
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
        .filter(|event| matches!(&event.payload, EventV1::TaskCancelled(data) if data.task_id == task_id))
        .collect::<Vec<_>>();
    assert_eq!(cancellations.len(), 1);
    assert_task_event_context(
        cancellations[0],
        &EventActor::new(ActorKind::Worker, Some(beta)),
        &queued_request_id,
    );
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
                if event.correlation_id.as_deref() == Some(request_id.as_str()) => Some(data.task_id),
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
            matches!(&event.payload, EventV1::TaskCancelled(data) if data.task_id == task_id)
                || matches!(&event.payload, EventV1::TaskCompleted(data) if data.task_id == task_id)
        })
        .collect::<Vec<_>>();
    assert_eq!(terminal_events.len(), 1);
    assert_task_event_context(
        terminal_events[0],
        &EventActor::new(ActorKind::Worker, Some(agent_id)),
        &request_id,
    );
}

fn assert_task_event_context(
    event: &EventEnvelopeV1,
    expected_actor: &EventActor,
    expected_correlation: &str,
) {
    assert_eq!(&event.actor, expected_actor);
    assert_eq!(event.correlation_id.as_deref(), Some(expected_correlation));
}

fn supervisor_actor() -> EventActor {
    EventActor::new(ActorKind::Supervisor, Some("agent_supervisor".to_string()))
}

fn load_events(events_path: &Path) -> Vec<EventEnvelopeV1> {
    let body = fs::read_to_string(events_path).expect("read events file");
    body.lines()
        .map(|line| serde_json::from_str::<EventEnvelopeV1>(line).expect("parse event jsonl line"))
        .collect()
}
