use harness_core::UnwrapOrAbort;
#[tokio::test]
async fn tool_task_lifecycle_events_preserve_owner_actor() {
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
async fn cancelled_tool_outcome_preserves_terminal_event_metadata() {
    // arrange
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let release = Arc::new(Notify::new());
    let coordinator = test_tool_lifecycle_coordinator(
        temp_dir.path(),
        Arc::new(FakeClock::new()),
        lifecycle_tool_registry(Arc::clone(&release)),
        Duration::from_millis(100),
        15_000,
        5,
        1,
    );
    let run = coordinator
        .start_run(
            "cancelled_tool_outcome_preserves_terminal_event_metadata",
            temp_dir.path().to_path_buf(),
        )
        .await
        .unwrap_or_abort();
    let tool_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
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

    // act
    coordinator
        .job_finished(
            task_id.clone(),
            JobOutcome::Cancelled {
                reason: "operator cancelled".to_string(),
            },
        )
        .await
        .unwrap_or_abort();
    let events = wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallFinished(data) if data.tool_call_id.as_str() == tool_call_id
            )
        })
    })
    .await;
    release.notify_waiters();
    coordinator.stop_run().await.unwrap_or_abort();

    // assert
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCancelled(data)
                if data.task_id == task_id && data.reason == "operator cancelled"
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCompleted(data) if data.task_id == task_id
        )
    }));
    let finished = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallFinished(data) if data.tool_call_id.as_str() == tool_call_id => {
                Some(data)
            }
            _ => None,
        })
        .unwrap_or_abort();
    assert_eq!(finished.status, ToolCallStatus::Failed);
    assert_eq!(finished.output_summary.as_deref(), Some("operator cancelled"));
    assert!(
        finished
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.timing.as_ref())
            .is_some(),
        "cancelled tool outcome should preserve timing metadata"
    );
}

