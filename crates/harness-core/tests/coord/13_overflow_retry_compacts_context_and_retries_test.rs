use harness_core::UnwrapOrAbort;
use std::time::Duration;
#[tokio::test]
async fn overflow_retry_compacts_context_and_retries_with_summary() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("A".repeat(12_000)),
            ProviderStreamEvent::Done {
                usage: Some(CompletionUsage {
                    prompt_tokens: 100,
                    completion_tokens: 100,
                    total_tokens: 200,
                }),
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("B".repeat(12_000)),
            ProviderStreamEvent::Done {
                usage: Some(CompletionUsage {
                    prompt_tokens: 100,
                    completion_tokens: 100,
                    total_tokens: 200,
                }),
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::error("prompt token count of 128713 exceeds the limit of 128000"),
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("Compaction summary of earlier turns.".to_string()),
            ProviderStreamEvent::Done {
                usage: None,
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("recovered answer".to_string()),
            ProviderStreamEvent::Done {
                usage: Some(CompletionUsage {
                    prompt_tokens: 64,
                    completion_tokens: 8,
                    total_tokens: 72,
                }),
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
        .unwrap_or_abort();

    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .unwrap_or_abort();
    tokio::task::yield_now().await;
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "second question")
        .await
        .unwrap_or_abort();
    tokio::task::yield_now().await;
    let third_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "third question")
        .await
        .unwrap_or_abort();
    tokio::task::yield_now().await;
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if provider.requests().len() >= 5 {
                break;
            }
            if load_events(&run.events_path)
                .iter()
                .any(|e| matches!(e.payload, EventV1::SessionCompaction(_)))
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_abort();
    coordinator.stop_run().await.unwrap_or_abort();

    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        5,
        "third turn should retry once after compaction (3 turns + 1 summary + 1 retry)"
    );
    let retried_messages = requests
        .last()
        .unwrap_or_abort()
        .messages
        .iter()
        .map(|message| (message.role.clone(), message.content.clone()))
        .collect::<Vec<_>>();
    assert!(retried_messages.iter().any(|(role, content)| {
        *role == MessageRole::Assistant
            && content.contains("Compaction summary of earlier turns")
    }));
    assert!(retried_messages
        .iter()
        .any(|(role, content)| { *role == MessageRole::User && content == "third question" }));

    let events = load_events(&run.events_path);
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, EventV1::SessionCompaction(_))));
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
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("A".repeat(12_000)),
            ProviderStreamEvent::Done {
                usage: Some(CompletionUsage {
                    prompt_tokens: 100,
                    completion_tokens: 100,
                    total_tokens: 200,
                }),
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::error("prompt token count of 128713 exceeds the limit of 128000"),
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("Compaction summary of earlier turns.".to_string()),
            ProviderStreamEvent::Done {
                usage: None,
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("recovered answer".to_string()),
            ProviderStreamEvent::Done {
                usage: Some(CompletionUsage {
                    prompt_tokens: 64,
                    completion_tokens: 8,
                    total_tokens: 72,
                }),
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
        .unwrap_or_abort();

    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .unwrap_or_abort();
    tokio::task::yield_now().await;
    let second_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "second question")
        .await
        .unwrap_or_abort();
    tokio::task::yield_now().await;
    coordinator.stop_run().await.unwrap_or_abort();

    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        4,
        "single preserved turn should still retry once after summary-only compaction (2 turns + 1 summary + 1 retry)"
    );
    let retried_messages = requests
        .last()
        .unwrap_or_abort()
        .messages
        .iter()
        .map(|message| (message.role.clone(), message.content.clone()))
        .collect::<Vec<_>>();
    assert!(retried_messages.iter().any(|(role, content)| {
        *role == MessageRole::Assistant
            && content.contains("Compaction summary of earlier turns")
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
        .any(|event| matches!(event.payload, EventV1::SessionCompaction(_))));
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
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("first answer".to_string()),
            ProviderStreamEvent::Done {
                usage: Some(CompletionUsage {
                    prompt_tokens: 32,
                    completion_tokens: 8,
                    total_tokens: 40,
                }),
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::error("prompt token count of 128713 exceeds the limit of 128000"),
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
        .unwrap_or_abort();

    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .unwrap_or_abort();
    tokio::task::yield_now().await;
    let second_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "second question")
        .await
        .unwrap_or_abort();
    tokio::task::yield_now().await;
    coordinator.stop_run().await.unwrap_or_abort();

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
                    && payload.trigger_reason == "overflow"
        )
    }));
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
async fn compaction_trigger_pre_prompt_occurs_before_provider_request_started() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let current_prompt = "C".repeat(12_000);
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        provider_text_events(&"B".repeat(12_000)),
        provider_text_events("Compaction summary of earlier turns."),
        provider_text_events("third answer"),
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        1,
        CompactionRuntimeConfig {
            reserve_tokens: 4_096,
            fallback_input_tokens: 12_000,
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_pre_prompt_compaction_order",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .unwrap_or_abort();
    tokio::task::yield_now().await;
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "second question")
        .await
        .unwrap_or_abort();
    tokio::task::yield_now().await;
    let third_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, &current_prompt)
        .await
        .unwrap_or_abort();
    tokio::task::yield_now().await;
    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);
    let pre_prompt_written_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::SessionCompaction(payload) if payload.trigger_reason == "pre_prompt"
            )
        })
        .unwrap_or_abort();
    let provider_started_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ProviderRequestStarted(_) if event.correlation_id.as_deref() == Some(third_request_id.as_str())
            )
        })
        .unwrap_or_abort();
    assert!(
        pre_prompt_written_idx < provider_started_idx,
        "pre-prompt compaction must be written before the third provider request is constructed"
    );
}
#[tokio::test]
async fn compaction_trigger_pre_prompt_attempts_once_per_turn() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let current_prompt = "C".repeat(12_000);
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        provider_text_events(&"B".repeat(12_000)),
        provider_text_events("Compaction summary of earlier turns."),
        provider_text_events("third answer"),
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        1,
        CompactionRuntimeConfig {
            reserve_tokens: 4_096,
            fallback_input_tokens: 12_000,
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_pre_prompt_compaction_attempts_once",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    for question in ["first question", "second question"] {
        coordinator
            .request_agent_turn(supervisor_actor(), agent_id.clone(), question)
            .await
            .unwrap_or_abort();
        tokio::task::yield_now().await;
    }
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id, &current_prompt)
        .await
        .unwrap_or_abort();
    tokio::task::yield_now().await;
    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);
    let pre_prompt_writes = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventV1::SessionCompaction(payload) if payload.trigger_reason == "pre_prompt"
            )
        })
        .count();
    assert_eq!(
        pre_prompt_writes, 1,
        "pre-prompt compaction should write at most one SessionCompaction for a turn"
    );
    assert_eq!(
        provider.requests().len(),
        4,
        "provider execution should continue once with the uncompacted context (3 turns + 1 summary)"
    );
}
