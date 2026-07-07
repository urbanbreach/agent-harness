use harness_core::UnwrapOrAbort;
#[tokio::test]
async fn completed_tool_turn_preserves_tool_messages_for_followup_context() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "call_edit".to_string(),
                function_name: "shell_run".to_string(),
                arguments_json: r#"{"command":"touch docs/config.md"}"#.to_string(),
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
            ProviderStreamEvent::TextDelta("I edited docs/config.md.".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 3,
                    completion_tokens: 2,
                    total_tokens: 5,
                },
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("I used shell.run.".to_string()),
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
        test_tool_registry(),
        allow_all_permission_policy(),
        vec!["shell.run".to_string()],
        12,
    );

    let run = coordinator
        .start_run(
            "coord_completed_tool_turn_preserves_tool_messages",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();

    coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let first_request_id = coordinator
        .request_agent_turn(
            supervisor_actor(),
            "agent_000001",
            "edit docs/config.md",
        )
        .await
        .unwrap_or_abort();
    wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(first_request_id.as_str())
                        && data.result_summary == "I edited docs/config.md."
            )
        })
    })
    .await;

    coordinator
        .request_agent_turn(supervisor_actor(), "agent_000001", "what tool did you use?")
        .await
        .unwrap_or_abort();
    wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data) if data.result_summary == "I used shell.run."
            )
        })
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        3,
        "tool turn plus follow-up should make three provider calls"
    );
    let followup_messages = &requests[2].messages;
    let tool_call_message = followup_messages
        .iter()
        .find(|message| {
            message
                .assistant_tool_calls
                .as_ref()
                .is_some_and(|calls| calls.iter().any(|call| call.tool_call_id == "call_edit"))
        })
        .unwrap_or_abort();
    let calls = tool_call_message
        .assistant_tool_calls
        .as_ref()
        .unwrap_or_abort();
    assert_eq!(calls[0].function_name, "shell_run");
    assert_eq!(
        calls[0].arguments_json,
        r#"{"command":"touch docs/config.md"}"#
    );

    let tool_result_message = followup_messages
        .iter()
        .find(|message| {
            message.role == MessageRole::Tool
                && message.tool_call_id.as_deref() == Some("call_edit")
        })
        .unwrap_or_abort();
    assert_eq!(tool_result_message.name.as_deref(), Some("shell_run"));
    assert!(tool_result_message
        .content
        .contains("touch docs/config.md"));
    assert!(followup_messages.iter().any(|message| {
        message.role == MessageRole::Assistant
            && message.content == "I edited docs/config.md."
            && message.assistant_tool_calls.is_none()
    }));
}
#[tokio::test]
async fn resumed_tool_turn_preserves_tool_messages_for_followup_context() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let initial_provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "call_edit".to_string(),
                function_name: "shell_run".to_string(),
                arguments_json: r#"{"command":"touch docs/config.md"}"#.to_string(),
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
            ProviderStreamEvent::TextDelta("I edited docs/config.md.".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 3,
                    completion_tokens: 2,
                    total_tokens: 5,
                },
            },
        ],
    ]);
    let initial = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(initial_provider),
        test_tool_registry(),
        allow_all_permission_policy(),
        vec!["shell.run".to_string()],
        12,
    );

    let run = initial
        .start_run(
            "coord_resumed_tool_turn_preserves_tool_messages",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();
    initial
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let first_request_id = initial
        .request_agent_turn(
            supervisor_actor(),
            "agent_000001",
            "edit docs/config.md",
        )
        .await
        .unwrap_or_abort();
    wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(first_request_id.as_str())
                        && data.result_summary == "I edited docs/config.md."
            )
        })
    })
    .await;
    initial.stop_run().await.unwrap_or_abort();

    let resumed_provider = CapturingProvider::new(vec!["I used shell.run."]);
    let resumed =
        test_resume_coordinator_with_provider(temp_dir.path(), Arc::new(resumed_provider.clone()));
    resumed
        .resume_run(&run.run_id, "interactive")
        .await
        .unwrap_or_abort();
    resumed
        .request_agent_turn(supervisor_actor(), "agent_000001", "what tool did you use?")
        .await
        .unwrap_or_abort();
    tokio::task::yield_now().await;
    resumed.stop_run().await.unwrap_or_abort();

    let requests = resumed_provider.requests();
    assert_eq!(requests.len(), 1, "expected one resumed provider request");
    let followup_messages = &requests[0].messages;
    let tool_call_message = followup_messages
        .iter()
        .find(|message| {
            message
                .assistant_tool_calls
                .as_ref()
                .is_some_and(|calls| calls.iter().any(|call| call.function_name == "shell_run"))
        })
        .unwrap_or_abort();
    let calls = tool_call_message
        .assistant_tool_calls
        .as_ref()
        .unwrap_or_abort();
    assert_eq!(calls[0].function_name, "shell_run");
    assert_eq!(
        calls[0].arguments_json,
        r#"{"command":"touch docs/config.md"}"#
    );
    let reconstructed_tool_call_id = calls[0].tool_call_id.as_str();

    let tool_result_message = followup_messages
        .iter()
        .find(|message| {
            message.role == MessageRole::Tool
                && message.tool_call_id.as_deref() == Some(reconstructed_tool_call_id)
        })
        .unwrap_or_abort();
    assert_eq!(tool_result_message.name.as_deref(), Some("shell_run"));
    assert!(tool_result_message
        .content
        .contains("touch docs/config.md"));
}
#[tokio::test]
async fn provider_stream_metadata_persists_to_jsonl_events() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![vec![
        ProviderStreamEvent::Started {
            metadata: Some(ProviderStreamStartMetadata {
                provider_session_id: Some("session-observed-1".to_string()),
                provider_cache_id: Some("cache-observed-1".to_string()),
            }),
        },
        ProviderStreamEvent::ReasoningDelta("provider reasoning summary".to_string()),
        ProviderStreamEvent::TextDelta("metadata visible".to_string()),
        ProviderStreamEvent::DoneWithMetadata {
            usage: CompletionUsage {
                prompt_tokens: 12,
                completion_tokens: 4,
                total_tokens: 16,
            },
            metadata: Some(ProviderStreamFinishedMetadata {
                provider_response_id: Some("resp-observed-1".to_string()),
                provider_session_id: Some("session-observed-1".to_string()),
                provider_cache_id: Some("cache-observed-1".to_string()),
                provider_stop_reason: Some("stop".to_string()),
                cache_read_tokens: Some(7),
                cache_write_tokens: Some(3),
                assistant_message_id: Some("msg-observed-1".to_string()),
                thinking: Some(ProviderStreamThinkingMetadata {
                    summary: Some("provider supplied thinking summary".to_string()),
                    summary_digest: Some("thinking-digest-provider".to_string()),
                    signature: Some("thinking-signature-provider".to_string()),
                }),
            }),
        },
    ]]);
    let coordinator = test_agent_coordinator_with_provider(temp_dir.path(), Arc::new(provider), 1);

    let run = coordinator
        .start_run(
            "coord_provider_metadata_jsonl",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let turn_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "inspect provider metadata")
        .await
        .unwrap_or_abort();

    wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(turn_request_id.as_str())
                        && data.result_summary == "metadata visible"
            )
        })
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);
    let started = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ProviderRequestStarted(data)
                if event.correlation_id.as_deref() == Some(turn_request_id.as_str()) =>
            {
                Some(data)
            }
            _ => None,
        })
        .unwrap_or_abort();
    let started_metadata = started.metadata.as_ref().unwrap_or_abort();
    assert_eq!(
        started_metadata.turn_id.as_deref(),
        Some(turn_request_id.as_str())
    );
    assert_eq!(
        started_metadata.provider_call_id.as_deref(),
        Some(started.request_id.as_str())
    );
    assert_eq!(
        started_metadata.provider_session_id.as_deref(),
        Some("session-observed-1")
    );
    assert_eq!(
        started_metadata.provider_cache_id.as_deref(),
        Some("cache-observed-1")
    );

    let finished = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ProviderRequestFinished(data)
                if event.correlation_id.as_deref() == Some(turn_request_id.as_str()) =>
            {
                Some(data)
            }
            _ => None,
        })
        .unwrap_or_abort();
    let finished_metadata = finished.metadata.as_ref().unwrap_or_abort();
    assert_eq!(
        finished_metadata.turn_id.as_deref(),
        Some(turn_request_id.as_str())
    );
    assert_eq!(
        finished_metadata.provider_call_id.as_deref(),
        Some(started.request_id.as_str())
    );
    assert_eq!(
        finished_metadata.provider_response_id.as_deref(),
        Some("resp-observed-1")
    );
    assert_eq!(
        finished_metadata.provider_session_id.as_deref(),
        Some("session-observed-1")
    );
    assert_eq!(
        finished_metadata.provider_cache_id.as_deref(),
        Some("cache-observed-1")
    );
    assert_eq!(
        finished_metadata.provider_stop_reason.as_deref(),
        Some("stop")
    );
    assert_eq!(finished_metadata.cache_read_tokens, Some(7));
    assert_eq!(finished_metadata.cache_write_tokens, Some(3));
    let assistant_message = finished_metadata
        .assistant_message
        .as_ref()
        .unwrap_or_abort();
    assert_eq!(
        assistant_message.message_id.as_deref(),
        Some("msg-observed-1")
    );
    assert!(assistant_message.text_digest.is_some());
    assert!(assistant_message.reasoning_digest.is_some());
    let thinking = finished_metadata
        .thinking
        .as_ref()
        .unwrap_or_abort();
    assert_eq!(
        thinking.summary.as_deref(),
        Some("provider supplied thinking summary")
    );
    assert_eq!(
        thinking.summary_digest.as_deref(),
        Some("thinking-digest-provider")
    );
    assert_eq!(
        thinking.signature.as_deref(),
        Some("thinking-signature-provider")
    );
}
#[tokio::test]
async fn provider_reasoning_metadata_persists_digest_without_raw_summary_fallback() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::ReasoningDelta("private reasoning text".to_string()),
        ProviderStreamEvent::TextDelta("visible answer".to_string()),
        ProviderStreamEvent::Done {
            usage: CompletionUsage {
                prompt_tokens: 2,
                completion_tokens: 2,
                total_tokens: 4,
            },
        },
    ]]);
    let coordinator = test_agent_coordinator_with_provider(temp_dir.path(), Arc::new(provider), 1);

    let run = coordinator
        .start_run(
            "coord_provider_reasoning_digest_only",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let turn_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "inspect reasoning metadata")
        .await
        .unwrap_or_abort();

    wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(turn_request_id.as_str())
                        && data.result_summary == "visible answer"
            )
        })
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);
    let thinking = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ProviderRequestFinished(data)
                if event.correlation_id.as_deref() == Some(turn_request_id.as_str()) =>
            {
                data.metadata.as_ref()?.thinking.as_ref()
            }
            _ => None,
        })
        .unwrap_or_abort();

    assert_eq!(thinking.summary, None);
    assert!(thinking.summary_digest.is_some());
    assert_eq!(thinking.signature, None);
}
