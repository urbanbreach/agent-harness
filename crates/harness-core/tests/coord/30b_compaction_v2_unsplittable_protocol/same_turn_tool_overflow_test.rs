#[tokio::test]
async fn compaction_v2_same_turn_tool_overflow_retries_without_replay() {
    // arrange
    // act
    // assert
    let hook_root = tempfile::tempdir().unwrap_or_abort();
    let hook_counter_path = hook_root.path().join("agent-turn-started.count");
    let hook_runtime = HookRuntimeConfig {
        hooks: HooksConfig {
            lifecycle: vec![LifecycleHookConfig {
                id: Some("same-turn-overflow-counter".to_string()),
                event: HookLifecycleEvent::AgentTurnStarted,
                command: vec![
                    "bash".to_string(),
                    "-lc".to_string(),
                    "printf 'started\\n' >> \"$HOOK_COUNTER_PATH\"".to_string(),
                ],
                cwd: Some(".".to_string()),
                timeout_ms: 4_000,
                critical: true,
                env: BTreeMap::from([(
                    "HOOK_COUNTER_PATH".to_string(),
                    hook_counter_path.display().to_string(),
                )]),
            }],
        },
        shell_allowlist: ShellAllowlist {
            executables: vec!["bash".to_string()],
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        },
        ..HookRuntimeConfig::default()
    };
    let (_temp, coordinator, run, agent_id, provider, tool_calls) = large_tool_harness(
        vec![
            provider_text_events(&"A".repeat(12_000)),
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::ToolCallComplete {
                    tool_call_id: "call_same_turn".to_string(),
                    function_name: "shell_run".to_string(),
                    arguments_json: "{}".to_string(),
                },
                ProviderStreamEvent::Done { usage: None },
            ],
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::error("same-turn continuation context overflow"),
            ],
            provider_text_events("same-turn bounded summary"),
            provider_text_events("same-turn bounded split prefix"),
            provider_text_events("same-turn retry answer"),
        ],
        hook_runtime,
    )
    .await;
    tool_turn(&coordinator, &agent_id, "old reducible history").await;

    let request_id = tool_turn(&coordinator, &agent_id, "tool then overflow in this turn").await;
    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);
    let correlated_starts = events
        .iter()
        .filter(|event| {
            event.correlation_id.as_deref() == Some(request_id.as_str())
                && matches!(event.payload, EventV1::ProviderRequestStarted(_))
        })
        .collect::<Vec<_>>();
    assert_eq!(correlated_starts.len(), 3, "tool call, overflow, one retry");
    let requests = provider.requests();
    assert_eq!(requests.len(), 6, "history, tool call, overflow, split summaries, retry");
    let compaction = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::SessionCompaction(payload) => Some(payload),
            _ => None,
        })
        .unwrap_or_abort();
    assert!(compaction.summary.contains("same-turn bounded summary"));
    assert!(compaction
        .summary
        .contains("same-turn bounded split prefix"));
    let retry = requests.last().unwrap_or_abort();
    let retry_start = correlated_starts[2];
    let committed_at_retry = events
        .iter()
        .filter(|event| event.seq <= retry_start.seq)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        normalize_provider_messages(&retry.messages),
        normalize_committed_messages(&committed_at_retry),
        "retry must reconstruct the durable user/call/result suffix exactly once"
    );
    let normalized = normalize_provider_messages(&retry.messages);
    assert_eq!(
        normalized
            .iter()
            .filter(|message| message.role == MessageRole::User
                && message.content == "tool then overflow in this turn")
            .count(),
        1,
        "the durable pending prompt must not be duplicated"
    );
    let pending_prompt_index = retry
        .messages
        .iter()
        .position(|message| {
            message.role == MessageRole::User
                && message.content == "tool then overflow in this turn"
        })
        .unwrap_or_abort();
    let semantics = harness_providers::generic_request_budget_semantics(
        retry,
        pending_prompt_index,
    )
    .unwrap_or_abort();
    let recorded_budget = match &retry_start.payload {
        EventV1::ProviderRequestStarted(started) => started
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.context_budget),
        _ => None,
    }
    .unwrap_or_abort();
    assert_eq!(
        recorded_budget.occupied_input_tokens,
        semantics.request_cost.total_input_tokens().unwrap_or_abort(),
        "the request budget must count the durable pending prompt exactly once"
    );
    assert_eq!(tool_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        events
            .iter()
            .filter(|event| {
                event.correlation_id.as_deref() == Some(request_id.as_str())
                    && matches!(event.payload, EventV1::UserMessageSubmitted(_))
            })
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| {
                event.correlation_id.as_deref() == Some(request_id.as_str())
                    && matches!(event.payload, EventV1::ToolCallFinished(_))
            })
            .count(),
        1
    );
    assert_eq!(
        fs::read_to_string(&hook_counter_path)
            .unwrap_or_abort()
            .lines()
            .count(),
        2,
        "one hook execution for each of the two user turns"
    );
    assert_eq!(session_compaction_values(&events).len(), 1);
}
