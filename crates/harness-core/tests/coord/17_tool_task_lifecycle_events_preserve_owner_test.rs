use harness_core::UnwrapOrAbort;
#[tokio::test]
async fn tool_task_lifecycle_events_preserve_owner_actor() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let clock = Arc::new(FakeClock::new());
    let coordinator = test_tool_lifecycle_coordinator(
        temp_dir.path(),
        clock,
        lifecycle_tool_registry(Arc::new(Notify::new())),
        Duration::from_millis(100),
        15_000,
        5,
        2,
    );

    let run = coordinator
        .start_run("tool_task_owner", temp_dir.path().to_path_buf())
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
            "shell.run",
            json!({"cmd": "true"}),
        )
        .await
        .unwrap_or_abort();
    coordinator
        .request_tool_call(
            owner_actor.clone(),
            Some("deep".to_string()),
            "shell.fail",
            json!({"cmd": "false"}),
        )
        .await
        .unwrap_or_abort();

    tokio::task::yield_now().await;
    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);
    let tool_task_ids = tool_task_ids(&events);
    assert_eq!(tool_task_ids.len(), 2, "expected two tool task ids");

    let scheduled_events = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventV1::TaskScheduled(data) if tool_task_ids.contains(data.task_id.as_str())
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        scheduled_events.len(),
        2,
        "expected two tool TaskScheduled events"
    );
    for event in scheduled_events {
        assert_task_event_context(event, &owner_actor, &request_id);
    }

    let terminal_events = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data) if tool_task_ids.contains(data.task_id.as_str())
            ) || matches!(
                &event.payload,
                EventV1::TaskCancelled(data) if tool_task_ids.contains(data.task_id.as_str())
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        terminal_events.len(),
        2,
        "expected two tool terminal events"
    );

    let completed = terminal_events
        .iter()
        .filter(|event| matches!(&event.payload, EventV1::TaskCompleted(_)))
        .count();
    let cancelled = terminal_events
        .iter()
        .filter(|event| matches!(&event.payload, EventV1::TaskCancelled(_)))
        .count();
    assert_eq!(completed, 1, "expected one tool completion");
    assert_eq!(cancelled, 1, "expected one tool cancellation");

    for event in terminal_events {
        assert_task_event_context(event, &owner_actor, &request_id);
    }
}
#[tokio::test]
async fn stale_tool_task_late_result_preserves_owner_actor() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let clock = Arc::new(FakeClock::new());
    let coordinator = test_tool_lifecycle_coordinator(
        temp_dir.path(),
        Arc::clone(&clock),
        lifecycle_tool_registry(Arc::new(Notify::new())),
        Duration::from_millis(100),
        10,
        5,
        1,
    );

    let run = coordinator
        .start_run("stale_tool_task_owner", temp_dir.path().to_path_buf())
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

    tokio::task::yield_now().await;
    let task_id = load_events(&run.events_path)
        .into_iter()
        .find_map(|event| match event.payload {
            EventV1::TaskScheduled(data)
                if data.queue_key.as_deref() == Some("tool:shell.block") =>
            {
                Some(data.task_id)
            }
            _ => None,
        })
        .unwrap_or_abort();
    coordinator
        .job_progress(task_id.clone(), JobProgressKind::Heartbeat)
        .await
        .unwrap_or_abort();
    coordinator
        .cancel_task(task_id.clone(), "manual cancellation")
        .await
        .unwrap_or_abort();
    coordinator
        .job_finished(
            task_id.clone(),
            JobOutcome::Cancelled {
                reason: "job cancelled".to_string(),
            },
        )
        .await
        .unwrap_or_abort();

    clock.advance(25);
    tokio::task::yield_now().await;
    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);
    let cancelled_event = events
        .iter()
        .find(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data) if data.task_id == task_id
            )
        })
        .unwrap_or_abort();
    assert_task_event_context(cancelled_event, &owner_actor, &request_id);

    let late_event = events
        .iter()
        .find(|event| {
            matches!(
                &event.payload,
                EventV1::TaskResultLate(data) if data.task_id == task_id
            )
        })
        .unwrap_or_abort();
    assert_task_event_context(late_event, &owner_actor, &request_id);
}
#[tokio::test]
async fn critical_hook_failure_fails_closed_and_records_metadata() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let hook_output_path = temp_dir.path().join("hook-finish.txt");
    let hook_runtime_config = HookRuntimeConfig {
        hooks: HooksConfig {
            lifecycle: vec![
                LifecycleHookConfig {
                    id: Some("tool-start-timeout".to_string()),
                    event: HookLifecycleEvent::ToolCallStarted,
                    command: vec![
                        "bash".to_string(),
                        "-lc".to_string(),
                        "sleep 0.05".to_string(),
                    ],
                    cwd: Some(".".to_string()),
                    timeout_ms: 10,
                    critical: false,
                    env: BTreeMap::new(),
                },
                LifecycleHookConfig {
                    id: Some("tool-finish-critical".to_string()),
                    event: HookLifecycleEvent::ToolCallFinished,
                    command: vec![
                        "bash".to_string(),
                        "-lc".to_string(),
                        "printf '%s|%s|%s|%s' \"$PWD\" \"$HOOK_CUSTOM\" \"$HARNESS_HOOK_EVENT\" \"$HARNESS_HOOK_TOOL_ID\" > \"$HOOK_OUTPUT_PATH\"; exit 23".to_string(),
                    ],
                    cwd: Some(".".to_string()),
                    timeout_ms: 4_000,
                    critical: true,
                    env: BTreeMap::from([
                        ("HOOK_CUSTOM".to_string(), "from-config".to_string()),
                        (
                            "HOOK_OUTPUT_PATH".to_string(),
                            hook_output_path.display().to_string(),
                        ),
                    ]),
                },
            ],
        },
        shell_allowlist: ShellAllowlist {
            executables: vec!["bash".to_string()],
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        },
        suppress_execution: false,
    };

    let clock = Arc::new(FakeClock::new());
    let coordinator = test_tool_lifecycle_coordinator_with_hook_runtime(
        temp_dir.path(),
        clock,
        lifecycle_tool_registry(Arc::new(Notify::new())),
        Duration::from_millis(50),
        15_000,
        5,
        1,
        hook_runtime_config,
    );

    let run = coordinator
        .start_run(
            "critical_hook_failure_fails_closed_and_records_metadata",
            temp_dir.path().to_path_buf(),
        )
        .await
        .unwrap_or_abort();

    let tool_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "true"}),
        )
        .await
        .unwrap_or_abort();

    tokio::task::yield_now().await;
    coordinator.stop_run().await.unwrap_or_abort();

    let hook_output = fs::read_to_string(&hook_output_path).unwrap_or_abort();
    assert!(
        hook_output.starts_with(&temp_dir.path().display().to_string()),
        "hook should execute from workspace-root cwd: {hook_output}"
    );
    assert!(hook_output.contains("from-config|tool_call_finished|shell.run"));

    let events = load_events(&run.events_path);
    let task_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskScheduled(data) if data.queue_key.as_deref() == Some("tool:shell.run") => {
                Some(data.task_id.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();

    assert!(
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data) if data.task_id == task_id
            )
        }),
        "critical finish hook should fail closed and cancel the task"
    );
    assert!(
        !events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data) if data.task_id == task_id
            )
        }),
        "critical finish hook must prevent successful task completion"
    );

    let tool_finished = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallFinished(data) if data.tool_call_id.as_str() == tool_call_id => Some(data),
            _ => None,
        })
        .unwrap_or_abort();
    assert_eq!(tool_finished.status, ToolCallStatus::Failed);
    let hook_executions = tool_finished
        .metadata
        .as_ref()
        .map(|metadata| metadata.hook_executions.clone())
        .unwrap_or_abort();
    assert_eq!(hook_executions.len(), 2, "expected both hooks recorded");
    assert_eq!(hook_executions[0].hook_name, "tool-start-timeout");
    assert_eq!(hook_executions[0].status, HookExecutionStatus::Failed);
    assert_eq!(
        hook_executions[0].hook_event.as_deref(),
        Some("tool_call_started")
    );
    assert_eq!(
        hook_executions[0].output_summary.as_deref(),
        Some("no output")
    );
    assert_eq!(hook_executions[1].hook_name, "tool-finish-critical");
    assert_eq!(hook_executions[1].status, HookExecutionStatus::Failed);
    assert_eq!(
        hook_executions[1].hook_event.as_deref(),
        Some("tool_call_finished")
    );
    assert_eq!(
        hook_executions[1].output_summary.as_deref(),
        Some("no output")
    );
}
#[tokio::test]
async fn noncritical_hook_failure_records_metadata_without_cancelling_task() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let hook_output_path = temp_dir.path().join("hook-finish-noncritical.txt");
    let hook_runtime_config = HookRuntimeConfig {
        hooks: HooksConfig {
            lifecycle: vec![LifecycleHookConfig {
                id: Some("tool-finish-noncritical".to_string()),
                event: HookLifecycleEvent::ToolCallFinished,
                command: vec![
                    "bash".to_string(),
                    "-lc".to_string(),
                    "printf '%s|%s|%s' \"$PWD\" \"$HARNESS_HOOK_EVENT\" \"$HARNESS_HOOK_TOOL_ID\" > \"$HOOK_OUTPUT_PATH\"; exit 17"
                        .to_string(),
                ],
                cwd: Some(".".to_string()),
                timeout_ms: 4_000,
                critical: false,
                env: BTreeMap::from([(
                    "HOOK_OUTPUT_PATH".to_string(),
                    hook_output_path.display().to_string(),
                )]),
            }],
        },
        shell_allowlist: ShellAllowlist {
            executables: vec!["bash".to_string()],
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        },
        suppress_execution: false,
    };

    let clock = Arc::new(FakeClock::new());
    let coordinator = test_tool_lifecycle_coordinator_with_hook_runtime(
        temp_dir.path(),
        clock,
        lifecycle_tool_registry(Arc::new(Notify::new())),
        Duration::from_millis(50),
        15_000,
        5,
        1,
        hook_runtime_config,
    );

    let run = coordinator
        .start_run(
            "noncritical_hook_failure_records_metadata_without_cancelling_task",
            temp_dir.path().to_path_buf(),
        )
        .await
        .unwrap_or_abort();

    let tool_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "true"}),
        )
        .await
        .unwrap_or_abort();

    tokio::task::yield_now().await;
    coordinator.stop_run().await.unwrap_or_abort();

    let hook_output = fs::read_to_string(&hook_output_path).unwrap_or_abort();
    assert!(
        hook_output.starts_with(&temp_dir.path().display().to_string()),
        "hook should execute from workspace-root cwd: {hook_output}"
    );
    assert!(hook_output.contains("tool_call_finished|shell.run"));

    let events = load_events(&run.events_path);
    let task_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskScheduled(data) if data.queue_key.as_deref() == Some("tool:shell.run") => {
                Some(data.task_id.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();

    assert!(
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(data) if data.task_id == task_id
            )
        }),
        "non-critical hook failure should keep the task completion intact"
    );
    assert!(
        !events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCancelled(data) if data.task_id == task_id
            )
        }),
        "non-critical hook failure should not cancel the task"
    );

    let tool_finished = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallFinished(data) if data.tool_call_id.as_str() == tool_call_id => Some(data),
            _ => None,
        })
        .unwrap_or_abort();
    assert_eq!(tool_finished.status, ToolCallStatus::Succeeded);
    let hook_executions = tool_finished
        .metadata
        .as_ref()
        .map(|metadata| metadata.hook_executions.clone())
        .unwrap_or_abort();
    assert_eq!(
        hook_executions.len(),
        1,
        "expected one failed hook recorded"
    );
    assert_eq!(hook_executions[0].hook_name, "tool-finish-noncritical");
    assert_eq!(hook_executions[0].status, HookExecutionStatus::Failed);
    assert_eq!(
        hook_executions[0].hook_event.as_deref(),
        Some("tool_call_finished")
    );
    assert_eq!(
        hook_executions[0].output_summary.as_deref(),
        Some("no output")
    );
}
#[test]
fn hook_runner_blocks_critical_and_reports_noncritical_failures() {
    // arrange
    // act
    // assert
    critical_hook_failure_fails_closed_and_records_metadata();
    noncritical_hook_failure_records_metadata_without_cancelling_task();
}
