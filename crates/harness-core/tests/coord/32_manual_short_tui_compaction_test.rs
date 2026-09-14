use harness_core::UnwrapOrAbort;

#[tokio::test]
async fn manual_short_tui_turns_remain_intact_within_recent_budget() {
    // arrange: the real TUI actor shape, default fallback budget, and two short completed turns.
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("Hello world".to_string()),
            ProviderStreamEvent::Done {
                usage: Some(CompletionUsage {
                    prompt_tokens: 4,
                    completion_tokens: 2,
                    total_tokens: 6,
                }),
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::ReasoningDelta("continuation reasoning".to_string()),
            ProviderStreamEvent::TextDelta("Hello again".to_string()),
            ProviderStreamEvent::Done {
                usage: Some(CompletionUsage {
                    prompt_tokens: 8,
                    completion_tokens: 3,
                    total_tokens: 11,
                }),
            },
        ],
        provider_text_events("short history summary"),
    ]);
    let coordinator =
        test_agent_coordinator_with_provider(temp_dir.path(), Arc::new(provider.clone()), 1);
    let run = coordinator
        .start_run("manual_short_tui", PathBuf::from("/workspace/project"))
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let tui_actor = EventActor::new(ActorKind::User, Some("interactive-user".to_string()));
    for prompt in ["hello", "hello"] {
        let request_id = coordinator
            .request_agent_turn(tui_actor.clone(), agent_id.clone(), prompt)
            .await
            .unwrap_or_abort();
        wait_for_events(&run.events_path, Duration::from_secs(1), |events| {
            events.iter().any(|event| matches!(
                &event.payload,
                EventV1::TaskCompleted(_)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
            ))
        })
        .await;
    }

    // act: the TUI issues `/compact` with no explicit through-request override.
    let outcome = coordinator
        .compact_agent_context(agent_id.clone(), None, "manual")
        .await
        .unwrap_or_abort();
    coordinator.stop_run().await.unwrap_or_abort();

    // Senpi keeps the complete history when every message fits the recent budget.
    assert_eq!(outcome, ManualCompactionOutcome::NoOp);
    assert_eq!(provider.requests().len(), 2);
    let events = load_events(&run.events_path);
    assert!(!events.iter().any(|event| matches!(event.payload, EventV1::SessionCompaction(_))));
    let preserved = harness_core::conversation::project_conversation(&events, &[]).unwrap_or_abort().messages;
    assert_eq!(preserved.iter().filter(|message| matches!(message, harness_core::conversation::ConversationMessage::User(user) if user.text == "hello")).count(), 2);
}
