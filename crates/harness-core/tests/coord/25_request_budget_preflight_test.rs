use harness_core::config::{
    ModelLimitProvenance, ResolvedModelLimits, ResolvedModelTarget,
};
use harness_core::{model_resolution::ModelResolution, UnwrapOrAbort};

fn preflight_target(context: u32, input: u32, output: u32) -> ResolvedModelTarget {
    ResolvedModelTarget {
        model_ref: "mock:model-1".to_string(),
        provider: "mock".to_string(),
        model: "model-1".to_string(),
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        thinking: None,
        limits: ResolvedModelLimits::from_values(
            Some(context),
            Some(input),
            Some(output),
            ModelLimitProvenance::explicit("task 4 preflight test"),
        ),
        resolution: ModelResolution::default(),
        catalog_entry: None,
    }
}

#[tokio::test]
async fn pre_prompt_budget_rebuilds_request_before_dispatch() {
    // arrange: two large prior turns and a current prompt that pressures the known budget.
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let current_prompt = "C".repeat(12_000);
    let provider = BudgetObservingProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        provider_text_events(&"B".repeat(12_000)),
        provider_text_events("Compaction summary of earlier turns."),
        provider_text_events("rebuilt answer"),
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        1,
        CompactionRuntimeConfig {
            reserve_tokens: 1_000,
            keep_recent_tokens: 2_000,
            fallback_input_tokens: 8_000,
            ..CompactionRuntimeConfig::default()
        },
    );
    let target = preflight_target(9_000, 8_000, 2_000);
    let run = coordinator
        .start_run("budget_pre_prompt_rebuild", PathBuf::from("/workspace/project"))
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    for question in ["first question", "second question"] {
        let request_id = coordinator
            .request_agent_turn_with_model_target(
                supervisor_actor(),
                agent_id.clone(),
                question,
                target.clone(),
            )
            .await
            .unwrap_or_abort();
        wait_for_events(&run.events_path, Duration::from_secs(1), |events| {
            events.iter().any(|event| {
                matches!(
                    &event.payload,
                    EventV1::TaskCompleted(_)
                        if event.correlation_id.as_deref() == Some(request_id.as_str())
                )
            })
        })
        .await;
    }
    provider.clear_observations();

    // act: the pressured current turn runs pre-prompt compaction.
    let request_id = coordinator
        .request_agent_turn_with_model_target(
            supervisor_actor(),
            agent_id,
            &current_prompt,
            target,
        )
        .await
        .unwrap_or_abort();
    let events = wait_for_events(&run.events_path, Duration::from_secs(2), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.result_summary == "rebuilt answer"
            )
        })
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

    // assert: the provisional shape was costed, but only the compacted shape was dispatched/persisted.
    let distinct_digests = provider
        .observations()
        .into_iter()
        .map(|observation| observation.message_digest)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        distinct_digests.len(),
        2,
        "provisional and rebuilt shapes must both be costed"
    );

    let requests = provider.requests();
    assert_eq!(requests.len(), 4, "three turns plus one summary request");
    let transmitted = requests.last().unwrap_or_abort();
    assert_eq!(transmitted.max_tokens, Some(2_000));
    assert!(transmitted.messages.iter().any(|message| {
        message.role == MessageRole::Assistant
            && message.content.contains("Compaction summary of earlier turns")
    }));
    assert!(!transmitted.messages.iter().any(|message| {
        message.role == MessageRole::User && message.content == "first question"
    }));

    let started = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ProviderRequestStarted(started)
                if event.correlation_id.as_deref() == Some(request_id.as_str()) => Some(started),
            _ => None,
        })
        .unwrap_or_abort();
    let snapshot = started
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.context_budget)
        .unwrap_or_abort();
    assert_eq!(snapshot.reserved_output_tokens, transmitted.max_tokens);
    let transmitted_bytes = serde_json::to_vec(transmitted).unwrap_or_abort();
    let transmitted_digest = blake3::hash(&transmitted_bytes)
        .to_hex()
        .chars()
        .take(12)
        .collect::<String>();
    assert_eq!(started.request_digest, transmitted_digest);
}

#[tokio::test]
async fn oversized_prompt_or_tools_rejected_before_provider() {
    // arrange: a known tiny budget and a prompt too large to fit even with compaction disabled.
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = BudgetObservingProvider::new(vec![provider_text_events("must not run")]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        1,
        CompactionRuntimeConfig {
            enabled: false,
            reserve_tokens: 10,
            estimated_token_triggers: false,
            fallback_input_tokens: 0,
            ..CompactionRuntimeConfig::default()
        },
    );
    let run = coordinator
        .start_run("budget_preflight_rejection", PathBuf::from("/workspace/project"))
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();

    // act: the oversized request reaches normal-turn preflight.
    let request_id = coordinator
        .request_agent_turn_with_model_target(
            supervisor_actor(),
            agent_id,
            "X".repeat(8_000),
            preflight_target(1_000, 900, 100),
        )
        .await
        .unwrap_or_abort();
    let events = wait_for_events(&run.events_path, Duration::from_secs(1), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(_) | EventV1::TaskCompleted(_)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
            )
        })
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

    // assert: costing occurred, but neither provider dispatch nor request-start persistence did.
    assert!(!provider.observations().is_empty());
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCancelled(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str())
                    && data.reason.contains("request budget")
        )
    }));
    assert_eq!(provider.requests().len(), 0);
    assert!(events.iter().all(|event| {
        !matches!(
            &event.payload,
            EventV1::ProviderRequestStarted(_)
                if event.correlation_id.as_deref() == Some(request_id.as_str())
        )
    }));
}
