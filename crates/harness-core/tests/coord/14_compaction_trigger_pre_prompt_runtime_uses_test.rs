use harness_core::UnwrapOrAbort;
#[tokio::test]
async fn compaction_trigger_pre_prompt_runtime_uses_checkpointed_prior_context() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let current_prompt = "C".repeat(12_000);
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        provider_text_events(&"B".repeat(12_000)),
        provider_text_events("Compaction summary of earlier turns."),
        provider_text_events("Preserved split-turn context."),
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
            "coord_pre_prompt_compaction_prior_context",
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

    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        5,
        "third turn should include main and split-turn compaction summary calls"
    );
    let third_messages = requests
        .last()
        .unwrap_or_abort()
        .messages
        .iter()
        .map(|message| (message.role.clone(), message.content.clone()))
        .collect::<Vec<_>>();
    assert!(third_messages.iter().any(|(role, content)| {
        *role == MessageRole::Assistant
            && content.contains("Compaction summary of earlier turns")
    }));
    assert!(third_messages
        .iter()
        .any(|(role, content)| { *role == MessageRole::User && content == &current_prompt }));
    assert!(!third_messages
        .iter()
        .any(|(role, content)| *role == MessageRole::User && content == "first question"));

    let events = load_events(&run.events_path);
    let compaction = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::SessionCompaction(payload) if payload.trigger_reason == "pre_prompt" => {
                Some(payload.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();
    assert!(compaction.tokens_before > 0);
}
#[tokio::test]
async fn compaction_no_loop_guards_cover_pre_prompt_overflow_and_failed_response() {
    // arrange
    // act
    // assert
    let pre_prompt_dir = tempfile::tempdir().unwrap_or_abort();
    let pre_prompt_current_prompt = "C".repeat(12_000);
    let pre_prompt_provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        provider_text_events(&"B".repeat(12_000)),
        provider_text_events("Compaction summary of earlier turns."),
        provider_text_events("Preserved split-turn context."),
        provider_text_events("third answer after pre-prompt no-shrink"),
    ]);
    let pre_prompt = test_agent_coordinator_with_provider_and_compaction(
        pre_prompt_dir.path(),
        Arc::new(pre_prompt_provider.clone()),
        1,
        CompactionRuntimeConfig {
            reserve_tokens: 4_096,
            fallback_input_tokens: 12_000,
            ..CompactionRuntimeConfig::default()
        },
    );
    let pre_prompt_run = pre_prompt
        .start_run(
            "coord_no_loop_pre_prompt",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();
    let pre_prompt_agent = pre_prompt
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    for question in ["first question", "second question"] {
        pre_prompt
            .request_agent_turn(supervisor_actor(), pre_prompt_agent.clone(), question)
            .await
            .unwrap_or_abort();
        tokio::task::yield_now().await;
    }
    let pre_prompt_request_id = pre_prompt
        .request_agent_turn(
            supervisor_actor(),
            pre_prompt_agent,
            &pre_prompt_current_prompt,
        )
        .await
        .unwrap_or_abort();
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
    pre_prompt.stop_run().await.unwrap_or_abort();
    let pre_prompt_attempt_count = pre_prompt_events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventV1::SessionCompaction(payload) if payload.trigger_reason == "pre_prompt"
            ) || matches!(
                &event.payload,
                EventV1::CompactionFailed(payload) if payload.trigger_reason == "pre_prompt"
            )
        })
        .count();
    assert_eq!(
        pre_prompt_attempt_count,
        1,
        "pre-prompt compaction should attempt at most once before provider execution"
    );
    assert_eq!(
        pre_prompt_provider.requests().len(),
        5,
        "pre-prompt compaction includes main and split-turn summary calls"
    );

    let overflow_dir = tempfile::tempdir().unwrap_or_abort();
    let overflow_provider = SequentialScriptedProvider::new(vec![
        provider_text_events("first answer"),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::error("prompt token count of 128713 exceeds the limit of 128000"),
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
        .unwrap_or_abort();
    let overflow_agent = overflow
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    overflow
        .request_agent_turn(supervisor_actor(), overflow_agent.clone(), "first question")
        .await
        .unwrap_or_abort();
    tokio::task::yield_now().await;
    let overflow_request_id = overflow
        .request_agent_turn(supervisor_actor(), overflow_agent, "second question")
        .await
        .unwrap_or_abort();
    let overflow_events = wait_for_events(&overflow_run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(payload)
                    if event.correlation_id.as_deref() == Some(overflow_request_id.as_str())
                        && payload.reason.contains("overflow compaction failed")
            )
        })
    })
    .await;
    overflow.stop_run().await.unwrap_or_abort();
    assert_eq!(
        overflow_events
            .iter()
            .filter(|event| matches!(
                &event.payload,
                EventV1::CompactionFailed(payload) if payload.trigger_reason == "overflow"
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

    let failed_dir = tempfile::tempdir().unwrap_or_abort();
    let failed_provider = SequentialScriptedProvider::new(vec![
        provider_text_events("first answer"),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta(format!(
                "partial provider output {}",
                "B".repeat(35_100)
            )),
            ProviderStreamEvent::error("provider exploded"),
        ],
    ]);
    let failed = test_agent_coordinator_with_provider_and_compaction(
        failed_dir.path(),
        Arc::new(failed_provider.clone()),
        1,
        CompactionRuntimeConfig {
            ..CompactionRuntimeConfig::default()
        },
    );
    let failed_run = failed
        .start_run(
            "coord_no_loop_failed_response",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();
    let failed_agent = failed
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    failed
        .request_agent_turn(supervisor_actor(), failed_agent.clone(), "first question")
        .await
        .unwrap_or_abort();
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
        .unwrap_or_abort();
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
        })
        },
    )
    .await;
    failed.stop_run().await.unwrap_or_abort();
    assert_eq!(
        failed_provider.requests().len(),
        2,
        "failed-response fragments are noncanonical and must not trigger summary or retry calls"
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
    // arrange
    // act
    // assert
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
        provider_text_events("Compaction summary of earlier turns."),
    ]);
    let coordinator =
        test_agent_coordinator_with_provider(temp_dir.path(), Arc::new(provider.clone()), 1);

    let run = coordinator
        .start_run(
            "coord_manual_compaction",
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
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "second question")
        .await
        .unwrap_or_abort();
    tokio::task::yield_now().await;

    let outcome = coordinator
        .compact_agent_context(agent_id, Some(second_request_id.clone()), "manual")
        .await
        .unwrap_or_abort();
    let ManualCompactionOutcome::Compacted {
        tokens_before,
        tokens_after,
        ..
    } = outcome
    else {
        panic!("expected compaction to apply");
    };
    assert!(tokens_after < tokens_before);

    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);
    let compaction = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::SessionCompaction(payload) if payload.trigger_reason == "manual" => {
                Some(payload.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();
    assert_eq!(compaction.agent_id, "agent_000001");
    assert_eq!(compaction.trigger_reason, "manual");
    assert_eq!(compaction.tokens_before, tokens_before);
}

#[tokio::test]
async fn manual_unknown_budget_remains_available_without_automatic_pressure() {
    // arrange: unknown model limits with conservative estimated triggers disabled.
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events("first answer"),
        provider_text_events("second answer"),
        provider_text_events("manual summary"),
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        1,
        CompactionRuntimeConfig {
            estimated_token_triggers: false,
            fallback_input_tokens: 0,
            ..CompactionRuntimeConfig::default()
        },
    );
    let run = coordinator
        .start_run(
            "manual_unknown_budget",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    for (prompt, answer) in [
        ("first question", "first answer"),
        ("second question", "second answer"),
    ] {
        let request_id = coordinator
            .request_agent_turn(supervisor_actor(), agent_id.clone(), prompt)
            .await
            .unwrap_or_abort();
        wait_for_events(&run.events_path, Duration::from_secs(1), |events| {
            events.iter().any(|event| {
                matches!(
                    &event.payload,
                    EventV1::TaskCompleted(payload)
                        if event.correlation_id.as_deref() == Some(request_id.as_str())
                            && payload.result_summary == answer
                )
            })
        })
        .await;
    }
    assert_eq!(provider.requests().len(), 2);
    assert!(!load_events(&run.events_path)
        .iter()
        .any(|event| matches!(event.payload, EventV1::SessionCompaction(_))));

    // act: the operator explicitly requests compaction.
    let outcome = coordinator
        .compact_agent_context(agent_id, None, "manual")
        .await
        .unwrap_or_abort();
    coordinator.stop_run().await.unwrap_or_abort();

    // assert: manual force semantics remain available without invented capacity.
    assert!(matches!(outcome, ManualCompactionOutcome::Compacted { .. }));
    assert_eq!(provider.requests().len(), 3);
}
