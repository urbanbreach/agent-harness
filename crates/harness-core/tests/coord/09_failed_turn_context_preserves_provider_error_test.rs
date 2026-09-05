use harness_core::UnwrapOrAbort;
#[tokio::test]
async fn failed_turn_context_preserves_provider_error_partial_output() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("partial answer".to_string()),
            ProviderStreamEvent::error("provider exploded"),
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("follow-up answer".to_string()),
            ProviderStreamEvent::Done {
                usage: Some(CompletionUsage {
                    prompt_tokens: 4,
                    completion_tokens: 2,
                    total_tokens: 6,
                }),
            },
        ],
    ]);
    let coordinator =
        test_agent_coordinator_with_provider(temp_dir.path(), Arc::new(provider.clone()), 1);

    let run = coordinator
        .start_run(
            "coord_failed_context_provider_error",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let failed_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "partial then error")
        .await
        .unwrap_or_abort();
    wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data)
                    if event.correlation_id.as_deref() == Some(failed_request_id.as_str())
                        && data.reason == "provider exploded"
            )
        })
    })
    .await;

    let follow_up_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "continue after failure")
        .await
        .unwrap_or_abort();
    wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(follow_up_request_id.as_str())
                        && data.result_summary == "follow-up answer"
            )
        })
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

    let requests = provider.requests();
    let follow_up = requests.last().unwrap_or_abort();
    let assistant_marker = follow_up
        .messages
        .iter()
        .find(|message| {
            message.role == MessageRole::Assistant
                && message
                    .content
                    .contains("Harness preserved an incomplete provider turn")
        })
        .unwrap_or_abort();
    assert!(assistant_marker.content.contains("Status: failed"));
    assert!(assistant_marker.content.contains("Stage: provider_error"));
    assert!(assistant_marker
        .content
        .contains("Reason: provider exploded"));
    assert!(assistant_marker.content.contains("partial answer"));
    assert!(follow_up.messages.iter().any(|message| {
        message.role == MessageRole::User && message.content == "partial then error"
    }));
    assert!(follow_up.messages.iter().any(|message| {
        message.role == MessageRole::User && message.content == "continue after failure"
    }));
}
#[tokio::test]
async fn failed_turn_context_preserves_cancelled_turn_marker() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let tool_started = Arc::new(Notify::new());
    let tool_release = Arc::new(Notify::new());
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("partial before cancellation".to_string()),
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "blocking_tool".to_string(),
                function_name: "shell_block".to_string(),
                arguments_json: "{}".to_string(),
            },
            ProviderStreamEvent::Done {
                usage: Some(CompletionUsage {
                    prompt_tokens: 2,
                    completion_tokens: 1,
                    total_tokens: 3,
                }),
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("after cancellation".to_string()),
            ProviderStreamEvent::Done {
                usage: Some(CompletionUsage {
                    prompt_tokens: 4,
                    completion_tokens: 2,
                    total_tokens: 6,
                }),
            },
        ],
    ]);
    let coordinator = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(provider.clone()),
        named_tool_registry(vec![NamedShellTool {
            id: "shell.block",
            output: "blocking output",
            started: Some(Arc::clone(&tool_started)),
            release: Some(Arc::clone(&tool_release)),
        }]),
        shell_only_permission_policy(),
        vec!["shell.block".to_string()],
        12,
    );

    let run = coordinator
        .start_run(
            "coord_failed_context_cancelled_marker",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let cancelled_request_id = coordinator
        .request_agent_turn(
            supervisor_actor(),
            agent_id.clone(),
            "cancel after assistant",
        )
        .await
        .unwrap_or_abort();

    tokio::time::timeout(Duration::from_millis(500), tool_started.notified())
        .await
        .unwrap_or_abort();
    let events = load_events(&run.events_path);
    let task_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(cancelled_request_id.as_str())
                    && data
                        .queue_key
                        .as_deref()
                        .is_some_and(|queue_key| queue_key.starts_with("provider_model:")) =>
            {
                Some(data.task_id.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();
    coordinator
        .cancel_task(task_id.clone(), "operator cancelled")
        .await
        .unwrap_or_abort();
    tool_release.notify_waiters();
    wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data)
                    if data.task_id == task_id && data.reason == "operator cancelled"
            )
        })
    })
    .await;
    tokio::task::yield_now().await;

    let follow_up_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "continue after cancellation")
        .await
        .unwrap_or_abort();
    wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(follow_up_request_id.as_str())
                        && data.result_summary == "after cancellation"
            )
        })
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

    let requests = provider.requests();
    let follow_up = requests.last().unwrap_or_abort();
    let assistant_marker = follow_up
        .messages
        .iter()
        .find(|message| {
            message.role == MessageRole::Assistant
                && message
                    .content
                    .contains("Harness preserved an incomplete provider turn")
        })
        .unwrap_or_abort();
    assert!(assistant_marker.content.contains("Status: aborted"));
    assert!(assistant_marker.content.contains("Stage: cancelled"));
    assert!(assistant_marker
        .content
        .contains("Reason: operator cancelled"));
    assert!(assistant_marker
        .content
        .contains("partial before cancellation"));
}
#[tokio::test]
async fn failed_turn_context_preserves_tool_failure_without_orphan_tool_call() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("plain text before tool failure".to_string()),
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "failing_tool".to_string(),
                function_name: "shell_fail".to_string(),
                arguments_json: "{}".to_string(),
            },
            ProviderStreamEvent::Done {
                usage: Some(CompletionUsage {
                    prompt_tokens: 2,
                    completion_tokens: 1,
                    total_tokens: 3,
                }),
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("after tool failure".to_string()),
            ProviderStreamEvent::Done {
                usage: Some(CompletionUsage {
                    prompt_tokens: 4,
                    completion_tokens: 2,
                    total_tokens: 6,
                }),
            },
        ],
    ]);
    let coordinator = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(provider.clone()),
        Arc::new({
            let mut registry = ToolRegistry::new();
            registry.register(Arc::new(FailingShellTool));
            registry
        }),
        shell_only_permission_policy(),
        vec!["shell.fail".to_string()],
        12,
    );

    let run = coordinator
        .start_run(
            "coord_failed_context_tool_failure",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let failed_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "call failing tool")
        .await
        .unwrap_or_abort();
    wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data)
                    if event.correlation_id.as_deref() == Some(failed_request_id.as_str())
                        && data.reason.contains("tool call `shell_fail` failed closed")
            )
        })
    })
    .await;

    let follow_up_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "continue after tool failure")
        .await
        .unwrap_or_abort();
    wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(follow_up_request_id.as_str())
                        && data.result_summary == "after tool failure"
            )
        })
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

    let requests = provider.requests();
    let follow_up = requests.last().unwrap_or_abort();
    let assistant_marker = follow_up
        .messages
        .iter()
        .find(|message| {
            message.role == MessageRole::Assistant
                && message
                    .content
                    .contains("Harness preserved an incomplete provider turn")
        })
        .unwrap_or_abort();
    assert!(assistant_marker.content.contains("Status: failed"));
    assert!(assistant_marker.content.contains("Stage: tool_failure"));
    assert!(assistant_marker
        .content
        .contains("plain text before tool failure"));
    assert!(!assistant_marker.content.contains("failing_tool"));
    assert!(assistant_marker.assistant_tool_calls.is_none());
    assert!(!follow_up
        .messages
        .iter()
        .any(|message| message.role == MessageRole::Tool));
}
#[tokio::test]
async fn failed_response_compaction_writes_checkpoint_after_provider_error() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta(format!(
                "partial provider output {}",
                "B".repeat(12_000)
            )),
            ProviderStreamEvent::error("provider exploded"),
        ],
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider),
        1,
        CompactionRuntimeConfig {
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_failed_response_compaction_provider_error",
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
    wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data) if data.result_summary == "A".repeat(12_000)
            )
        })
    })
    .await;

    let failed_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "partial then error")
        .await
        .unwrap_or_abort();
    let events = wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data)
                    if event.correlation_id.as_deref() == Some(failed_request_id.as_str())
                        && data.reason == "provider exploded"
            )
        })
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCancelled(data)
                if event.correlation_id.as_deref() == Some(failed_request_id.as_str())
                    && data.reason == "provider exploded"
        )
    }));
}
