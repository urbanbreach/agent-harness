use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use harness_core::store::{EventStoreError, EventStoreOpener, JsonlFileEventStore};

#[derive(Default)]
struct CountingEventStoreOpener {
    open_count: AtomicUsize,
    open_existing_count: AtomicUsize,
}

impl CountingEventStoreOpener {
    fn counts(&self) -> (usize, usize) {
        (
            self.open_count.load(Ordering::SeqCst),
            self.open_existing_count.load(Ordering::SeqCst),
        )
    }
}

impl EventStoreOpener for CountingEventStoreOpener {
    fn open(
        &self,
        session_dir: &Path,
        run_id: &str,
        deterministic: bool,
    ) -> Result<JsonlFileEventStore, EventStoreError> {
        self.open_count.fetch_add(1, Ordering::SeqCst);
        JsonlFileEventStore::open(session_dir, run_id, deterministic)
    }

    fn open_existing(
        &self,
        session_dir: &Path,
        run_id: &str,
        deterministic: bool,
    ) -> Result<JsonlFileEventStore, EventStoreError> {
        self.open_existing_count.fetch_add(1, Ordering::SeqCst);
        JsonlFileEventStore::open_existing(session_dir, run_id, deterministic)
    }
}

fn coordinator_with_counting_opener(
    session_dir: &Path,
    opener: Arc<CountingEventStoreOpener>,
) -> CoordinatorHandle {
    let mut config = CoordinatorConfig::new(session_dir);
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.tool_registry = test_tool_registry();
    config.provider = Arc::new(test_mock_provider());
    config.agent_profiles = agent_profiles();
    config.event_store_opener = opener;
    spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    )
}

#[tokio::test]
async fn event_store_opener_counts_root_start_exactly_once() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let opener = Arc::new(CountingEventStoreOpener::default());
    let coordinator =
        coordinator_with_counting_opener(temp_dir.path(), Arc::clone(&opener));

    coordinator
        .start_run("root opener", temp_dir.path())
        .await
        .unwrap_or_abort();

    assert_eq!(opener.counts(), (1, 0));
    coordinator.stop_run().await.unwrap_or_abort();
}

#[tokio::test]
async fn event_store_opener_counts_root_and_child_creation_exactly() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let opener = Arc::new(CountingEventStoreOpener::default());
    let coordinator =
        coordinator_with_counting_opener(temp_dir.path(), Arc::clone(&opener));

    coordinator
        .start_run("child opener", temp_dir.path())
        .await
        .unwrap_or_abort();
    let parent_agent_id = coordinator
        .spawn_agent(supervisor_actor(), "default", None)
        .await
        .unwrap_or_abort();
    coordinator
        .spawn_agent_idle(
            supervisor_actor(),
            "default",
            Some(parent_agent_id),
        )
        .await
        .unwrap_or_abort();

    assert_eq!(opener.counts(), (2, 0));
    coordinator.stop_run().await.unwrap_or_abort();
}

#[tokio::test]
async fn event_store_opener_counts_root_resume_exactly_once() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_event_store_opener_resume";
    write_resume_fixture(
        temp_dir.path(),
        run_id,
        &[
            resume_fixture_event(
                run_id,
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "resume opener".into(),
                    workspace_root: temp_dir.path().display().to_string(),
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
            resume_fixture_event(
                run_id,
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".into(),
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
                    summary: "fixture segment complete".to_string(),
                }),
            ),
        ],
    );
    let opener = Arc::new(CountingEventStoreOpener::default());
    let coordinator =
        coordinator_with_counting_opener(temp_dir.path(), Arc::clone(&opener));

    let resumed = coordinator.resume_run(run_id, "resume opener").await;
    assert!(resumed.is_ok(), "resume failed: {resumed:?}");

    assert_eq!(opener.counts(), (0, 1));
    coordinator.stop_run().await.unwrap_or_abort();
}