#[tokio::test]
async fn stale_tool_task_late_result_preserves_owner_actor() {
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
                        "-c".to_string(),
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
                        "-c".to_string(),
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
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let hook_output_path = temp_dir.path().join("hook-finish-noncritical.txt");
    let hook_runtime_config = HookRuntimeConfig {
        hooks: HooksConfig {
            lifecycle: vec![LifecycleHookConfig {
                id: Some("tool-finish-noncritical".to_string()),
                event: HookLifecycleEvent::ToolCallFinished,
                command: vec![
                    "bash".to_string(),
                    "-c".to_string(),
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
    critical_hook_failure_fails_closed_and_records_metadata();
    noncritical_hook_failure_records_metadata_without_cancelling_task();
}

struct SchedulerControlledTool {
    id: &'static str,
    started: tokio::sync::mpsc::UnboundedSender<(String, tokio::sync::oneshot::Sender<bool>)>,
}

#[async_trait]
impl Tool for SchedulerControlledTool {
    fn id(&self) -> &str {
        self.id
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(
        &self,
        ctx: ToolContext,
        _args: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        // Consume the spawn heartbeat before tests advance the injected clock.
        ctx.coordinator.event_store().await.unwrap_or_abort();
        let (finish, finished) = tokio::sync::oneshot::channel();
        self.started
            .send((ctx.tool_call_id.to_string(), finish))
            .unwrap_or_abort();
        if finished.await.unwrap_or_abort() {
            Ok(ToolResult::text("finished"))
        } else {
            Err(ToolError::Execution("controlled failure".to_string()))
        }
    }
}

async fn submit_native_tool(
    coordinator: &CoordinatorHandle,
    events_path: &Path,
    actor: EventActor,
    tool_id: &str,
    marker: usize,
) -> (
    String,
    String,
    tokio::task::JoinHandle<Result<ToolResult, String>>,
) {
    let previous_seq = load_events(events_path).last().map_or(0, |event| event.seq);
    let handle = coordinator.clone();
    let tool_id = tool_id.to_string();
    let response = tokio::spawn(async move {
        handle
            .execute_agent_tool_call(actor, None, tool_id, json!({"marker": marker}))
            .await
    });
    let events = wait_for_events(events_path, Duration::from_secs(2), |events| {
        events.iter().any(|event| {
            event.seq > previous_seq
                && matches!(&event.payload, EventV1::TaskScheduled(data)
                if data.queue_key.as_deref().is_some_and(|key| key.starts_with("tool:")))
        })
    })
    .await;
    let task_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskScheduled(data) if event.seq > previous_seq => {
                Some(data.task_id.to_string())
            }
            _ => None,
        })
        .unwrap_or_abort();
    let call_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallRequested(data) if event.seq > previous_seq => {
                Some(data.tool_call_id.to_string())
            }
            _ => None,
        })
        .unwrap_or_abort();
    (task_id, call_id, response)
}

#[tokio::test]
async fn native_tool_scheduler_admits_fifo_cancels_queue_and_releases_failures() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let (started, mut starts) = tokio::sync::mpsc::unbounded_channel();
    let mut registry = ToolRegistry::new();
    for id in ["shell.block", "shell.other"] {
        registry.register(Arc::new(SchedulerControlledTool {
            id,
            started: started.clone(),
        }));
    }
    let coordinator = test_tool_lifecycle_coordinator(
        temp_dir.path(),
        Arc::new(FakeClock::new()),
        Arc::new(registry),
        Duration::ZERO,
        15_000,
        5,
        1,
    );
    let run = coordinator
        .start_run("native_tool_scheduler_fifo", temp_dir.path())
        .await
        .unwrap_or_abort();
    let mut calls = Vec::new();
    for marker in 0..4 {
        calls.push(
            submit_native_tool(
                &coordinator,
                &run.events_path,
                supervisor_actor(),
                "shell.block",
                marker,
            )
            .await,
        );
    }
    let states: Vec<_> = load_events(&run.events_path)
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::TaskScheduled(data) => Some(data.state),
            _ => None,
        })
        .collect();
    assert_eq!(
        states,
        vec![
            TaskScheduleState::Started,
            TaskScheduleState::Queued,
            TaskScheduleState::Queued,
            TaskScheduleState::Queued
        ]
    );
    let (first_call, finish_first) = starts.recv().await.unwrap_or_abort();
    assert_eq!(first_call, calls[0].1);
    let other = submit_native_tool(
        &coordinator,
        &run.events_path,
        supervisor_actor(),
        "shell.other",
        0,
    )
    .await;
    let (other_call, finish_other) = starts.recv().await.unwrap_or_abort();
    assert_eq!(other_call, other.1);
    finish_other.send(true).unwrap_or_abort();
    assert!(other.2.await.unwrap_or_abort().is_ok());
    coordinator
        .cancel_task(&calls[2].0, "cancel queued")
        .await
        .unwrap_or_abort();
    finish_first.send(true).unwrap_or_abort();
    let (second_call, finish_second) = starts.recv().await.unwrap_or_abort();
    assert_eq!(second_call, calls[1].1);
    coordinator
        .job_finished(
            &calls[0].0,
            JobOutcome::Cancelled {
                reason: "duplicate late result".to_string(),
            },
        )
        .await
        .unwrap_or_abort();
    coordinator.event_store().await.unwrap_or_abort();
    assert!(!load_events(&run.events_path).iter().any(|event| matches!(&event.payload, EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == calls[3].1)));
    finish_second.send(false).unwrap_or_abort();
    let (fourth_call, finish_fourth) = starts.recv().await.unwrap_or_abort();
    assert_eq!(fourth_call, calls[3].1);
    finish_fourth.send(true).unwrap_or_abort();
    for (index, (_, _, response)) in calls.into_iter().enumerate() {
        let result = tokio::time::timeout(Duration::from_secs(2), response)
            .await
            .unwrap_or_abort()
            .unwrap_or_abort();
        assert_eq!(result.is_ok(), matches!(index, 0 | 3));
    }
    assert!(starts.try_recv().is_err());
    coordinator.stop_run().await.unwrap_or_abort();
}

