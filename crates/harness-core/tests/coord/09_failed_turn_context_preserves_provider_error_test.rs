#[tokio::test]
async fn failed_turn_context_preserves_provider_error_partial_output() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("partial answer".to_string()),
            ProviderStreamEvent::Error {
                message: "provider exploded".to_string(),
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("follow-up answer".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 4,
                    completion_tokens: 2,
                    total_tokens: 6,
                },
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
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let failed_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "partial then error")
        .await
        .expect("request failing turn");
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
        .expect("request follow-up turn");
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
    coordinator.stop_run().await.expect("stop run");

    let requests = provider.requests();
    let follow_up = requests.last().expect("follow-up provider request");
    let assistant_marker = follow_up
        .messages
        .iter()
        .find(|message| {
            message.role == MessageRole::Assistant
                && message
                    .content
                    .contains("Harness preserved an incomplete provider turn")
        })
        .expect("failed turn marker should be sent before follow-up prompt");
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
    let temp_dir = tempfile::tempdir().expect("tempdir");
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
                usage: CompletionUsage {
                    prompt_tokens: 2,
                    completion_tokens: 1,
                    total_tokens: 3,
                },
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("after cancellation".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 4,
                    completion_tokens: 2,
                    total_tokens: 6,
                },
            },
        ],
    ]);
    let coordinator = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(provider.clone()),
        named_tool_registry(vec![NamedShellTool {
            id: "shell.block",
            output: "blocking output",
            started: Some(tool_started.clone()),
            release: Some(tool_release.clone()),
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
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let cancelled_request_id = coordinator
        .request_agent_turn(
            supervisor_actor(),
            agent_id.clone(),
            "cancel after assistant",
        )
        .await
        .expect("request cancellable turn");

    tokio::time::timeout(Duration::from_millis(500), tool_started.notified())
        .await
        .expect("blocking tool should start");
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
        .expect("agent task id");
    coordinator
        .cancel_task(task_id.clone(), "operator cancelled")
        .await
        .expect("cancel running agent turn");
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
        .expect("request follow-up turn");
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
    coordinator.stop_run().await.expect("stop run");

    let requests = provider.requests();
    let follow_up = requests.last().expect("follow-up provider request");
    let assistant_marker = follow_up
        .messages
        .iter()
        .find(|message| {
            message.role == MessageRole::Assistant
                && message
                    .content
                    .contains("Harness preserved an incomplete provider turn")
        })
        .expect("cancelled turn marker should be sent before follow-up prompt");
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
    let temp_dir = tempfile::tempdir().expect("tempdir");
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
                usage: CompletionUsage {
                    prompt_tokens: 2,
                    completion_tokens: 1,
                    total_tokens: 3,
                },
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("after tool failure".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 4,
                    completion_tokens: 2,
                    total_tokens: 6,
                },
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
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let failed_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "call failing tool")
        .await
        .expect("request failing tool turn");
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
        .expect("request follow-up turn");
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
    coordinator.stop_run().await.expect("stop run");

    let requests = provider.requests();
    let follow_up = requests.last().expect("follow-up provider request");
    let assistant_marker = follow_up
        .messages
        .iter()
        .find(|message| {
            message.role == MessageRole::Assistant
                && message
                    .content
                    .contains("Harness preserved an incomplete provider turn")
        })
        .expect("tool-failure marker should be sent before follow-up prompt");
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
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta(format!(
                "partial provider output {}",
                "B".repeat(12_000)
            )),
            ProviderStreamEvent::Error {
                message: "provider exploded".to_string(),
            },
        ],
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider),
        1,
        CompactionRuntimeConfig {
            fallback_input_tokens: 2_000,
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_failed_response_compaction_provider_error",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .expect("first turn");
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
        .expect("failing turn");
    let events = wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::CompactionWritten(data)
                    if data.trigger_reason == "failed_response"
                        && data.through_request_id.as_deref() == Some(failed_request_id.as_str())
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    let cancelled_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data)
                    if event.correlation_id.as_deref() == Some(failed_request_id.as_str())
                        && data.reason == "provider exploded"
            )
        })
        .expect("original provider failure cancellation");
    let requested_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::CompactionRequested(data)
                    if data.trigger_reason == "failed_response"
                        && data.through_request_id.as_deref() == Some(failed_request_id.as_str())
            )
        })
        .expect("failed-response compaction requested");
    assert!(
        cancelled_idx < requested_idx,
        "terminal TaskCancelled must be durable before failed-response compaction starts"
    );

    let checkpoint = checkpoint_for_trigger(&run, &events, "failed_response");
    let failed_turn = checkpoint
        .recent_turns
        .iter()
        .find(|turn| !turn.status.is_completed())
        .expect("failed provider turn remains provider-visible");
    assert_eq!(
        failed_turn.status,
        harness_core::agent::ProviderConversationTurnStatus::Failed
    );
    assert_eq!(failed_turn.failure_stage.as_deref(), Some("provider_error"));
    assert_eq!(
        failed_turn.failure_reason.as_deref(),
        Some("provider exploded")
    );
    assert!(failed_turn
        .assistant_response
        .contains("partial provider output"));
}
