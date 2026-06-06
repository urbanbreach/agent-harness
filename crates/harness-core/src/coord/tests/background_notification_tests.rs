use super::*;

#[path = "background_notification_delivery_tests.rs"]
mod background_notification_delivery_tests;
pub(super) use background_notification_delivery_tests::{
    background_task_completion_caps_and_redacts_description_and_summary as delivery_background_task_completion_caps_and_redacts_description_and_summary,
    background_task_completion_notifies_parent_once_and_queues_active_parent as delivery_background_task_completion_notifies_parent_once_and_queues_active_parent,
};

pub(super) async fn background_task_completion_schedules_pending_wakeup_when_parent_finishes() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = background_task_notification_run_state(temp_dir.path(), "run_bg_race");
    let (job_tx, _job_rx) = mpsc::channel(4);
    let terminal_event = append_payload_event_with_correlation(
        &clock,
        &redactor,
        &mut run_state,
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        Some("task:task_child_terminal".to_string()),
        Some("req_child".to_string()),
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_child_terminal".to_string(),
            result_summary: "child output".to_string(),
            result_digest: "digest-child".to_string(),
            metadata: None,
        }),
    )
    .expect("append terminal completion");

    append_background_task_notification_and_schedule(
        &clock,
        &redactor,
        Arc::new(TokioLifecycleHookCommandExecutor),
        job_tx.clone(),
        &mut run_state,
        Default::default(),
        Default::default(),
        run_state_test_config(temp_dir.path()).provider,
        test_tool_registry(),
        Some(background_child_task(true)),
        &terminal_event,
        BackgroundTaskNotificationStatus::Completed,
        "child output",
    )
    .await
    .expect("active parent stores pending wakeup");

    let pending = run_state
        .pending_agent_wakeups
        .get("agent_parent")
        .expect("pending parent wakeup")
        .first()
        .cloned()
        .expect("one pending wakeup");
    run_state.running_agent_turns.clear();

    schedule_pending_agent_wakeups_for_idle_agent(
        &clock,
        &redactor,
        Arc::new(TokioLifecycleHookCommandExecutor),
        job_tx,
        &mut run_state,
        Default::default(),
        Default::default(),
        run_state_test_config(temp_dir.path()).provider,
        test_tool_registry(),
        "agent_parent",
    )
    .await
    .expect("schedule pending wakeup after parent finish");

    assert!(run_state.pending_agent_wakeups.is_empty());
    let running = run_state
        .running_agent_turns
        .values()
        .find(|running| running.agent_id == "agent_parent")
        .expect("pending wakeup starts follow-up parent turn");
    assert_eq!(running.request_id, pending.request_id);
    assert_eq!(running.request_prompt, pending.notification_text);
}

pub(super) async fn background_task_completion_queues_parent_when_parent_is_idle() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = background_task_notification_run_state(temp_dir.path(), "run_bg_idle");
    run_state.running_agent_turns.clear();
    let (job_tx, _job_rx) = mpsc::channel(4);
    let terminal_event = append_payload_event_with_correlation(
        &clock,
        &redactor,
        &mut run_state,
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        Some("task:task_child_terminal".to_string()),
        Some("req_child".to_string()),
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_child_terminal".to_string(),
            result_summary: "child output".to_string(),
            result_digest: "digest-child".to_string(),
            metadata: None,
        }),
    )
    .expect("append terminal completion");

    append_background_task_notification_and_schedule(
        &clock,
        &redactor,
        Arc::new(TokioLifecycleHookCommandExecutor),
        job_tx,
        &mut run_state,
        Default::default(),
        Default::default(),
        run_state_test_config(temp_dir.path()).provider,
        test_tool_registry(),
        Some(background_child_task(true)),
        &terminal_event,
        BackgroundTaskNotificationStatus::Completed,
        "child output",
    )
    .await
    .expect("schedule idle parent wakeup");

    assert!(run_state.pending_agent_wakeups.is_empty());
    assert!(run_state.queued_agent_turns.is_empty());
    let running = run_state
        .running_agent_turns
        .values()
        .find(|running| running.agent_id == "agent_parent")
        .expect("idle parent wakeup starts a new turn when capacity is available");
    assert_eq!(running.profile_name, "parent");
    assert_eq!(running.model_ref, "mock:parent-model");
    assert!(running
        .request_prompt
        .contains("[BACKGROUND TASK COMPLETED]"));
    assert!(running
        .request_prompt
        .contains("background_output(request_id=\"req_child\")"));
    assert!(running
        .request_prompt
        .contains("task(session_id=\"agent_child\")"));

    let started = read_events(&run_state.info.events_path)
        .into_iter()
        .any(|event| match event.payload {
            EventV1::TaskScheduled(payload) => payload.state == TaskScheduleState::Started,
            _ => false,
        });
    assert!(started, "idle parent wakeup records a started turn");
}

