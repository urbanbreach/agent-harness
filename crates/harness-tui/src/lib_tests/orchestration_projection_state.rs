use super::*;

pub(super) fn orchestration_projection_tracks_queued_started_completed_counts() {
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
        Some("req_worker_primary"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_worker_primary".to_string().into(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("agent:queued:primary".to_string()),
            metadata: None,
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 1,
            queued: 1,
            running: 0,
            stale: 0,
        }
    );
    let rows = app.orchestration_visible_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (
            rows[0].task_id.as_str(),
            rows[0].queue_key.as_deref(),
            rows[0].warning.as_deref(),
            rows[0].state,
        ),
        (
            "task_worker_primary",
            Some("agent:queued:primary"),
            None,
            crate::app::OrchestrationTaskState::Queued,
        )
    );

    app.ingest_event(envelope_with_actor(
        3,
        Some("req_worker_primary"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_worker_primary".to_string().into(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:running:primary".to_string()),
            metadata: None,
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 1,
            queued: 0,
            running: 1,
            stale: 0,
        }
    );
    let rows = app.orchestration_visible_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (
            rows[0].task_id.as_str(),
            rows[0].queue_key.as_deref(),
            rows[0].warning.as_deref(),
            rows[0].state,
        ),
        (
            "task_worker_primary",
            Some("agent:running:primary"),
            None,
            crate::app::OrchestrationTaskState::Running,
        )
    );

    app.ingest_event(envelope_with_actor(
        4,
        Some("req_worker_secondary"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_worker_secondary".to_string().into(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("agent:queued:secondary".to_string()),
            metadata: None,
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 1,
            queued: 1,
            running: 1,
            stale: 0,
        },
        "active_agents must count unique worker owners only"
    );
    assert_eq!(
        app.orchestration_visible_rows()
            .iter()
            .map(|row| row.task_id.as_str())
            .collect::<Vec<_>>(),
        vec!["task_worker_primary", "task_worker_secondary"]
    );

    app.ingest_event(envelope_with_actor(
        5,
        Some("req_worker_primary"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
            task_id: "task_worker_primary".to_string().into(),
            result_summary: "primary completed".to_string(),
            result_digest: "digest-primary".to_string(),
            metadata: None,
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 1,
            queued: 1,
            running: 0,
            stale: 0,
        }
    );

    app.ingest_event(envelope_with_actor(
        6,
        Some("req_worker_secondary"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
            task_id: "task_worker_secondary".to_string().into(),
            result_summary: "secondary completed".to_string(),
            result_digest: "digest-secondary".to_string(),
            metadata: None,
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 0,
            queued: 0,
            running: 0,
            stale: 0,
        }
    );
    assert_eq!(
        app.orchestration_visible_rows()
            .iter()
            .map(|row| row.task_id.as_str())
            .collect::<Vec<_>>(),
        vec!["task_worker_secondary", "task_worker_primary"]
    );

    app.ingest_event(envelope_with_actor(
        7,
        None,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Supervisor, None),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_supervisor_only".to_string().into(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:supervisor".to_string()),
            metadata: None,
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 0,
            queued: 0,
            running: 1,
            stale: 0,
        },
        "non-worker rows must not contribute to active_agents"
    );
}

