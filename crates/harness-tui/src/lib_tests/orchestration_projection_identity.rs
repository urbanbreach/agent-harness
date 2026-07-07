use super::*;
use crate::UnwrapOrAbort;

pub(super) fn orchestration_projection_resolves_owner_labels() {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_worker".to_string(),
            profile: "researcher".to_string(),
            parent_agent_id: None,
        }),
    ));

    app.ingest_event(envelope_with_actor(
        2,
        Some("req_worker"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_worker".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("agent:queued".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        3,
        None,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Supervisor, None),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_supervisor".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:supervisor".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        4,
        None,
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("coordinator".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_system".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("tool:shell.run".to_string()),
        }),
    ));

    let summary = app.orchestration_summary();
    assert_eq!(
        summary,
        crate::app::OrchestrationSummary {
            active_agents: 1,
            queued: 2,
            running: 1,
            stale: 0,
        }
    );

    let rows = app.orchestration_visible_rows();
    assert_eq!(
        rows.iter()
            .map(|row| row.task_id.as_str())
            .collect::<Vec<_>>(),
        vec!["task_supervisor", "task_system", "task_worker"]
    );

    let worker = rows
        .iter()
        .find(|row| row.task_id == "task_worker")
        .unwrap();
    assert_eq!(
        app.orchestration_owner_labels(worker),
        crate::app::OrchestrationOwnerLabels {
            label: "agent_worker".to_string(),
            profile: "researcher".to_string(),
        }
    );

    let supervisor = rows
        .iter()
        .find(|row| row.task_id == "task_supervisor")
        .unwrap();
    assert_eq!(
        app.orchestration_owner_labels(supervisor),
        crate::app::OrchestrationOwnerLabels {
            label: "supervisor".to_string(),
            profile: "n/a".to_string(),
        }
    );

    let system = rows
        .iter()
        .find(|row| row.task_id == "task_system")
        .unwrap();
    assert_eq!(
        app.orchestration_owner_labels(system),
        crate::app::OrchestrationOwnerLabels {
            label: "system".to_string(),
            profile: "n/a".to_string(),
        }
    );
}

pub(super) fn orchestration_projection_ignores_duplicate_seq_events() {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_worker".to_string(),
            profile: "researcher".to_string(),
            parent_agent_id: None,
        }),
    ));
    app.ingest_event(envelope_with_actor(
        2,
        Some("req_worker"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_dup".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:running".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        3,
        Some("req_worker"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::StaleDetected(harness_core::event::StaleDetectedEvent {
            task_id: "task_dup".to_string(),
            stale_for_ms: 3001,
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 1,
            queued: 0,
            running: 0,
            stale: 1,
        }
    );
    assert_eq!(app.orchestration_visible_rows().len(), 1);
    assert_eq!(
        app.orchestration_latest_warning(),
        Some("stale for 3001 ms")
    );

    app.ingest_event(envelope_with_actor(
        1,
        None,
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("coordinator".to_string()),
        ),
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_worker".to_string(),
            profile: "rewritten".to_string(),
            parent_agent_id: None,
        }),
    ));
    app.ingest_event(envelope_with_actor(
        2,
        Some("req_worker"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_dup".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("agent:queued".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        3,
        Some("req_worker"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::StaleDetected(harness_core::event::StaleDetectedEvent {
            task_id: "task_dup".to_string(),
            stale_for_ms: 9999,
        }),
    ));

    assert_eq!(app.events.len(), 3);
    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 1,
            queued: 0,
            running: 0,
            stale: 1,
        }
    );
    let rows = app.orchestration_visible_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, crate::app::OrchestrationTaskState::Stale);
    assert_eq!(rows[0].queue_key.as_deref(), Some("agent:running"));
    assert_eq!(
        app.orchestration_latest_warning(),
        Some("stale for 3001 ms")
    );
    assert_eq!(
        app.orchestration_owner_labels(&rows[0]),
        crate::app::OrchestrationOwnerLabels {
            label: "agent_worker".to_string(),
            profile: "researcher".to_string(),
        }
    );
}