pub(super) async fn background_task_completion_sync_spawn_does_not_notify() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = background_task_notification_run_state(temp_dir.path(), "run_sync_none");
    let (job_tx, _job_rx) = mpsc::channel(4);
    let terminal_event = append_payload_event_with_correlation(
        &clock,
        &redactor,
        &mut run_state,
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        Some("task:task_child_terminal".to_string()),
        Some("req_child".to_string()),
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_child_terminal".to_string(),
            result_summary: "sync child output".to_string(),
            result_digest: "digest-child".to_string(),
            metadata: None,
        }),
    )
    .expect("append terminal completion");

    append_background_task_notification_and_schedule(
        &clock,
        &redactor,
        Arc::new(TokioLifecycleHookCommandExecutor),
        job_tx,
        &mut run_state,
        Default::default(),
        Default::default(),
        run_state_test_config(temp_dir.path()).provider,
        test_tool_registry(),
        Some(background_child_task(false)),
        &terminal_event,
        BackgroundTaskNotificationStatus::Completed,
        "sync child output",
    )
    .await
    .expect("sync child ignored");

    let events = read_events(&run_state.info.events_path);
    assert!(!events
        .iter()
        .any(|event| matches!(event.payload, EventV1::BackgroundTaskNotification(_))));
    assert!(run_state.queued_agent_turns.is_empty());
}

pub(super) async fn background_task_completion_records_pending_notification_when_parent_cannot_wake(
) {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = background_task_notification_run_state(temp_dir.path(), "run_bg_pending");
    let (job_tx, _job_rx) = mpsc::channel(4);
    let terminal_event = append_payload_event_with_correlation(
        &clock,
        &redactor,
        &mut run_state,
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        Some("task:task_child_terminal".to_string()),
        Some("req_child".to_string()),
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_child_terminal".to_string(),
            result_summary: "child output".to_string(),
            result_digest: "digest-child".to_string(),
            metadata: None,
        }),
    )
    .expect("append terminal completion");
    let mut child_task = background_child_task(true);
    child_task.parent_agent_id = Some("missing_parent_agent".to_string());

    append_background_task_notification_and_schedule(
        &clock,
        &redactor,
        Arc::new(TokioLifecycleHookCommandExecutor),
        job_tx,
        &mut run_state,
        Default::default(),
        Default::default(),
        run_state_test_config(temp_dir.path()).provider,
        test_tool_registry(),
        Some(child_task),
        &terminal_event,
        BackgroundTaskNotificationStatus::Completed,
        "child output",
    )
    .await
    .expect("pending notification is durable");

    let events = read_events(&run_state.info.events_path);
    let notification = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::BackgroundTaskNotification(payload) => Some(payload),
            _ => None,
        })
        .expect("pending notification event");
    assert_eq!(
        notification.parent_agent_id.as_deref(),
        Some("missing_parent_agent")
    );
    assert_eq!(notification.delivered_turn_request_id, None);
    assert!(run_state.queued_agent_turns.is_empty());
}

pub(super) async fn background_task_completion_cancellation_and_late_terminal_do_not_duplicate() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = background_task_notification_run_state(temp_dir.path(), "run_bg_cancel");
    let (job_tx, _job_rx) = mpsc::channel(4);
    let child_task = background_child_task(true);
    let terminal_event = append_payload_event_with_correlation(
        &clock,
        &redactor,
        &mut run_state,
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        Some("task:task_child_terminal".to_string()),
        Some("req_child".to_string()),
        EventV1::TaskCancelled(TaskCancelledEvent {
            task_id: "task_child_terminal".to_string(),
            reason: "provider failed closed".to_string(),
            task_scope: Some(crate::event::TaskTerminalScope::AgentTurn),
        }),
    )
    .expect("append cancellation");
    append_background_task_notification_and_schedule(
        &clock,
        &redactor,
        Arc::new(TokioLifecycleHookCommandExecutor),
        job_tx.clone(),
        &mut run_state,
        Default::default(),
        Default::default(),
        run_state_test_config(temp_dir.path()).provider,
        test_tool_registry(),
        Some(child_task.clone()),
        &terminal_event,
        BackgroundTaskNotificationStatus::Failed,
        "provider failed closed",
    )
    .await
    .expect("failed child notifies");

    let late_terminal = append_payload_event_with_correlation(
        &clock,
        &redactor,
        &mut run_state,
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        Some("task:task_child_terminal_late".to_string()),
        Some("req_child".to_string()),
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_child_terminal_late".to_string(),
            result_summary: "late output".to_string(),
            result_digest: "digest-late".to_string(),
            metadata: None,
        }),
    )
    .expect("append duplicate late terminal");
    append_background_task_notification_and_schedule(
        &clock,
        &redactor,
        Arc::new(TokioLifecycleHookCommandExecutor),
        job_tx,
        &mut run_state,
        Default::default(),
        Default::default(),
        run_state_test_config(temp_dir.path()).provider,
        test_tool_registry(),
        Some(child_task),
        &late_terminal,
        BackgroundTaskNotificationStatus::Completed,
        "late output",
    )
    .await
    .expect("late duplicate ignored");

    let events = read_events(&run_state.info.events_path);
    let notifications = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::BackgroundTaskNotification(payload) => Some(payload),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(notifications.len(), 1);
    assert_eq!(
        notifications[0].status,
        BackgroundTaskNotificationStatus::Failed
    );
    assert_eq!(notifications[0].summary, "provider failed closed");
}

