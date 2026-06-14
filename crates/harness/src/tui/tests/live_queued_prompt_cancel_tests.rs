use harness_core::event::{EventEnvelopeV1, TaskScheduleState};
use harness_providers::{CompletionRequest, Provider, ProviderEventStream, ProviderStreamEvent};
use tokio_stream::StreamExt;

use super::*;

struct PendingStartProvider;

#[async_trait::async_trait]
impl Provider for PendingStartProvider {
    async fn stream_completion(&self, _req: CompletionRequest) -> ProviderEventStream {
        Box::pin(tokio_stream::once(ProviderStreamEvent::Start).chain(tokio_stream::pending()))
    }
}

#[test]
fn live_ui_router_forwards_queued_prompt_cancel_without_switching_workflow() {
    // Given
    let (intent_tx, mut intent_rx) = mpsc::unbounded_channel::<UiIntent>();
    let launch_selection = Arc::new(Mutex::new(LaunchMetadata::default()));
    let (selected_workflow, sink) =
        build_live_ui_intent_router(intent_tx, Arc::clone(&launch_selection), false);

    // When
    sink(UiIntent::CancelQueuedPrompt {
        task_id: "task_queued".to_string(),
    });

    // Then
    assert!(recover_mutex_lock(&selected_workflow).is_none());
    assert_eq!(
        intent_rx.try_recv().ok(),
        Some(UiIntent::CancelQueuedPrompt {
            task_id: "task_queued".to_string(),
        })
    );
}

#[tokio::test]
async fn queued_prompt_cancel_intent_cancels_coordinator_queued_task() {
    // Given
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
    config.deterministic_store = true;
    config.agent_profiles = golden_path_profiles();
    config.provider = Arc::new(PendingStartProvider);
    config.provider_model_concurrency = 1;

    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator
        .start_run("tui_queued_prompt_cancel", temp_dir.path())
        .await
        .expect("start run");
    let store = coordinator.event_store().await.expect("event store");
    let mut events = store.subscribe(1).expect("subscribe events");
    let agent = coordinator
        .spawn_agent_idle(supervisor_actor(), "planner", None)
        .await
        .expect("spawn idle agent");

    let _active_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent.clone(), "active turn")
        .await
        .expect("request active turn");
    let queued_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent, "queued prompt")
        .await
        .expect("request queued turn");
    let queued_event = wait_for_matching_event(&mut events, |event| {
        matches!(
            &event.payload,
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(queued_request_id.as_str())
                    && data.state == TaskScheduleState::Queued
        )
    })
    .await;
    let queued_task_id = match queued_event.payload {
        EventV1::TaskScheduled(data) => data.task_id,
        _ => unreachable!("matcher only accepts queued TaskScheduled"),
    };
    let (intent_tx, intent_rx) = mpsc::unbounded_channel();
    let (notice_tx, _notice_rx) = std_mpsc::channel();
    let handle = tokio::spawn(handle_ui_intents(
        coordinator.clone(),
        intent_rx,
        user_actor(),
        None,
        notice_tx,
        TuiAuthBackendContext {
            config_path: None,
            session_dir: Some(temp_dir.path().to_path_buf()),
            workspace_root: temp_dir.path().to_path_buf(),
        },
    ));

    // When
    intent_tx
        .send(UiIntent::CancelQueuedPrompt {
            task_id: queued_task_id.clone(),
        })
        .expect("send queued cancel intent");
    drop(intent_tx);
    handle
        .await
        .expect("ui intent task join")
        .expect("ui intent task ok");

    // Then
    let cancelled = wait_for_matching_event(&mut events, |event| {
        matches!(
            &event.payload,
            EventV1::TaskCancelled(data)
                if data.task_id == queued_task_id
                    && data.reason == "queued prompt removed before scheduling"
        )
    })
    .await;
    assert_eq!(
        cancelled.correlation_id.as_deref(),
        Some(queued_request_id.as_str())
    );

    coordinator.stop_run().await.expect("stop run");
    let persisted_events = load_events_from_run_dir(&run.run_dir).expect("load run events");
    assert!(!persisted_events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ProviderRequestStarted(_)
                if event.correlation_id.as_deref() == Some(queued_request_id.as_str())
        )
    }));
}

async fn wait_for_matching_event(
    events: &mut harness_core::store::EventStream,
    mut matches_event: impl FnMut(&EventEnvelopeV1) -> bool,
) -> EventEnvelopeV1 {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = events
                .next()
                .await
                .expect("event stream open")
                .expect("event");
            if matches_event(&event) {
                return event;
            }
        }
    })
    .await
    .expect("matching event within timeout")
}
