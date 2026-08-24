use harness_core::UnwrapOrAbort;

async fn run_semantic_script(
    provider_events: Vec<ProviderStreamEvent>,
) -> Vec<EventEnvelopeV1> {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![provider_events]);
    let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.provider_model_concurrency = 1;
    config.provider = Arc::new(provider);
    config.agent_profiles = agent_profiles();
    config.provider_retry = harness_core::config::ProviderRetryRuntimeConfig {
        max_retries: 0,
        base_delay_ms: 0,
        max_delay_ms: 0,
    };
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator
        .start_run(
            "semantic-history",
            PathBuf::from("/workspace/semantic-history"),
        )
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let store = coordinator.event_store().await.unwrap_or_abort();
    let mut stream = store.subscribe(1).unwrap_or_abort();
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "semantic history")
        .await
        .unwrap_or_abort();

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = stream
                .next()
                .await
                .unwrap_or_abort()
                .unwrap_or_abort();
            let terminal = event.correlation_id.as_deref() == Some(request_id.as_str())
                && matches!(
                    event.payload,
                    EventV1::TaskCompleted(_) | EventV1::TaskCancelled(_)
                );
            if terminal {
                break;
            }
        }
    })
    .await
    .unwrap_or_abort();

    coordinator.stop_run().await.unwrap_or_abort();
    load_events(&run.events_path)
}

fn successful_semantic_events(chunks: &[&str]) -> Vec<ProviderStreamEvent> {
    let mut events = vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::ReasoningDelta("reasoned".to_string()),
    ];
    events.extend(
        chunks
            .iter()
            .map(|chunk| ProviderStreamEvent::TextDelta((*chunk).to_string())),
    );
    events.push(ProviderStreamEvent::Done {
        usage: Some(CompletionUsage {
            prompt_tokens: 7,
            completion_tokens: 3,
            total_tokens: 10,
        }),
    });
    events
}

fn assistant_text_without_deltas(events: &[EventEnvelopeV1]) -> String {
    let durable = events
        .iter()
        .filter(|event| {
            !matches!(
                event.payload,
                EventV1::ProviderStreamDelta(_) | EventV1::ProviderReasoningDelta(_)
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    let projection =
        harness_core::conversation::project_conversation(&durable, &[]).unwrap_or_abort();
    projection
        .messages
        .iter()
        .find_map(|message| match message {
            harness_core::conversation::ConversationMessage::Assistant(assistant) => {
                Some(assistant.text.clone())
            }
            _ => None,
        })
        .unwrap_or_default()
}

#[tokio::test]
async fn semantic_history_commits_final_sanitized_facts_without_durable_deltas() {
    // arrange
    let events = run_semantic_script(successful_semantic_events(&["final answer"])).await;

    // act
    let delta_count = events
        .iter()
        .filter(|event| {
            matches!(
                event.payload,
                EventV1::ProviderStreamDelta(_) | EventV1::ProviderReasoningDelta(_)
            )
        })
        .count();
    let final_commit = events.iter().find(|event| {
        matches!(
            event.payload,
            EventV1::AssistantMessageFinished(_)
        )
    });
    let serialized_commit = final_commit
        .map(|event| serde_json::to_value(event).unwrap_or_abort())
        .unwrap_or(serde_json::Value::Null);
    let parts = serialized_commit
        .pointer("/payload/data/parts")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();

    // assert
    assert_eq!(delta_count, 0, "provider fragments must not be durable");
    assert!(
        !parts.is_empty(),
        "assistant completion must carry final sanitized semantic parts"
    );
}

#[tokio::test]
async fn provider_chunk_boundaries_produce_identical_durable_history() {
    // arrange
    let one_chunk = run_semantic_script(successful_semantic_events(&["hello world"])).await;
    let many_chunks = run_semantic_script(successful_semantic_events(&["hello ", "world"])).await;

    // assert: compare complete envelopes, including ordering and metadata.
    assert_eq!(
        one_chunk, many_chunks,
        "provider chunk boundaries must not change any durable envelope"
    );
}

#[tokio::test]
async fn lost_live_deltas_are_settled_by_final_commit() {
    // arrange
    let events = run_semantic_script(successful_semantic_events(&["settled answer"])).await;

    // act
    let restored = assistant_text_without_deltas(&events);

    // assert
    assert_eq!(
        restored, "settled answer",
        "final commit must restore content when live fragments are unavailable"
    );
}

#[tokio::test]
async fn interrupted_fragments_remain_noncanonical() {
    // arrange
    let events = run_semantic_script(vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::ReasoningDelta("partial thought".to_string()),
        ProviderStreamEvent::TextDelta("partial answer".to_string()),
        ProviderStreamEvent::Error {
            message: "provider interrupted".to_string(),
            category: Some(ProviderErrorCategory::TransportFailure),
            remediation: None,
            retry_after_ms: None,
        },
    ])
    .await;

    // act
    let fragments = events
        .iter()
        .filter(|event| {
            matches!(
                event.payload,
                EventV1::ProviderStreamDelta(_) | EventV1::ProviderReasoningDelta(_)
            )
        })
        .count();
    let completed = events
        .iter()
        .any(|event| matches!(event.payload, EventV1::AssistantMessageFinished(_)));

    // assert
    assert_eq!(
        fragments, 0,
        "interrupted transport fragments must remain live-only"
    );
    assert!(!completed, "interrupted requests must not synthesize a commit");
}

#[tokio::test]
async fn runtime_subscription_delivers_live_deltas_without_replay() {
    // arrange: subscribe to the typed runtime surface before triggering the provider.
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![successful_semantic_events(&[
        "live answer",
    ])]);
    let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.provider_model_concurrency = 1;
    config.provider = Arc::new(provider);
    config.agent_profiles = agent_profiles();
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator
        .start_run(
            "semantic-history-runtime",
            PathBuf::from("/workspace/semantic-history-runtime"),
        )
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let store = coordinator.event_store().await.unwrap_or_abort();
    let mut runtime = store.subscribe_runtime(1).unwrap_or_abort();

    // act
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "semantic history runtime")
        .await
        .unwrap_or_abort();
    let mut live_text = String::new();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match runtime.next().await.unwrap_or_abort().unwrap_or_abort() {
                harness_core::event::RuntimeEvent::Live(event) => {
                    if let harness_core::event::LiveEventV1::ProviderTextDelta { delta, .. } =
                        &event.payload
                    {
                        live_text.push_str(delta);
                    }
                }
                harness_core::event::RuntimeEvent::Durable(event)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && matches!(event.payload, EventV1::TaskCompleted(_)) =>
                {
                    break;
                }
                harness_core::event::RuntimeEvent::Durable(_) => {}
            }
        }
    })
    .await
    .unwrap_or_abort();
    coordinator.stop_run().await.unwrap_or_abort();
    let events = load_events(&run.events_path);

    // assert
    assert_eq!(live_text, "live answer");
    assert!(!events.iter().any(|event| matches!(
        event.payload,
        EventV1::ProviderStreamDelta(_) | EventV1::ProviderReasoningDelta(_)
    )));
    assert_eq!(assistant_text_without_deltas(&events), "live answer");
}