pub(super) fn background_task_completion_replay_projection_is_side_effect_free() {
    let run_id = "run_bg_replay";
    let events = [restore_fixture_event(
        run_id,
        1,
        EventActor::new(ActorKind::System, Some("coordinator".to_string())),
        Some("req_child"),
        EventV1::BackgroundTaskNotification(crate::event::BackgroundTaskNotificationEvent {
            parent_session_id: run_id.to_string(),
            parent_agent_id: Some("agent_parent".to_string()),
            child_session_id: "agent_child".to_string(),
            child_request_id: "req_child".to_string(),
            task_id: "agent_child".to_string(),
            description: "Summarize the repository".to_string(),
            status: BackgroundTaskNotificationStatus::Completed,
            summary: "child summary".to_string(),
            terminal_event_id: "evt-terminal".to_string(),
            terminal_task_id: "task_child_terminal".to_string(),
            delivered_turn_request_id: Some("req_parent_wakeup".to_string()),
        }),
    )];

    let summary = crate::proj::project_run_summary(events.iter()).expect("project summary");
    let timeline = crate::proj::project_timeline_index(events.iter()).expect("project timeline");

    assert_eq!(summary.counts.total_events, 1);
    assert_eq!(
        summary
            .counts
            .by_type
            .get("background_task_notification")
            .copied(),
        Some(1)
    );
    assert_eq!(timeline.events.len(), 1);
}

fn run_state_test_config(session_dir: &Path) -> CoordinatorConfig {
    let mut config = test_config(session_dir);
    config
        .agent_profiles
        .insert("parent".to_string(), parent_agent_profile());
    config
        .agent_profiles
        .insert("child".to_string(), child_agent_profile());
    config
}

fn background_task_notification_run_state(session_dir: &Path, run_id: &str) -> RunState {
    let mut run_state = test_run_state(session_dir, run_id);
    run_state
        .agents
        .insert("agent_parent".to_string(), parent_agent_profile());
    run_state
        .agents
        .insert("agent_child".to_string(), child_agent_profile());
    run_state.running_agent_turns.insert(
        "task_parent_active".to_string(),
        RunningAgentTurn {
            agent_id: "agent_parent".to_string(),
            request_id: "req_parent_active".to_string(),
            request_prompt: "active parent turn".to_string(),
            profile_name: "parent".to_string(),
            model_ref: "mock:parent-model".to_string(),
            model_settings: Default::default(),
            category: Some("parent".to_string()),
            queue_key: ConcurrencyKey::ProviderModel {
                provider_id: "mock".to_string(),
                model_id: "parent-model".to_string(),
            },
            cancellation_token: CancellationToken::new(),
            started_mono_ms: 0,
            hook_executions: Vec::new(),
            latest_provider_usage: None,
            latest_provider_request_id: None,
            latest_assistant_output: None,
            latest_provider_id: None,
            latest_model_id: None,
            child_task: None,
        },
    );
    run_state
}

fn parent_agent_profile() -> AgentProfile {
    AgentProfile {
        name: "parent".to_string(),
        category: "parent".to_string(),
        model_ref: "mock:parent-model".to_string(),
        model_ref_explicit: true,
        system_prompt: "parent system".to_string(),
        cache_retention: Default::default(),
        max_iters: Some(1),
        temperature: None,
        tool_failure_mode: crate::config::ToolFailureMode::FailTurn,
        toolset: vec!["background_output".to_string()],
    }
}

fn child_agent_profile() -> AgentProfile {
    AgentProfile {
        name: "child".to_string(),
        category: "child".to_string(),
        model_ref: "mock:child-model".to_string(),
        model_ref_explicit: true,
        system_prompt: "child system".to_string(),
        cache_retention: Default::default(),
        max_iters: Some(1),
        temperature: None,
        tool_failure_mode: crate::config::ToolFailureMode::FailTurn,
        toolset: vec!["read".to_string()],
    }
}

fn background_child_task(run_in_background: bool) -> ChildTaskTurnState {
    ChildTaskTurnState {
        parent_tool_call_id: "toolcall_parent_task".to_string(),
        parent_session_id: "run_bg_once".to_string(),
        parent_agent_id: Some("agent_parent".to_string()),
        child_session_id: "agent_child".to_string(),
        child_request_id: "req_child".to_string(),
        task_id: "agent_child".to_string(),
        description: "Summarize the repository".to_string(),
        run_in_background,
    }
}
