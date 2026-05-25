#[tokio::test]
async fn tool_results_project_in_assistant_source_order_after_out_of_order_completion() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let slow_started = Arc::new(Notify::new());
    let slow_release = Arc::new(Notify::new());
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "slow_call".to_string(),
                function_name: "shell_slow".to_string(),
                arguments_json: "{}".to_string(),
            },
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "fast_call".to_string(),
                function_name: "shell_fast".to_string(),
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
            ProviderStreamEvent::TextDelta("ordered final".to_string()),
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
        named_tool_registry(vec![
            NamedShellTool {
                id: "shell.slow",
                output: "slow output",
                started: Some(slow_started.clone()),
                release: Some(slow_release.clone()),
            },
            NamedShellTool {
                id: "shell.fast",
                output: "fast output",
                started: None,
                release: None,
            },
        ]),
        shell_only_permission_policy(),
        vec!["shell.slow".to_string(), "shell.fast".to_string()],
        12,
    );

    let run = coordinator
        .start_run(
            "coord_source_order_after_out_of_order_tools",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "call slow then fast")
        .await
        .expect("request agent turn");

    tokio::time::timeout(Duration::from_millis(500), slow_started.notified())
        .await
        .expect("slow tool should start");
    let fast_completed_before_slow_release =
        tokio::time::timeout(Duration::from_millis(150), async {
            loop {
                let events = load_events(&run.events_path);
                if events.iter().any(|event| {
                    matches!(
                        &event.payload,
                        EventV1::TaskCompleted(data) if data.result_summary == "fast output"
                    )
                }) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .is_ok();

    slow_release.notify_waiters();
    wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data) if data.result_summary == "ordered final"
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    assert!(
        fast_completed_before_slow_release,
        "fast tool should be allowed to complete before the earlier slow tool is released"
    );

    let events = load_events(&run.events_path);
    let chronological_tool_finishes = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::ToolCallFinished(data)
                if matches!(
                    data.output_summary.as_deref(),
                    Some("fast output" | "slow output")
                ) =>
            {
                data.output_summary.clone()
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        chronological_tool_finishes,
        vec!["fast output".to_string(), "slow output".to_string()],
        "JSONL lifecycle events should remain chronological by tool completion order"
    );

    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        2,
        "tool turn should continue once with tool outputs"
    );
    let tool_messages = requests[1]
        .messages
        .iter()
        .filter(|message| message.role == MessageRole::Tool)
        .map(|message| (message.tool_call_id.clone(), message.content.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        tool_messages,
        vec![
            (Some("slow_call".to_string()), "slow output".to_string()),
            (Some("fast_call".to_string()), "fast output".to_string()),
        ],
        "provider context must preserve model source order, not tool completion order"
    );
}
#[tokio::test]
async fn duplicate_provider_tool_call_ids_fail_before_tool_start() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::ToolCallComplete {
            tool_call_id: "dup_call".to_string(),
            function_name: "shell_run".to_string(),
            arguments_json: "{}".to_string(),
        },
        ProviderStreamEvent::ToolCallComplete {
            tool_call_id: "dup_call".to_string(),
            function_name: "shell_run".to_string(),
            arguments_json: "{}".to_string(),
        },
        ProviderStreamEvent::Done {
            usage: CompletionUsage {
                prompt_tokens: 2,
                completion_tokens: 1,
                total_tokens: 3,
            },
        },
    ]]);
    let coordinator = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(provider),
        test_tool_registry(),
        shell_only_permission_policy(),
        vec!["shell.run".to_string()],
        12,
    );

    let run = coordinator
        .start_run(
            "coord_duplicate_provider_tool_call_ids",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "duplicate tools")
        .await
        .expect("request agent turn");

    let events = wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            event.correlation_id.as_deref() == Some(request_id.as_str())
                && matches!(
                    &event.payload,
                    EventV1::TaskCancelled(_) | EventV1::TaskCompleted(_)
                )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCancelled(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str())
                    && data.reason.contains("duplicate")
                    && data.reason.contains("tool_call_id")
        )
    }));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.payload, EventV1::ToolCallStarted(_))),
        "duplicate provider tool ids must be rejected before any tool starts"
    );
}
#[tokio::test]
async fn empty_provider_tool_call_id_fails_before_tool_start() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::ToolCallComplete {
            tool_call_id: String::new(),
            function_name: "shell_run".to_string(),
            arguments_json: "{}".to_string(),
        },
        ProviderStreamEvent::Done {
            usage: CompletionUsage {
                prompt_tokens: 2,
                completion_tokens: 1,
                total_tokens: 3,
            },
        },
    ]]);
    let coordinator = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(provider),
        test_tool_registry(),
        shell_only_permission_policy(),
        vec!["shell.run".to_string()],
        12,
    );

    let run = coordinator
        .start_run(
            "coord_empty_provider_tool_call_id",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "empty tool id")
        .await
        .expect("request agent turn");

    let events = wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            event.correlation_id.as_deref() == Some(request_id.as_str())
                && matches!(
                    &event.payload,
                    EventV1::TaskCancelled(_) | EventV1::TaskCompleted(_)
                )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCancelled(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str())
                    && data.reason.contains("invalid")
                    && data.reason.contains("tool_call_id")
        )
    }));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.payload, EventV1::ToolCallStarted(_))),
        "empty provider tool ids must be rejected before any tool starts"
    );
}
#[tokio::test]
async fn denied_or_pending_tool_never_starts_before_permission_resolution() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(test_mock_provider()),
        test_tool_registry(),
        deny_all_permission_policy(),
        vec!["shell.run".to_string()],
        12,
    );

    let run = coordinator
        .start_run(
            "coord_denied_tool_no_start",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let error = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "true"}),
        )
        .await
        .expect_err("denied request should fail");
    let CoordinatorError::PermissionDenied(tool_call_id) = error else {
        panic!("expected permission denial");
    };
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallFinished(data)
                if data.tool_call_id == tool_call_id && data.status == ToolCallStatus::Failed
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id == tool_call_id
        )
    }));

    let pending_temp_dir = tempfile::tempdir().expect("pending tempdir");
    let pending_coordinator = test_agent_tool_coordinator(
        pending_temp_dir.path(),
        Arc::new(test_mock_provider()),
        test_tool_registry(),
        ask_shell_permission_policy(),
        vec!["shell.run".to_string()],
        12,
    );

    let pending_run = pending_coordinator
        .start_run(
            "coord_ask_pending_tool_no_start",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start pending run");
    let pending_tool_call_id = pending_coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "true"}),
        )
        .await
        .expect("ask request should be pending");

    let pending_events = wait_for_events(
        &pending_run.events_path,
        Duration::from_millis(500),
        |events| {
            events.iter().any(|event| {
                matches!(
                    &event.payload,
                    EventV1::PermissionRequested(data)
                        if data.tool_call_id.as_deref() == Some(pending_tool_call_id.as_str())
                )
            })
        },
    )
    .await;
    assert!(!pending_events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id == pending_tool_call_id
        )
    }));

    let pending_permission_id = pending_events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_deref() == Some(pending_tool_call_id.as_str()) =>
            {
                Some(data.permission_id.clone())
            }
            _ => None,
        })
        .expect("pending permission id");
    pending_coordinator
        .resolve_permission(
            pending_permission_id,
            RuntimePermissionDecision::Deny,
            Some("test cleanup".to_string()),
        )
        .await
        .expect("resolve pending permission");
    pending_coordinator
        .stop_run()
        .await
        .expect("stop pending run");
}
#[tokio::test]
async fn ask_pending_tool_call_never_emits_started_before_approval() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(test_mock_provider()),
        test_tool_registry(),
        ask_shell_permission_policy(),
        vec!["shell.run".to_string()],
        12,
    );

    let run = coordinator
        .start_run(
            "coord_ask_pending_tool_no_start",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let tool_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "true"}),
        )
        .await
        .expect("ask request should be pending");

    let events = wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::PermissionRequested(data)
                    if data.tool_call_id.as_deref() == Some(tool_call_id.as_str())
            )
        })
    })
    .await;
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id == tool_call_id
        )
    }));

    let permission_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_deref() == Some(tool_call_id.as_str()) =>
            {
                Some(data.permission_id.clone())
            }
            _ => None,
        })
        .expect("permission id");
    coordinator
        .resolve_permission(
            permission_id,
            RuntimePermissionDecision::Deny,
            Some("test cleanup".to_string()),
        )
        .await
        .expect("resolve pending permission");
    coordinator.stop_run().await.expect("stop run");
}
