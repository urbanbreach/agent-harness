use super::*;
use harness_core::UnwrapOrAbort;

#[tokio::test]
async fn compaction_v2_second_overflow_terminates() {
    // arrange
    // act
    // assert
    // Given: both the initial provider request and its sole retry overflow.
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let hook_counter_path = temp_dir.path().join("agent-turn-started.count");
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        provider_text_events(&"B".repeat(12_000)),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::error("first context overflow"),
        ],
        provider_text_events("overflow summary"),
        provider_text_events("overflow split prefix"),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::error("second context overflow"),
        ],
    ]);
    let hook_runtime = HookRuntimeConfig {
        hooks: HooksConfig {
            lifecycle: vec![LifecycleHookConfig {
                id: Some("count-agent-turn-start".to_string()),
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
    let coordinator = test_agent_coordinator_with_provider_compaction_and_hooks(
        temp_dir.path(),
        Arc::new(provider.clone()),
        2,
        CompactionRuntimeConfig::default(),
        hook_runtime,
    );
    let run = coordinator
        .start_run(
            "compaction-v2-retry-side-effects",
            temp_dir.path().to_path_buf(),
        )
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let harness = CompactionV2Harness {
        _temp_dir: temp_dir,
        coordinator,
        run,
        agent_id,
    };
    harness.turn("first terminal turn").await;
    harness.turn("second terminal turn").await;

    // When: the retry also overflows.
    let request_id = harness.turn("always overflowing turn").await;
    harness.stop().await;

    // Then: the second overflow is terminal and no third provider attempt exists.
    let events = harness.events();
    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        6,
        "history, overflow, split summaries, and one retry only"
    );
    let compaction = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::SessionCompaction(payload) => Some(payload),
            _ => None,
        })
        .unwrap_or_abort();
    assert!(compaction.summary.contains("overflow summary"));
    assert!(compaction.summary.contains("overflow split prefix"));
    assert_eq!(
        events
            .iter()
            .filter(|event| {
                matches!(event.payload, EventV1::ProviderRequestStarted(_))
                    && event.correlation_id.as_deref() == Some(request_id.as_str())
            })
            .count(),
        2,
        "second overflow must not dispatch a third main-provider request"
    );
    assert_eq!(session_compaction_values(&events).len(), 1);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.payload, EventV1::ToolCallFinished(_)))
            .count(),
        0,
        "overflow termination must not replay tools"
    );
    assert_eq!(
        fs::read_to_string(&hook_counter_path)
            .unwrap_or_abort()
            .lines()
            .count(),
        3,
        "one configured turn-start hook side effect per user turn"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| {
                event.correlation_id.as_deref() == Some(request_id.as_str())
                    && matches!(event.payload, EventV1::UserMessageSubmitted(_))
            })
            .count(),
        1,
        "overflow retry must not replay the original user submission"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.payload, EventV1::AssistantMessageFinished(_)))
            .count(),
        2,
        "failed attempts must not append semantic assistant memory"
    );
    assert!(events.iter().any(|event| {
        event.correlation_id.as_deref() == Some(request_id.as_str())
            && matches!(event.payload, EventV1::TaskCancelled(_))
    }));
}
