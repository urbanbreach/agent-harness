use super::*;
use crate::UnwrapOrAbort;

pub(crate) async fn background_task_completion_notifies_parent_once_and_queues_active_parent() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = background_task_notification_run_state(temp_dir.path(), "run_bg_once");
    let (job_tx, _job_rx) = mpsc::channel(4);
    let child_task = background_child_task(true);
    let long_summary = format!("{} full-output-tail", "child output ".repeat(80));
    let terminal_event = append_payload_event_with_correlation(
        &clock,
        &redactor,
        &mut run_state,
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        Some("task:task_child_terminal".to_string()),
        Some("req_child".to_string()),
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_child_terminal".to_string().into(),
            result_summary: long_summary.clone(),
            result_digest: "digest-child".to_string(),
            metadata: None,
        }),
    )
    .unwrap_or_abort();

    append_background_task_notification_and_schedule(
        &clock,
        &redactor,
        Arc::new(TokioLifecycleHookCommandExecutor),
        job_tx.clone(),
        &mut run_state,
        Default::default(),
        Default::default(),
        crate::config::ProviderRetryRuntimeConfig::default(),
        run_state_test_config(temp_dir.path()).provider,
        test_tool_registry(),
        Some(child_task.clone()),
        &terminal_event,
        BackgroundTaskNotificationStatus::Completed,
        &long_summary,
    )
    .await
    .unwrap_or_abort();
    append_background_task_notification_and_schedule(
        &clock,
        &redactor,
        Arc::new(TokioLifecycleHookCommandExecutor),
        job_tx,
        &mut run_state,
        Default::default(),
        Default::default(),
        crate::config::ProviderRetryRuntimeConfig::default(),
        run_state_test_config(temp_dir.path()).provider,
        test_tool_registry(),
        Some(child_task),
        &terminal_event,
        BackgroundTaskNotificationStatus::Completed,
        &long_summary,
    )
    .await
    .unwrap_or_abort();

    let events = read_events(&run_state.info.events_path);
    let notifications = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::BackgroundTaskNotification(payload) => Some(payload),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(notifications.len(), 1);
    let notification = notifications[0];
    assert_eq!(notification.task_id.as_str(), "agent_child");
    assert_eq!(notification.child_request_id, "req_child");
    assert_eq!(notification.description, "Summarize the repository");
    assert_eq!(
        notification.status,
        BackgroundTaskNotificationStatus::Completed
    );
    assert_eq!(notification.terminal_event_id, terminal_event.event_id);
    assert_eq!(notification.terminal_task_id, "task_child_terminal");
    assert!(notification.summary.chars().count() <= 512);
    assert!(!notification.summary.contains("full-output-tail"));

    let reminder = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::UserMessageSubmitted(payload)
                if payload.request_id.as_str()
                    == notification
                        .delivered_turn_request_id
                        .clone()
                        .unwrap_or_abort() =>
            {
                Some(payload.text.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();
    assert!(reminder.contains("[BACKGROUND TASK COMPLETED]"));
    assert!(reminder.contains("ID: agent_child"));
    assert!(reminder.contains("Request ID: req_child"));
    assert!(reminder.contains("Description: Summarize the repository"));
    assert!(reminder.contains("Status: completed"));
    assert!(reminder.contains("background_output(request_id=\"req_child\")"));
    assert!(reminder.contains("task(session_id=\"agent_child\")"));
    assert!(!reminder.contains("full-output-tail"));

    assert!(run_state.queued_agent_turns.is_empty());
    let pending = run_state
        .pending_agent_wakeups
        .get("agent_parent")
        .unwrap_or_abort();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].notification_text, reminder);
    assert_eq!(
        Some(pending[0].request_id.as_str()),
        notification.delivered_turn_request_id.as_deref()
    );
}

pub(crate) async fn background_task_completion_caps_and_redacts_description_and_summary() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = background_task_notification_run_state(temp_dir.path(), "run_bg_redact");
    let (job_tx, _job_rx) = mpsc::channel(4);
    let terminal_event = append_payload_event_with_correlation(
        &clock,
        &redactor,
        &mut run_state,
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        Some("task:task_child_terminal".to_string()),
        Some("req_child".to_string()),
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_child_terminal".to_string().into(),
            result_summary: "terminal summary".to_string(),
            result_digest: "digest-child".to_string(),
            metadata: None,
        }),
    )
    .unwrap_or_abort();
    let mut child = background_child_task(true);
    child.description = format!("{} sk-SECRETSECRETSECRETSECRET", "d".repeat(400));
    let summary = format!("{} Bearer token.secret", "s".repeat(900));

    append_background_task_notification_and_schedule(
        &clock,
        &redactor,
        Arc::new(TokioLifecycleHookCommandExecutor),
        job_tx,
        &mut run_state,
        Default::default(),
        Default::default(),
        crate::config::ProviderRetryRuntimeConfig::default(),
        run_state_test_config(temp_dir.path()).provider,
        test_tool_registry(),
        Some(child),
        &terminal_event,
        BackgroundTaskNotificationStatus::Completed,
        &summary,
    )
    .await
    .unwrap_or_abort();

    let notification = read_events(&run_state.info.events_path)
        .into_iter()
        .find_map(|event| match event.payload {
            EventV1::BackgroundTaskNotification(payload) => Some(payload),
            _ => None,
        })
        .unwrap_or_abort();
    assert!(notification.description.chars().count() <= 161);
    assert!(notification.summary.chars().count() <= 512);
    assert!(!notification.description.contains("sk-SECRET"));
    assert!(!notification.summary.contains("token.secret"));
    assert!(notification.description.contains("…"));
    assert!(notification.summary.contains("…"));

    let reminder = read_events(&run_state.info.events_path)
        .into_iter()
        .find_map(|event| match event.payload {
            EventV1::UserMessageSubmitted(payload)
                if Some(payload.request_id.as_str())
                    == notification.delivered_turn_request_id.as_deref() =>
            {
                Some(payload.text)
            }
            _ => None,
        })
        .unwrap_or_abort();
    assert!(!reminder.contains("sk-SECRET"));
    assert!(!reminder.contains("token.secret"));
}