pub(super) fn orchestration_projection_preserves_background_notification_terminal_states() {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("background_task_notification:req_failed"),
        harness_core::event::EventV1::BackgroundTaskNotification(
            harness_core::event::BackgroundTaskNotificationEvent {
                parent_session_id: "agent_parent".to_string(),
                parent_agent_id: Some("agent_parent".to_string()),
                child_session_id: "agent_failed".to_string(),
                child_request_id: "req_failed".to_string(),
                task_id: "task_failed".to_string(),
                description: "failed child".to_string(),
                status: harness_core::event::BackgroundTaskNotificationStatus::Failed,
                summary: "provider failed closed".to_string(),
                terminal_event_id: "evt-terminal-failed".to_string(),
                terminal_task_id: "task_failed".to_string(),
                delivered_turn_request_id: Some("req_parent_notice".to_string()),
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("background_task_notification:req_timeout"),
        harness_core::event::EventV1::BackgroundTaskNotification(
            harness_core::event::BackgroundTaskNotificationEvent {
                parent_session_id: "agent_parent".to_string(),
                parent_agent_id: Some("agent_parent".to_string()),
                child_session_id: "agent_timeout".to_string(),
                child_request_id: "req_timeout".to_string(),
                task_id: "task_timeout".to_string(),
                description: "timed out child".to_string(),
                status: harness_core::event::BackgroundTaskNotificationStatus::TimedOut,
                summary: "child timed out".to_string(),
                terminal_event_id: "evt-terminal-timeout".to_string(),
                terminal_task_id: "task_timeout".to_string(),
                delivered_turn_request_id: None,
            },
        ),
    ));

    let rows = app.orchestration_visible_rows();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows.iter()
            .map(|row| (row.task_id.as_str(), row.warning.as_deref(), row.state))
            .collect::<Vec<_>>(),
        vec![
            (
                "task_timeout",
                Some("timed out"),
                crate::app::OrchestrationTaskState::TimedOut,
            ),
            (
                "task_failed",
                Some("failed"),
                crate::app::OrchestrationTaskState::Failed,
            ),
        ]
    );
    assert_eq!(app.orchestration_latest_warning(), Some("timed out"));
    assert!(app.operator_rail_has_sections());
}

pub(super) fn background_notification_projects_chat_reminder_without_duplicate_user_event() {
    let mut app = app::AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_parent".to_string(),
            profile: "build".to_string(),
            parent_agent_id: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some("background_task_notification:req_child"),
        harness_core::event::EventV1::BackgroundTaskNotification(
            harness_core::event::BackgroundTaskNotificationEvent {
                parent_session_id: "agent_parent".to_string(),
                parent_agent_id: Some("agent_parent".to_string()),
                child_session_id: "agent_child".to_string(),
                child_request_id: "req_child".to_string(),
                task_id: "agent_child".to_string(),
                description: "summarize README \u{1b}]52;c;secret\u{7}".to_string(),
                status: harness_core::event::BackgroundTaskNotificationStatus::Completed,
                summary: r#"{"sessionId":"term-1","cols":80,"token":"secret"}"#.to_string(),
                terminal_event_id: "evt-terminal-child".to_string(),
                terminal_task_id: "agent_child".to_string(),
                delivered_turn_request_id: Some("req_parent_wakeup".to_string()),
            },
        ),
    ));

    assert_eq!(app.activities.len(), 1);
    let activity = &app.activities[0];
    assert_eq!(activity.request_id, "req_parent_wakeup");
    assert_eq!(activity.status, app::ActivityStatus::Queued);
    assert_eq!(activity.profile_label, "build");
    let reminder = activity.user_message.as_ref().unwrap_or_abort();
    assert!(reminder.text.contains("[BACKGROUND TASK COMPLETED]"));
    assert!(reminder.text.contains("ID: agent_child"));
    assert!(!reminder.text.contains("summarize README"));
    assert!(!reminder.text.contains("sessionId"));
    assert!(!reminder.text.contains("secret"));
    assert!(!reminder
        .text
        .chars()
        .any(|ch| ch.is_control() && ch != '\n'));
    assert!(reminder
        .text
        .contains("background_output(request_id=\"req_child\")"));
    assert!(reminder.text.contains("task(session_id=\"agent_child\")"));

    app.ingest_event(envelope(
        3,
        Some("req_parent_wakeup"),
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_parent_wakeup".to_string(),
                text: "<system-reminder>canonical coordinator wakeup</system-reminder>".to_string(),
            },
        ),
    ));

    assert_eq!(app.activities.len(), 1);
    assert_eq!(
        app.activities[0]
            .user_message
            .as_ref()
            .unwrap_or_abort()
            .text,
        "<system-reminder>canonical coordinator wakeup</system-reminder>"
    );
    assert_eq!(app.activities[0].status, app::ActivityStatus::Queued);

    app.ingest_event(envelope_with_actor(
        4,
        Some("req_parent_wakeup"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_parent".to_string()),
        ),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_parent_wakeup".to_string(),
                provider_id: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "canonical coordinator wakeup".to_string(),
                request_digest: "digest-parent-wakeup".to_string(),
                metadata: None,
            },
        ),
    ));

    assert_eq!(app.activities[0].status, app::ActivityStatus::Streaming);
    assert_eq!(app.activities[0].profile_label, "build");
}

pub(super) fn operator_sidebar_shows_running_child_turn_before_task_tool_finishes() {
    let mut app = app::AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_parent".to_string(),
            profile: "build".to_string(),
            parent_agent_id: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        None,
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_child".to_string(),
            profile: "explore".to_string(),
            parent_agent_id: Some("agent_parent".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        3,
        Some("req_child"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_child".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_child_turn".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("provider_model:mock:model-1".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        4,
        Some("req_child"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_child".to_string()),
        ),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_child".to_string(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "inspect task behavior".to_string(),
                request_digest: "digest-child".to_string(),
                metadata: None,
            },
        ),
    ));

    assert!(app.operator_rail_has_sections());
    let sidebar = operator_sidebar_text(&app);
    assert!(sidebar.contains("▼ Subagents"));
    assert!(sidebar.contains("• ⠋ Explore Task"));
}