#[tokio::test]
async fn native_tool_scheduler_automatically_cancels_stale_leaf_and_promotes_queue() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let clock = Arc::new(FakeClock::new());
    let (started, mut starts) = tokio::sync::mpsc::unbounded_channel();
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(SchedulerControlledTool {
        id: "shell.block",
        started,
    }));
    let coordinator = test_tool_lifecycle_coordinator(
        temp_dir.path(),
        Arc::clone(&clock),
        Arc::new(registry),
        Duration::ZERO,
        10,
        1,
        1,
    );
    let run = coordinator
        .start_run("native_tool_scheduler_stale", temp_dir.path())
        .await
        .unwrap_or_abort();
    let first = submit_native_tool(
        &coordinator,
        &run.events_path,
        supervisor_actor(),
        "shell.block",
        0,
    )
    .await;
    let (_, _finish_first) = starts.recv().await.unwrap_or_abort();
    let second = submit_native_tool(
        &coordinator,
        &run.events_path,
        supervisor_actor(),
        "shell.block",
        1,
    )
    .await;
    clock.advance(25);
    let events = wait_for_events(&run.events_path, Duration::from_secs(2), |events| {
        events.iter().any(|event| {
            matches!(&event.payload, EventV1::StaleDetected(data)
            if data.task_id.as_str() == first.0)
        })
    })
    .await;
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(&event.payload, EventV1::StaleDetected(_)))
            .count(),
        1
    );
    assert!(tokio::time::timeout(Duration::from_secs(2), first.2)
        .await
        .unwrap_or_abort()
        .unwrap_or_abort()
        .is_err());
    let (call_id, finish) = tokio::time::timeout(Duration::from_secs(2), starts.recv())
        .await
        .unwrap_or_abort()
        .unwrap_or_abort();
    assert_eq!(call_id, second.1);
    finish.send(true).unwrap_or_abort();
    assert!(second.2.await.unwrap_or_abort().is_ok());
    coordinator.stop_run().await.unwrap_or_abort();
}

struct SchedulerControlledProvider {
    started: tokio::sync::mpsc::UnboundedSender<tokio::sync::oneshot::Sender<()>>,
}

#[async_trait]
impl Provider for SchedulerControlledProvider {
    fn request_budget_semantics(
        &self,
        request: &CompletionRequest,
        pending_prompt_index: usize,
    ) -> Result<
        harness_providers::ProviderBudgetSemantics,
        harness_providers::ProviderRequestCostError,
    > {
        harness_providers::generic_request_budget_semantics(request, pending_prompt_index)
    }
    async fn stream_completion(&self, _request: CompletionRequest) -> ProviderEventStream {
        let (finish, finished) = tokio::sync::oneshot::channel();
        self.started.send(finish).unwrap_or_abort();
        finished.await.unwrap_or_abort();
        Box::pin(tokio_stream::iter(provider_text_events("child finished")))
    }
}

