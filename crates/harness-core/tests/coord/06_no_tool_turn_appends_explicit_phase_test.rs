#[tokio::test]
async fn no_tool_turn_appends_explicit_phase_barriers_in_order() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_agent_coordinator_with_provider(
        temp_dir.path(),
        Arc::new(CapturingProvider::new(vec!["phase complete"])),
        1,
    );

    let run = coordinator
        .start_run(
            "coord_no_tool_explicit_phase_order",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "phase order prompt")
        .await
        .expect("request agent turn");

    wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.result_summary == "phase complete"
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let scheduled_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::TaskScheduled(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.state == TaskScheduleState::Started
                        && data
                            .queue_key
                            .as_deref()
                            .is_some_and(|queue_key| queue_key.starts_with("provider_model:"))
            )
        })
        .expect("agent turn scheduled barrier");
    let provider_started_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ProviderRequestStarted(_)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
            )
        })
        .expect("provider start barrier");
    let provider_delta_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ProviderStreamDelta(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.delta == "phase complete"
            )
        })
        .expect("provider text delta");
    let provider_finished_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ProviderRequestFinished(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.finish_reason == "done"
            )
        })
        .expect("provider finish barrier");
    let assistant_message_finished_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::AssistantMessageFinished(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.tool_call_count == 0
            )
        })
        .expect("assistant message end barrier");
    let turn_completed_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.result_summary == "phase complete"
            )
        })
        .expect("turn end barrier");

    assert!(scheduled_idx < provider_started_idx);
    assert!(provider_started_idx < provider_delta_idx);
    assert!(provider_delta_idx < provider_finished_idx);
    assert!(provider_finished_idx < assistant_message_finished_idx);
    assert!(assistant_message_finished_idx < turn_completed_idx);
    assert!(!events.iter().any(|event| {
        event.correlation_id.as_deref() == Some(request_id.as_str())
            && matches!(event.payload, EventV1::ToolCallStarted(_))
    }));
}
#[tokio::test]
async fn tool_turn_does_not_preflight_until_assistant_message_end_is_durable() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("calling tool".to_string()),
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "phase_tool".to_string(),
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
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("tool phase done".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 3,
                    completion_tokens: 2,
                    total_tokens: 5,
                },
            },
        ],
    ]);
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
            "coord_tool_waits_for_assistant_barrier",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "tool phase barrier")
        .await
        .expect("request agent turn");

    wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.result_summary == "tool phase done"
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    let provider_started = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::ProviderRequestStarted(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str()) =>
            {
                Some(data.request_id.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(provider_started.len(), 2, "tool turn should continue once");

    let first_provider_finished_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ProviderRequestFinished(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.request_id == provider_started[0]
            )
        })
        .expect("provider finish barrier for first provider call");
    let assistant_message_finished_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::AssistantMessageFinished(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.request_id == provider_started[0]
                        && data.tool_call_count == 1
            )
        })
        .expect("assistant message end barrier for first provider call");
    let tool_requested_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallRequested(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.tool_id == "shell.run"
            )
        })
        .expect("tool preflight requested");
    let tool_started_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallStarted(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && !data.tool_call_id.is_empty()
            )
        })
        .expect("tool started");
    let tool_finished_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallFinished(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.status == ToolCallStatus::Succeeded
            )
        })
        .expect("tool result barrier");
    let second_provider_started_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ProviderRequestStarted(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.request_id == provider_started[1]
            )
        })
        .expect("follow-up provider start");

    assert!(first_provider_finished_idx < assistant_message_finished_idx);
    assert!(assistant_message_finished_idx < tool_requested_idx);
    assert!(assistant_message_finished_idx < tool_started_idx);
    assert!(tool_finished_idx < second_provider_started_idx);
}
#[tokio::test]
async fn queued_turn_recomputes_context_at_provider_start() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = DelayedCapturingProvider::new(
        vec!["first answer", "beta answer", "second answer"],
        Duration::from_millis(50),
    );
    let coordinator =
        test_agent_coordinator_with_provider(temp_dir.path(), Arc::new(provider.clone()), 1);

    let run = coordinator
        .start_run(
            "coord_queued_turn_recomputes_context",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let beta_agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "beta", None)
        .await
        .expect("spawn idle beta");

    let first_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .expect("first turn");
    wait_for_events(&run.events_path, Duration::from_secs(1), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(first_request_id.as_str())
                        && data.result_summary == "first answer"
            )
        })
    })
    .await;

    let beta_request_id = coordinator
        .request_agent_turn(supervisor_actor(), beta_agent_id, "beta question")
        .await
        .expect("beta turn holding provider slot");
    wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::ProviderRequestStarted(_)
                    if event.correlation_id.as_deref() == Some(beta_request_id.as_str())
            )
        })
    })
    .await;

    let second_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "second question")
        .await
        .expect("queued second turn");

    wait_for_events(&run.events_path, Duration::from_secs(1), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(second_request_id.as_str())
                        && data.result_summary == "second answer"
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    let requests = provider.requests();
    assert_eq!(requests.len(), 3, "expected all provider turns to run");
    let second_shape = requests[2]
        .messages
        .iter()
        .map(|message| (message.role.clone(), message.content.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        second_shape,
        vec![
            (MessageRole::System, "alpha-prompt".to_string()),
            (MessageRole::User, "first question".to_string()),
            (MessageRole::Assistant, "first answer".to_string()),
            (MessageRole::User, "second question".to_string()),
        ],
        "queued turn should use provider context recomputed after the earlier turn completed"
    );

    let events = load_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(second_request_id.as_str())
                    && data.state == TaskScheduleState::Queued
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCompleted(data)
                if event.correlation_id.as_deref() == Some(first_request_id.as_str())
                    && data.result_summary == "first answer"
        )
    }));
}
