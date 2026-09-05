use harness_core::UnwrapOrAbort;
#[tokio::test]
async fn cancelling_turn_waiting_for_permission_emits_turn_end_without_tool_start() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::ToolCallComplete {
            tool_call_id: "needs_permission".to_string(),
            function_name: "shell_run".to_string(),
            arguments_json: "{}".to_string(),
        },
        ProviderStreamEvent::Done {
            usage: Some(CompletionUsage {
                prompt_tokens: 2,
                completion_tokens: 1,
                total_tokens: 3,
            }),
        },
    ]]);
    let coordinator = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(provider),
        test_tool_registry(),
        ask_shell_permission_policy(),
        vec!["shell.run".to_string()],
        12,
    );

    let run = coordinator
        .start_run(
            "coord_cancel_turn_waiting_permission",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "permission gated tool")
        .await
        .unwrap_or_abort();

    let events = wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events
            .iter()
            .any(|event| matches!(event.payload, EventV1::PermissionRequested(_)))
    })
    .await;
    let agent_task_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str())
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
    let (permission_id, provider_tool_call_id) = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data) => Some((
                data.permission_id.clone(),
                data.tool_call_id.clone().unwrap_or_abort(),
            )),
            _ => None,
        })
        .unwrap_or_abort();

    coordinator
        .cancel_task(agent_task_id.clone(), "cancel while permission pending")
        .await
        .unwrap_or_abort();
    coordinator
        .resolve_permission(
            permission_id,
            RuntimePermissionDecision::Allow,
            Some("late approval".to_string()),
        )
        .await
        .unwrap_or_abort();
    tokio::task::yield_now().await;
    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCancelled(data)
                if data.task_id == agent_task_id
                    && data.reason == "cancel while permission pending"
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id == provider_tool_call_id
        )
    }));
}
#[tokio::test]
async fn late_tool_result_after_turn_cancellation_is_task_result_late() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let release = Arc::new(Notify::new());
    let provider = SequentialScriptedProvider::new(vec![
        vec![
            ProviderStreamEvent::Start,
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
            ProviderStreamEvent::TextDelta(
                "should not be requested after cancellation".to_string(),
            ),
            ProviderStreamEvent::Done {
                usage: Some(CompletionUsage {
                    prompt_tokens: 3,
                    completion_tokens: 2,
                    total_tokens: 5,
                }),
            },
        ],
    ]);
    let coordinator = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(provider),
        lifecycle_tool_registry(Arc::clone(&release)),
        shell_only_permission_policy(),
        vec!["shell.block".to_string()],
        12,
    );

    let run = coordinator
        .start_run(
            "coord_late_tool_after_turn_cancel",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "run blocking tool")
        .await
        .unwrap_or_abort();

    let events = wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskScheduled(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.queue_key.as_deref() == Some("tool:shell.block")
            )
        })
    })
    .await;
    let agent_task_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str())
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
    let tool_task_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str())
                    && data.queue_key.as_deref() == Some("tool:shell.block") =>
            {
                Some(data.task_id.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();

    coordinator
        .cancel_task(agent_task_id.clone(), "cancel turn during tool execution")
        .await
        .unwrap_or_abort();
    let events = wait_for_events(&run.events_path, Duration::from_millis(700), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskResultLate(data) if data.task_id == tool_task_id
            )
        })
    })
    .await;
    release.notify_waiters();
    coordinator.stop_run().await.unwrap_or_abort();

    let turn_terminal_events = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data) if data.task_id == agent_task_id
            ) || matches!(
                &event.payload,
                EventV1::TaskCompleted(data) if data.task_id == agent_task_id
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        turn_terminal_events.len(),
        1,
        "cancelled turn should have exactly one terminal event"
    );
    assert!(matches!(
        &turn_terminal_events[0].payload,
        EventV1::TaskCancelled(data)
            if data.reason == "cancel turn during tool execution"
    ));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskResultLate(data) if data.task_id == tool_task_id
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCompleted(data) if data.task_id == tool_task_id
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ProviderStreamDelta(data)
                if data.delta == "should not be requested after cancellation"
        )
    }));
}
#[tokio::test]
async fn cancelled_tool_task_records_late_result_without_completion() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let release = Arc::new(Notify::new());
    let clock = Arc::new(FakeClock::new());
    let coordinator = test_tool_lifecycle_coordinator(
        temp_dir.path(),
        clock,
        lifecycle_tool_registry(Arc::clone(&release)),
        Duration::from_millis(50),
        15_000,
        5,
        1,
    );

    let run = coordinator
        .start_run(
            "coord_cancel_tool_late_result",
            temp_dir.path().to_path_buf(),
        )
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "alpha-prompt")
        .await
        .unwrap_or_abort();
    let owner_actor = EventActor::new(ActorKind::Worker, Some(agent_id));
    tokio::task::yield_now().await;
    coordinator
        .request_tool_call(
            owner_actor.clone(),
            Some("deep".to_string()),
            "shell.block",
            json!({"cmd": "wait"}),
        )
        .await
        .unwrap_or_abort();

    let events = wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskScheduled(data)
                    if data.queue_key.as_deref() == Some("tool:shell.block")
            )
        })
    })
    .await;
    let task_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskScheduled(data)
                if data.queue_key.as_deref() == Some("tool:shell.block") =>
            {
                Some(data.task_id.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();
    coordinator
        .cancel_task(task_id.clone(), "manual cancellation")
        .await
        .unwrap_or_abort();
    release.notify_waiters();
    wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskResultLate(data) if data.task_id == task_id
            )
        })
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);
    let late = events
        .iter()
        .find(|event| {
            matches!(
                &event.payload,
                EventV1::TaskResultLate(data) if data.task_id == task_id
            )
        })
        .unwrap_or_abort();
    assert_task_event_context(late, &owner_actor, &request_id);
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCompleted(data) if data.task_id == task_id
        )
    }));
}
#[tokio::test]
async fn provider_partial_output_then_error_is_not_successful_assistant_message() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::TextDelta("partial answer".to_string()),
        ProviderStreamEvent::error("provider exploded"),
    ]]);
    let coordinator = test_agent_coordinator_with_provider(temp_dir.path(), Arc::new(provider), 1);

    let run = coordinator
        .start_run(
            "coord_provider_partial_output_error",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "partial then error")
        .await
        .unwrap_or_abort();

    let events = wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.reason == "provider exploded"
            )
        })
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

    let finished_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ProviderRequestFinished(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.finish_reason == "error"
                        && data.output_digest.is_none()
            )
        })
        .unwrap_or_abort();
    assert!(!events[..finished_idx].iter().any(|event| {
        matches!(&event.payload, EventV1::ProviderStreamDelta(_))
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCompleted(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str())
                    && data.result_summary.contains("partial answer")
        )
    }));
}
#[tokio::test]
async fn records_provider_error_events_and_fails_agent_turn() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::TextDelta("partial answer".to_string()),
        ProviderStreamEvent::categorized_error(
            "provider exploded",
            ProviderErrorCategory::RateLimited,
        ),
    ]]);
    let coordinator = test_agent_coordinator_with_provider(temp_dir.path(), Arc::new(provider), 1);

    let run = coordinator
        .start_run(
            "coord_provider_error_fails_agent_turn",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "partial then error")
        .await
        .unwrap_or_abort();

    let events = wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && data.reason == "rate_limited: provider exploded"
            )
        })
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

    assert!(!events.iter().any(|event| {
        matches!(&event.payload, EventV1::ProviderStreamDelta(_))
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ProviderRequestFinished(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str())
                    && data.finish_reason == "error"
                    && data.output_digest.is_none()
                    && data.metadata.as_ref().and_then(|metadata| metadata.provider_error_category)
                        == Some(ProviderErrorCategory::RateLimited)
                    && data.metadata.as_ref()
                        .and_then(|metadata| metadata.provider_error_remediation.as_deref())
                        .is_some_and(|hint| hint.contains("rate limit"))
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCompleted(data)
                if event.correlation_id.as_deref() == Some(request_id.as_str())
                    && data.result_summary.contains("partial answer")
        )
    }));
}