#[tokio::test]
async fn native_tool_scheduler_foreground_wait_resets_watchdog_after_child_termination() {
    for (wrapper, queued_child) in [("task", false), ("agent.spawn", true)] {
        let temp_dir = tempfile::tempdir().unwrap_or_abort();
        let clock = Arc::new(FakeClock::new());
        let (started, mut starts) = tokio::sync::mpsc::unbounded_channel();
        let (provider_started, mut provider_starts) = tokio::sync::mpsc::unbounded_channel();
        let mut registry = ToolRegistry::new();
        for id in [wrapper, "shell.block"] {
            registry.register(Arc::new(SchedulerControlledTool {
                id,
                started: started.clone(),
            }));
        }
        let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
        config.tool_registry = Arc::new(registry);
        config.provider = Arc::new(SchedulerControlledProvider {
            started: provider_started,
        });
        config.agent_profiles = agent_profiles();
        config.tool_concurrency = 1;
        config.provider_model_concurrency = 1;
        config.stale_timeout_ms = 15_000;
        config.watchdog_tick_ms = 1;
        let coordinator = spawn_coordinator(
            config,
            Arc::<FakeClock>::clone(&clock),
            Arc::new(DefaultRedactor::default()),
        );
        let run = coordinator
            .start_run("foreground_watchdog", temp_dir.path())
            .await
            .unwrap_or_abort();
        let parent = submit_native_tool(
            &coordinator,
            &run.events_path,
            supervisor_actor(),
            wrapper,
            0,
        )
        .await;
        let (_, _finish_parent) = starts.recv().await.unwrap_or_abort();
        let mut blocker = None;
        if queued_child {
            let agent = coordinator
                .spawn_agent_idle(supervisor_actor(), "alpha", None)
                .await
                .unwrap_or_abort();
            coordinator
                .request_agent_turn(supervisor_actor(), agent, "hold provider slot")
                .await
                .unwrap_or_abort();
            blocker = Some(provider_starts.recv().await.unwrap_or_abort());
        }
        let child = coordinator
            .spawn_agent_idle(supervisor_actor(), "alpha", None)
            .await
            .unwrap_or_abort();
        let request = coordinator
            .request_child_agent_turn_with_model(
                supervisor_actor(),
                child.clone(),
                "child",
                None,
                None,
                ChildTaskRequestMetadata {
                    parent_tool_call_id: parent.1.clone(),
                    parent_session_id: run.run_id.as_str().into(),
                    parent_agent_id: None,
                    child_session_id: child.clone().into(),
                    task_id: child.into(),
                    description: "foreground child".to_string(),
                    run_in_background: false,
                },
            )
            .await
            .unwrap_or_abort();
        let child_task = load_events(&run.events_path)
            .iter()
            .find_map(|event| match &event.payload {
                EventV1::TaskScheduled(data)
                    if event.correlation_id.as_deref() == Some(request.as_str()) =>
                {
                    Some(data.task_id.clone())
                }
                _ => None,
            })
            .unwrap_or_abort();
        let finish_child = if queued_child {
            None
        } else {
            Some(provider_starts.recv().await.unwrap_or_abort())
        };
        let leaf = submit_native_tool(
            &coordinator,
            &run.events_path,
            supervisor_actor(),
            "shell.block",
            0,
        )
        .await;
        let (_, _finish_leaf) = starts.recv().await.unwrap_or_abort();
        clock.advance(20_000);
        // A stale leaf proves a watchdog tick ran while the real child wait was protected.
        let events = wait_for_events(&run.events_path, Duration::from_secs(2), |events| {
            events.iter().any(|event| matches!(&event.payload, EventV1::StaleDetected(data) if data.task_id.as_str() == leaf.0))
        }).await;
        assert!(!events.iter().any(|event| matches!(&event.payload, EventV1::StaleDetected(data) if data.task_id.as_str() == parent.0)));
        if let Some(finish_child) = finish_child {
            finish_child.send(()).unwrap_or_abort();
        } else {
            coordinator
                .cancel_task(&child_task, "cancel queued child")
                .await
                .unwrap_or_abort();
        }
        wait_for_events(&run.events_path, Duration::from_secs(2), |events| {
            events.iter().any(|event| match &event.payload {
                EventV1::TaskCompleted(data) => data.task_id == child_task,
                EventV1::TaskCancelled(data) => data.task_id == child_task,
                _ => false,
            })
        })
        .await;
        clock.advance(15_001);
        let events = wait_for_events(&run.events_path, Duration::from_secs(2), |events| {
            events.iter().any(|event| matches!(&event.payload, EventV1::StaleDetected(data) if data.task_id.as_str() == parent.0))
        }).await;
        let stale_for = events.iter().find_map(|event| match &event.payload {
            EventV1::StaleDetected(data) if data.task_id.as_str() == parent.0 => {
                Some(data.stale_for_ms)
            }
            _ => None,
        });
        assert_eq!(
            stale_for,
            Some(15_001),
            "{wrapper}: post-child result collection gets a fresh interval"
        );
        for response in [parent.2, leaf.2] {
            assert!(tokio::time::timeout(Duration::from_secs(2), response)
                .await
                .unwrap_or_abort()
                .unwrap_or_abort()
                .is_err());
        }
        coordinator.stop_run().await.unwrap_or_abort();
        drop(blocker);
    }
}

#[tokio::test]
async fn native_tool_scheduler_shutdown_resolves_queued_responses_without_starting() {
    for fail in [false, true] {
        let temp_dir = tempfile::tempdir().unwrap_or_abort();
        let (started, mut starts) = tokio::sync::mpsc::unbounded_channel();
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(SchedulerControlledTool {
            id: "shell.block",
            started,
        }));
        let coordinator = test_tool_lifecycle_coordinator(
            temp_dir.path(),
            Arc::new(FakeClock::new()),
            Arc::new(registry),
            Duration::ZERO,
            15_000,
            1,
            1,
        );
        let run = coordinator
            .start_run("queued_shutdown", temp_dir.path())
            .await
            .unwrap_or_abort();
        let first = submit_native_tool(
            &coordinator,
            &run.events_path,
            supervisor_actor(),
            "shell.block",
            0,
        )
        .await;
        let (_, _finish_first) = starts.recv().await.unwrap_or_abort();
        let second = submit_native_tool(
            &coordinator,
            &run.events_path,
            supervisor_actor(),
            "shell.block",
            1,
        )
        .await;
        if fail {
            coordinator
                .fail_run("controlled run failure")
                .await
                .unwrap_or_abort();
        } else {
            coordinator.stop_run().await.unwrap_or_abort();
        }
        for response in [first.2, second.2] {
            let error = tokio::time::timeout(Duration::from_secs(2), response)
                .await
                .unwrap_or_abort()
                .unwrap_or_abort()
                .err()
                .unwrap_or_abort();
            assert!(
                !error.contains("channel"),
                "respond with the shutdown reason: {error}"
            );
        }
        let events = load_events(&run.events_path);
        assert_eq!(events.iter().filter(|event| matches!(&event.payload, EventV1::TaskCancelled(data) if data.task_id.as_str() == second.0)).count(), 1);
        assert!(!events.iter().any(|event| matches!(&event.payload, EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == second.1)));
        assert!(starts.try_recv().is_err());
    }
}

