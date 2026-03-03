use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use harness_core::agent::AgentProfile;
use harness_core::clock::FakeClock;
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle};
use harness_core::event::{ActorKind, EventActor, EventEnvelopeV1, EventV1};
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
async fn coord_spawn_agent_idle_appends_agent_spawned_but_no_provider_events() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_agent_coordinator(temp_dir.path(), Duration::from_millis(5));

    let run = coordinator
        .start_run("coord_spawn_idle", PathBuf::from("/workspace/project"))
        .await
        .expect("start run");

    let actor = EventActor::new(ActorKind::Supervisor, Some("agent_supervisor".to_string()));
    let _agent_id = coordinator
        .spawn_agent_idle(actor, "alpha", None)
        .await
        .expect("spawn idle agent");

    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    assert!(
        events
            .iter()
            .any(|event| matches!(event.payload, EventV1::AgentSpawned(_))),
        "expected AgentSpawned event"
    );
    assert!(
        !events.iter().any(|event| {
            matches!(
                event.payload,
                EventV1::ProviderRequestStarted(_)
                    | EventV1::ProviderStreamDelta(_)
                    | EventV1::ProviderRequestFinished(_)
            )
        }),
        "idle spawn must not schedule provider work"
    );
}

#[tokio::test]
async fn coord_request_agent_turn_appends_user_message_and_provider_events() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_agent_coordinator(temp_dir.path(), Duration::from_millis(5));

    let run = coordinator
        .start_run("coord_request_turn", PathBuf::from("/workspace/project"))
        .await
        .expect("start run");

    let supervisor = EventActor::new(ActorKind::Supervisor, Some("agent_supervisor".to_string()));
    let agent_id = coordinator
        .spawn_agent_idle(supervisor.clone(), "alpha", None)
        .await
        .expect("spawn idle alpha");

    let request_id = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, Some("agent_supervisor".to_string())),
            agent_id.clone(),
            "alpha-prompt",
        )
        .await
        .expect("request agent turn");

    tokio::time::sleep(Duration::from_millis(200)).await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let stream_key = format!("agent:{agent_id}");

    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::UserMessageSubmitted(data)
                if data.agent_id == agent_id
                    && data.content == "alpha-prompt"
                    && event.correlation_id.as_deref() == Some(request_id.as_str())
                    && event.stream_key.as_deref() == Some(stream_key.as_str())
        )
    }));

    let provider_related: Vec<_> = events
        .iter()
        .filter(|event| match &event.payload {
            EventV1::ProviderRequestStarted(data) => data.request_id == request_id,
            EventV1::ProviderStreamDelta(data) => data.request_id == request_id,
            EventV1::ProviderRequestFinished(data) => data.request_id == request_id,
            _ => false,
        })
        .collect();

    assert!(
        !provider_related.is_empty(),
        "expected provider events for requested turn"
    );
    assert!(provider_related.iter().all(
        |event| event.correlation_id.as_deref() == Some(request_id.as_str())
    ));
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
                },
                CompletionMessage {
                    role: MessageRole::User,
                    content: prompt.to_string(),
                },
            ],
            temperature: Some(0.0),
            max_tokens: None,
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

fn load_events(events_path: &Path) -> Vec<EventEnvelopeV1> {
    let body = fs::read_to_string(events_path).expect("read events file");
    body.lines()
        .map(|line| serde_json::from_str::<EventEnvelopeV1>(line).expect("parse event jsonl line"))
        .collect()
}