pub(super) fn orchestration_projection_tracks_stale_then_late_result() {
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
        Some("req_stale"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_stale".to_string().into(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:running:stale".to_string()),
            metadata: None,
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 1,
            queued: 0,
            running: 1,
            stale: 0,
        }
    );

    app.ingest_event(envelope_with_actor(
        3,
        Some("req_stale"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::StaleDetected(harness_core::event::StaleDetectedEvent {
            task_id: "task_stale".to_string().into(),
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
    let rows = app.orchestration_visible_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (
            rows[0].task_id.as_str(),
            rows[0].queue_key.as_deref(),
            rows[0].warning.as_deref(),
            rows[0].state,
        ),
        (
            "task_stale",
            Some("agent:running:stale"),
            Some("stale for 3001 ms"),
            crate::app::OrchestrationTaskState::Stale,
        )
    );

    app.ingest_event(envelope_with_actor(
        4,
        Some("req_stale"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskResultLate(harness_core::event::TaskResultLateEvent {
            task_id: "task_stale".to_string().into(),
            result_digest: "digest-late".to_string(),
        }),
    ));

    assert_eq!(
        app.orchestration_summary(),
        crate::app::OrchestrationSummary {
            active_agents: 0,
            queued: 0,
            running: 0,
            stale: 0,
        }
    );
    let rows = app.orchestration_visible_rows();
    assert_eq!(
        rows.len(),
        1,
        "late result must update the stale row in place"
    );
    assert_eq!(
        (
            rows[0].task_id.as_str(),
            rows[0].queue_key.as_deref(),
            rows[0].warning.as_deref(),
            rows[0].state,
        ),
        (
            "task_stale",
            Some("agent:running:stale"),
            Some("late result after stale cancellation"),
            crate::app::OrchestrationTaskState::LateResult,
        )
    );
    assert_eq!(
        app.orchestration_latest_warning(),
        Some("late result after stale cancellation")
    );
}

pub(super) fn orchestration_projection_distinguishes_background_failure_and_timeout() {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_failed"),
        harness_core::event::EventV1::BackgroundTaskNotification(
            harness_core::event::BackgroundTaskNotificationEvent {
                parent_session_id: "agent_parent".into(),
                parent_agent_id: Some("agent_parent".to_string()),
                child_session_id: "agent_failed".into(),
                child_request_id: "req_failed".to_string(),
                task_id: "task_failed".to_string().into(),
                description: "fail child".to_string(),
                status: harness_core::event::BackgroundTaskNotificationStatus::Failed,
                summary: "child failed".to_string(),
                terminal_event_id: "evt_failed".to_string(),
                terminal_task_id: "task_failed".to_string(),
                delivered_turn_request_id: Some("req_parent_failed".to_string()),
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_timeout"),
        harness_core::event::EventV1::BackgroundTaskNotification(
            harness_core::event::BackgroundTaskNotificationEvent {
                parent_session_id: "agent_parent".into(),
                parent_agent_id: Some("agent_parent".to_string()),
                child_session_id: "agent_timeout".into(),
                child_request_id: "req_timeout".to_string(),
                task_id: "task_timeout".to_string().into(),
                description: "timeout child".to_string(),
                status: harness_core::event::BackgroundTaskNotificationStatus::TimedOut,
                summary: "child timed out".to_string(),
                terminal_event_id: "evt_timeout".to_string(),
                terminal_task_id: "task_timeout".to_string(),
                delivered_turn_request_id: Some("req_parent_timeout".to_string()),
            },
        ),
    ));

    let rows = app.orchestration_visible_rows();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].task_id, "task_timeout");
    assert_eq!(rows[0].state, crate::app::OrchestrationTaskState::TimedOut);
    assert_eq!(rows[0].warning.as_deref(), Some("timed out"));
    assert_eq!(rows[1].task_id, "task_failed");
    assert_eq!(rows[1].state, crate::app::OrchestrationTaskState::Failed);
    assert_eq!(rows[1].warning.as_deref(), Some("failed"));
}

pub(super) fn orchestration_projection_retains_only_recent_terminal_rows() {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_live_stale".to_string().into(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:running:live".to_string()),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        None,
        harness_core::event::EventV1::StaleDetected(harness_core::event::StaleDetectedEvent {
            task_id: "task_live_stale".to_string().into(),
            stale_for_ms: 4242,
        }),
    ));
    app.ingest_event(envelope(
        3,
        None,
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_live_queued".to_string().into(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("agent:queued:live".to_string()),
            metadata: None,
        }),
    ));

    app.ingest_event(envelope(
        4,
        None,
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_terminal_1".to_string().into(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("terminal:q1".to_string()),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        5,
        None,
        harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
            task_id: "task_terminal_1".to_string().into(),
            result_summary: "terminal 1 completed".to_string(),
            result_digest: "digest-terminal-1".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        6,
        None,
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_terminal_2".to_string().into(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("terminal:q2".to_string()),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        7,
        None,
        harness_core::event::EventV1::TaskCancelled(harness_core::event::TaskCancelledEvent {
            task_id: "task_terminal_2".to_string().into(),
            reason: "cancelled 2".to_string(),
            task_scope: None,
        }),
    ));
    app.ingest_event(envelope(
        8,
        None,
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_terminal_3".to_string().into(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("terminal:q3".to_string()),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        9,
        None,
        harness_core::event::EventV1::StaleDetected(harness_core::event::StaleDetectedEvent {
            task_id: "task_terminal_3".to_string().into(),
            stale_for_ms: 9003,
        }),
    ));
    app.ingest_event(envelope(
        10,
        None,
        harness_core::event::EventV1::TaskResultLate(harness_core::event::TaskResultLateEvent {
            task_id: "task_terminal_3".to_string().into(),
            result_digest: "digest-terminal-3".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        11,
        None,
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_terminal_4".to_string().into(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("terminal:q4".to_string()),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        12,
        None,
        harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
            task_id: "task_terminal_4".to_string().into(),
            result_summary: "terminal 4 completed".to_string(),
            result_digest: "digest-terminal-4".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        13,
        None,
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_terminal_5".to_string().into(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("terminal:q5".to_string()),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        14,
        None,
        harness_core::event::EventV1::TaskCancelled(harness_core::event::TaskCancelledEvent {
            task_id: "task_terminal_5".to_string().into(),
            reason: "cancelled 5".to_string(),
            task_scope: None,
        }),
    ));
    app.ingest_event(envelope(
        15,
        None,
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_terminal_6".to_string().into(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("terminal:q6".to_string()),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        16,
        None,
        harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
            task_id: "task_terminal_6".to_string().into(),
            result_summary: "terminal 6 completed".to_string(),
            result_digest: "digest-terminal-6".to_string(),
            metadata: None,
        }),
    ));

    let rows = app.orchestration_visible_rows();
    assert_eq!(rows.len(), 7);
    assert_eq!(
        rows.iter()
            .map(|row| (
                row.task_id.as_str(),
                row.queue_key.as_deref(),
                row.warning.as_deref(),
                row.state,
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                "task_live_stale",
                Some("agent:running:live"),
                Some("stale for 4242 ms"),
                crate::app::OrchestrationTaskState::Stale,
            ),
            (
                "task_live_queued",
                Some("agent:queued:live"),
                None,
                crate::app::OrchestrationTaskState::Queued,
            ),
            (
                "task_terminal_6",
                Some("terminal:q6"),
                None,
                crate::app::OrchestrationTaskState::Completed,
            ),
            (
                "task_terminal_5",
                Some("terminal:q5"),
                Some("cancelled 5"),
                crate::app::OrchestrationTaskState::Cancelled,
            ),
            (
                "task_terminal_4",
                Some("terminal:q4"),
                None,
                crate::app::OrchestrationTaskState::Completed,
            ),
            (
                "task_terminal_3",
                Some("terminal:q3"),
                Some("late result after stale cancellation"),
                crate::app::OrchestrationTaskState::LateResult,
            ),
            (
                "task_terminal_2",
                Some("terminal:q2"),
                Some("cancelled 2"),
                crate::app::OrchestrationTaskState::Cancelled,
            ),
        ]
    );
    assert!(
        !rows.iter().any(|row| row.task_id == "task_terminal_1"),
        "terminal retention must drop the oldest terminal row once six exist"
    );
}