#[tokio::test]
async fn native_tool_scheduler_nested_siblings_queue_and_turn_cancellation_keeps_leaf_limits() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let (started, mut starts) = tokio::sync::mpsc::unbounded_channel();
    let (provider_started, mut provider_starts) = tokio::sync::mpsc::unbounded_channel();
    let mut registry = ToolRegistry::new();
    for id in ["task", "shell.block"] {
        registry.register(Arc::new(SchedulerControlledTool {
            id,
            started: started.clone(),
        }));
    }
    let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
    config.tool_registry = Arc::new(registry);
    config.provider = Arc::new(SchedulerControlledProvider {
        started: provider_started,
    });
    config.agent_profiles = agent_profiles();
    config
        .agent_profiles
        .get_mut("alpha")
        .unwrap_or_abort()
        .toolset = vec!["task".to_string(), "shell.block".to_string()];
    config.tool_concurrency = 1;
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator
        .start_run("nested_queue_cancel", temp_dir.path())
        .await
        .unwrap_or_abort();
    let parent = submit_native_tool(
        &coordinator,
        &run.events_path,
        supervisor_actor(),
        "task",
        0,
    )
    .await;
    let (_, _finish_parent) = starts.recv().await.unwrap_or_abort();
    let leaf = submit_native_tool(
        &coordinator,
        &run.events_path,
        supervisor_actor(),
        "shell.block",
        0,
    )
    .await;
    let (_, finish_leaf) = starts.recv().await.unwrap_or_abort();
    let child = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let request = coordinator
        .request_child_agent_turn_with_model(
            supervisor_actor(),
            child.clone(),
            "child",
            None,
            None,
            ChildTaskRequestMetadata {
                parent_tool_call_id: parent.1.clone(),
                parent_session_id: run.run_id.as_str().into(),
                parent_agent_id: None,
                child_session_id: child.clone().into(),
                task_id: child.clone().into(),
                description: "foreground child".to_string(),
                run_in_background: false,
            },
        )
        .await
        .unwrap_or_abort();
    let _finish_child = provider_starts.recv().await.unwrap_or_abort();
    let actor = EventActor::new(ActorKind::Worker, Some(child));
    let first = submit_native_tool(&coordinator, &run.events_path, actor.clone(), "task", 1).await;
    let (call_id, finish_first) = starts.recv().await.unwrap_or_abort();
    assert_eq!(call_id, first.1);
    let second = submit_native_tool(&coordinator, &run.events_path, actor.clone(), "task", 2).await;
    let third = submit_native_tool(&coordinator, &run.events_path, actor.clone(), "task", 3).await;
    let child_leaf = submit_native_tool(
        &coordinator,
        &run.events_path,
        actor.clone(),
        "shell.block",
        1,
    )
    .await;
    for queued in [&second, &third, &child_leaf] {
        assert!(load_events(&run.events_path).iter().any(
            |event| matches!(&event.payload, EventV1::TaskScheduled(data)
            if data.task_id.as_str() == queued.0 && data.state == TaskScheduleState::Queued)
        ));
    }
    finish_first.send(true).unwrap_or_abort();
    assert!(first.2.await.unwrap_or_abort().is_ok());
    let (call_id, _finish_second) = starts.recv().await.unwrap_or_abort();
    assert_eq!(call_id, second.1);
    let child_task = load_events(&run.events_path)
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskScheduled(data)
                if event.correlation_id.as_deref() == Some(request.as_str())
                    && data
                        .queue_key
                        .as_deref()
                        .is_some_and(|key| key.starts_with("provider_model:")) =>
            {
                Some(data.task_id.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();
    coordinator
        .cancel_task(child_task, "cancel child turn")
        .await
        .unwrap_or_abort();
    for (task_id, call_id, response) in [second, third, child_leaf] {
        let error = tokio::time::timeout(Duration::from_secs(2), response)
            .await
            .unwrap_or_abort()
            .unwrap_or_abort()
            .err()
            .unwrap_or_abort();
        assert!(!error.contains("channel"));
        let events = load_events(&run.events_path);
        let cancelled = events.iter().find(|event| matches!(&event.payload, EventV1::TaskCancelled(data) if data.task_id.as_str() == task_id)).unwrap_or_abort();
        assert_task_event_context(cancelled, &actor, &request);
        assert_eq!(events.iter().filter(|event| matches!(&event.payload, EventV1::ToolCallFinished(data) if data.tool_call_id.as_str() == call_id)).count(), 1);
    }
    finish_leaf.send(true).unwrap_or_abort();
    assert!(leaf.2.await.unwrap_or_abort().is_ok());
    assert!(starts.try_recv().is_err());
    coordinator.stop_run().await.unwrap_or_abort();
    assert!(parent.2.await.unwrap_or_abort().is_err());
}

#[cfg(unix)]
struct SchedulerStartHooks(AtomicUsize);

#[cfg(unix)]
#[async_trait]
impl harness_core::coord::LifecycleHookCommandExecutor for SchedulerStartHooks {
    async fn execute(
        &self,
        _invocation: harness_core::coord::LifecycleHookCommandInvocation,
    ) -> Result<harness_core::coord::LifecycleHookCommandOutput, String> {
        use std::os::unix::process::ExitStatusExt;
        if self.0.fetch_add(1, Ordering::SeqCst) == 1 {
            Err("controlled start hook failure".to_string())
        } else {
            Ok(harness_core::coord::LifecycleHookCommandOutput {
                status: std::process::ExitStatus::from_raw(0),
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn native_tool_scheduler_start_hook_failure_releases_slot_and_drains_next_call() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let (started, mut starts) = tokio::sync::mpsc::unbounded_channel();
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(SchedulerControlledTool {
        id: "shell.block",
        started,
    }));
    let hooks = Arc::new(SchedulerStartHooks(AtomicUsize::new(0)));
    let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
    config.tool_registry = Arc::new(registry);
    config.tool_concurrency = 1;
    config.hook_command_executor = Arc::<SchedulerStartHooks>::clone(&hooks);
    config.hook_runtime_config = HookRuntimeConfig {
        hooks: HooksConfig {
            lifecycle: vec![LifecycleHookConfig {
                id: Some("controlled-start".to_string()),
                event: HookLifecycleEvent::ToolCallStarted,
                command: vec!["bash".to_string()],
                cwd: Some(".".to_string()),
                timeout_ms: 1_000,
                critical: true,
                env: BTreeMap::new(),
            }],
        },
        shell_allowlist: ShellAllowlist {
            executables: vec!["bash".to_string()],
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        },
        suppress_execution: false,
    };
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator
        .start_run("queued_hook_failure", temp_dir.path())
        .await
        .unwrap_or_abort();
    let first = submit_native_tool(
        &coordinator,
        &run.events_path,
        supervisor_actor(),
        "shell.block",
        0,
    )
    .await;
    let (_, finish_first) = starts.recv().await.unwrap_or_abort();
    let second = submit_native_tool(
        &coordinator,
        &run.events_path,
        supervisor_actor(),
        "shell.block",
        1,
    )
    .await;
    let third = submit_native_tool(
        &coordinator,
        &run.events_path,
        supervisor_actor(),
        "shell.block",
        2,
    )
    .await;
    assert_eq!(
        hooks.0.load(Ordering::SeqCst),
        1,
        "queued calls must not run start hooks"
    );
    finish_first.send(true).unwrap_or_abort();
    assert!(first.2.await.unwrap_or_abort().is_ok());
    let error = tokio::time::timeout(Duration::from_secs(2), second.2)
        .await
        .unwrap_or_abort()
        .unwrap_or_abort()
        .err()
        .unwrap_or_abort();
    assert!(error.contains("controlled start hook failure"));
    let (call_id, finish_third) = starts.recv().await.unwrap_or_abort();
    assert_eq!(
        call_id, third.1,
        "failed start must not execute the second tool"
    );
    finish_third.send(true).unwrap_or_abort();
    assert!(third.2.await.unwrap_or_abort().is_ok());
    let events = load_events(&run.events_path);
    assert_eq!(events.iter().filter(|event| matches!(&event.payload, EventV1::TaskCancelled(data) if data.task_id.as_str() == second.0)).count(), 1);
    assert_eq!(events.iter().filter(|event| matches!(&event.payload, EventV1::ToolCallFinished(data) if data.tool_call_id.as_str() == second.1)).count(), 1);
    assert_eq!(hooks.0.load(Ordering::SeqCst), 3);
    coordinator.stop_run().await.unwrap_or_abort();
}
