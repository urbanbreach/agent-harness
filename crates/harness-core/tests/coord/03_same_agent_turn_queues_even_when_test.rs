use harness_core::UnwrapOrAbort;
#[tokio::test]
async fn same_agent_turn_queues_even_when_provider_model_has_free_slots() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let coordinator = test_agent_coordinator_with_provider(
        temp_dir.path(),
        Arc::new(SlowMockProvider {
            inner: test_mock_provider(),
            delay: Duration::from_millis(150),
        }),
        2,
    );

    let run = coordinator
        .start_run(
            "coord_same_agent_turn_queues",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();

    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();

    let first_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first prompt")
        .await
        .unwrap_or_abort();
    let queued_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "queued prompt")
        .await
        .unwrap_or_abort();

    let events = wait_for_events(&run.events_path, Duration::from_secs(2), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskScheduled(data)
                    if event.correlation_id.as_deref() == Some(queued_request_id.as_str())
                        && data.state == TaskScheduleState::Queued
            )
        }) && !events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::ProviderRequestStarted(_)
                    if event.correlation_id.as_deref() == Some(queued_request_id.as_str())
            )
        })
    })
    .await;
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(queued_request_id.as_str())
                    && data.state == TaskScheduleState::Queued
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ProviderRequestStarted(_)
                if event.correlation_id.as_deref() == Some(queued_request_id.as_str())
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(first_request_id.as_str())
                    && data.state == TaskScheduleState::Started
        )
    }));

    let events = wait_for_events(&run.events_path, Duration::from_secs(2), |events| {
        task_schedule_states_for_request(events, &queued_request_id)
            == vec![TaskScheduleState::Queued, TaskScheduleState::Started]
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

    let queued_schedule_states = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(queued_request_id.as_str()) =>
            {
                Some(data.state)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        queued_schedule_states,
        vec![TaskScheduleState::Queued, TaskScheduleState::Started]
    );
}
#[tokio::test]
async fn background_task_completion_after_tool_result_wakes_parent_in_followup_turn() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let tool_release = Arc::new(Notify::new());
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "parent_block".to_string(),
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
            ProviderStreamEvent::TextDelta("child completed".to_string()),
            ProviderStreamEvent::Done {
                usage: Some(CompletionUsage {
                    prompt_tokens: 2,
                    completion_tokens: 2,
                    total_tokens: 4,
                }),
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("parent final before wakeup".to_string()),
            ProviderStreamEvent::Done {
                usage: Some(CompletionUsage {
                    prompt_tokens: 3,
                    completion_tokens: 2,
                    total_tokens: 5,
                }),
            },
        ],
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("wakeup acknowledged".to_string()),
            ProviderStreamEvent::Done {
                usage: Some(CompletionUsage {
                    prompt_tokens: 3,
                    completion_tokens: 2,
                    total_tokens: 5,
                }),
            },
        ],
    ]);
    let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.provider_model_concurrency = 2;
    config.permission_policy = shell_only_permission_policy();
    config.tool_registry = lifecycle_tool_registry(Arc::clone(&tool_release));
    config.provider = Arc::new(provider.clone());
    config.agent_profiles = agent_profiles();
    if let Some(profile) = config.agent_profiles.get_mut("alpha") {
        profile.toolset = vec!["shell.block".to_string()];
    }
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = coordinator
        .start_run(
            "background_task_completion_after_tool_result_wakes_parent_in_followup_turn",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();
    let parent_agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let child_agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "beta", Some(parent_agent_id.clone()))
        .await
        .unwrap_or_abort();
    let parent_request_id = coordinator
        .request_agent_turn(supervisor_actor(), parent_agent_id.clone(), "parent prompt")
        .await
        .unwrap_or_abort();

    wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallRequested(data)
                    if data.tool_id == "shell.block"
                        && event.correlation_id.as_deref() == Some(parent_request_id.as_str())
            )
        })
    })
    .await;

    let child_request_id = coordinator
        .request_child_agent_turn_with_model(
            supervisor_actor(),
            child_agent_id.clone(),
            "child prompt",
            None,
            None,
            ChildTaskRequestMetadata {
                parent_tool_call_id: "toolcall_parent_task".to_string(),
                parent_session_id: run.run_id.as_str().into(),
                parent_agent_id: Some(parent_agent_id.clone()),
                child_session_id: child_agent_id.clone().into(),
                task_id: child_agent_id.into(),
                description: "Child background task".to_string(),
                run_in_background: true,
            },
        )
        .await
        .unwrap_or_abort();

    wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::BackgroundTaskNotification(data)
                    if data.child_request_id == child_request_id
            )
        })
    })
    .await;

    tool_release.notify_waiters();

    wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data) if data.result_summary == "wakeup acknowledged"
            )
        })
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

    let requests = provider.requests();
    assert_eq!(requests.len(), 4);
    let parent_tool_result_request = &requests[2];
    assert!(parent_tool_result_request.messages.iter().any(|message| {
        message.role == MessageRole::Tool && message.content.contains("unblocked")
    }));
    assert!(parent_tool_result_request
        .messages
        .iter()
        .all(|message| !message.content.contains("[BACKGROUND TASK COMPLETED]")));

    let wakeup_request = &requests[3];
    assert!(wakeup_request.messages.iter().any(|message| {
        message.role == MessageRole::User
            && message.content.contains("[BACKGROUND TASK COMPLETED]")
            && message
                .content
                .contains(&format!("Request ID: {child_request_id}"))
    }));
}
#[tokio::test]
async fn same_agent_blocked_turns_start_fifo() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let coordinator = test_agent_coordinator_with_provider(
        temp_dir.path(),
        Arc::new(SlowMockProvider {
            inner: test_mock_provider(),
            delay: Duration::from_millis(40),
        }),
        2,
    );

    let run = coordinator
        .start_run("coord_same_agent_fifo", PathBuf::from("/workspace/project"))
        .await
        .unwrap_or_abort();

    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();

    let first_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first prompt")
        .await
        .unwrap_or_abort();
    let second_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "second prompt")
        .await
        .unwrap_or_abort();
    let third_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "third prompt")
        .await
        .unwrap_or_abort();

    let events = wait_for_events(&run.events_path, Duration::from_secs(2), |events| {
        provider_started_request_ids(events).len() >= 3
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

    let started_request_ids = provider_started_request_ids(&events);
    let expected = vec![first_request_id, second_request_id, third_request_id];
    assert_eq!(started_request_ids, expected);
}
#[tokio::test]
async fn cancelling_promoted_same_agent_queued_turn_promotes_next_blocked_turn() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let coordinator = test_agent_coordinator_with_provider(
        temp_dir.path(),
        Arc::new(SlowMockProvider {
            inner: test_mock_provider(),
            delay: Duration::from_millis(250),
        }),
        1,
    );

    let run = coordinator
        .start_run(
            "coord_same_agent_cancel_promoted",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();

    let alpha = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let beta = coordinator
        .spawn_agent_idle(supervisor_actor(), "beta", None)
        .await
        .unwrap_or_abort();

    let _alpha_first = coordinator
        .request_agent_turn(supervisor_actor(), alpha.clone(), "alpha first")
        .await
        .unwrap_or_abort();
    let beta_request_id = coordinator
        .request_agent_turn(supervisor_actor(), beta, "beta first")
        .await
        .unwrap_or_abort();
    let alpha_second = coordinator
        .request_agent_turn(supervisor_actor(), alpha.clone(), "alpha second")
        .await
        .unwrap_or_abort();
    let alpha_third = coordinator
        .request_agent_turn(supervisor_actor(), alpha, "alpha third")
        .await
        .unwrap_or_abort();

    let events = wait_for_events(&run.events_path, Duration::from_secs(5), |events| {
        task_schedule_states_for_request(events, &alpha_second) == vec![TaskScheduleState::Queued]
            && events.iter().any(|event| {
                matches!(
                    &event.payload,
                    EventV1::TaskScheduled(data)
                        if event.correlation_id.as_deref() == Some(beta_request_id.as_str())
                            && data.state == TaskScheduleState::Started
                )
            })
    })
    .await;
    let alpha_second_task_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(alpha_second.as_str()) =>
            {
                Some(data.task_id.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();

    coordinator
        .cancel_task(alpha_second_task_id, "skip promoted same-agent prompt")
        .await
        .unwrap_or_abort();

    let events = wait_for_events(&run.events_path, Duration::from_secs(5), |events| {
        task_schedule_states_for_request(events, &alpha_third)
            == vec![TaskScheduleState::Queued, TaskScheduleState::Started]
            && events.iter().any(|event| {
                matches!(
                    &event.payload,
                    EventV1::ProviderRequestStarted(_)
                        if event.correlation_id.as_deref() == Some(alpha_third.as_str())
                )
            })
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ProviderRequestStarted(_)
                if event.correlation_id.as_deref() == Some(alpha_second.as_str())
        )
    }));
}
#[tokio::test]
async fn queued_agent_turn_cancellation_preserves_owner_context() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let coordinator = test_agent_coordinator(temp_dir.path(), Duration::from_millis(25));

    let run = coordinator
        .start_run(
            "coord_agent_turn_cancel_queued",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();

    let alpha = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let beta = coordinator
        .spawn_agent_idle(supervisor_actor(), "beta", None)
        .await
        .unwrap_or_abort();

    let _running_request_id = coordinator
        .request_agent_turn(supervisor_actor(), alpha, "alpha-prompt")
        .await
        .unwrap_or_abort();
    let queued_request_id = coordinator
        .request_agent_turn(supervisor_actor(), beta.clone(), "beta-prompt")
        .await
        .unwrap_or_abort();

    let task_id = load_events(&run.events_path)
        .into_iter()
        .find_map(|event| match event.payload {
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(queued_request_id.as_str())
                    && data.state == TaskScheduleState::Queued =>
            {
                Some(data.task_id)
            }
            _ => None,
        })
        .unwrap_or_abort();

    coordinator
        .cancel_task(task_id.clone(), "manual queued cancellation")
        .await
        .unwrap_or_abort();

    tokio::task::yield_now().await;
    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);
    let cancellations = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data) if data.task_id == task_id
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        cancellations.len(),
        1,
        "queued turn should emit one cancellation"
    );
    assert_task_event_context(
        cancellations[0],
        &EventActor::new(ActorKind::Worker, Some(beta)),
        &queued_request_id,
    );
    assert!(matches!(
        &cancellations[0].payload,
        EventV1::TaskCancelled(data) if data.reason == "manual queued cancellation"
    ));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskScheduled(data)
                if data.task_id == task_id && data.state == TaskScheduleState::Started
        )
    }));
}
