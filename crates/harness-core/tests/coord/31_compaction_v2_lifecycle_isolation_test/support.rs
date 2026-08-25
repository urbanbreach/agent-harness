pub(super) async fn lifecycle_harness() -> (
    CompactionV2Harness,
    BlockingSummaryProvider,
    tokio::sync::oneshot::Receiver<()>,
    Arc<Notify>,
) {
    let (provider, entered, release) = BlockingSummaryProvider::new(
        vec![
            provider_text_events("lifecycle one"),
            provider_text_events("lifecycle two"),
            provider_text_events("released summary"),
            provider_text_events("other agent answer"),
        ],
        2,
    );
    let harness = CompactionV2Harness::with_provider(
        Arc::new(provider.clone()),
        CompactionRuntimeConfig::default(),
    )
    .await;
    harness.turn("lifecycle turn one").await;
    harness.turn("lifecycle turn two").await;
    (harness, provider, entered, release)
}

pub(super) async fn agent_turn(
    coordinator: &CoordinatorHandle,
    agent_id: &str,
    prompt: &str,
) -> String {
    let store = coordinator.event_store().await.unwrap_or_abort();
    let mut events = store.subscribe(1).unwrap_or_abort();
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, prompt)
        .await
        .unwrap_or_abort();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = events.next().await.unwrap_or_abort().unwrap_or_abort();
            if event.correlation_id.as_deref() == Some(request_id.as_str())
                && matches!(
                    event.payload,
                    EventV1::TaskCompleted(_) | EventV1::TaskCancelled(_)
                )
            {
                break;
            }
        }
    })
    .await
    .unwrap_or_abort();
    request_id
}

pub(super) fn spawn_compaction(
    harness: &CompactionV2Harness,
) -> tokio::task::JoinHandle<Result<ManualCompactionOutcome, CoordinatorError>> {
    let coordinator = harness.coordinator.clone();
    let agent_id = harness.agent_id.clone();
    tokio::spawn(async move {
        coordinator
            .compact_agent_context(agent_id, None, "manual")
            .await
    })
}
