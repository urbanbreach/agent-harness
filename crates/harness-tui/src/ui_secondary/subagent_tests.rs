use super::operator_rail_test_fixtures::*;
use super::*;
use crate::UnwrapOrAbort;

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_renders_subagent_rows_from_orchestration_state() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(operator_rail_test_event_with_correlation(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
        "req_subagents",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_subagents".to_string(),
                text: "Delegate the investigation".to_string(),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        2,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_subagents",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_task_running".to_string(),
                tool_id: "task".to_string(),
                args_summary: serde_json::json!({
                    "description": "inspect README and summarize task background behavior in three bullets",
                    "subagent_type": "explore"
                })
                .to_string(),
                args_digest: "digest-task-running".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        3,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_subagents",
        harness_core::event::EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
            tool_call_id: "tool_call_task_running".to_string(),
        }),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        4,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_subagents",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_task_explore_done".to_string(),
                tool_id: "task".to_string(),
                args_summary: serde_json::json!({
                    "description": "cross-check docs",
                    "subagent_type": "explore"
                })
                .to_string(),
                args_digest: "digest-task-explore-done".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        5,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_subagents",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_task_explore_done".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("Task Result: cross-check docs".to_string()),
                output_digest: Some("digest-task-explore-done-output".to_string()),
                output_json: None,
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        6,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_subagents",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_task_done".to_string(),
                tool_id: "task".to_string(),
                args_summary: serde_json::json!({
                    "description": "summary",
                    "subagent_type": "general"
                })
                .to_string(),
                args_digest: "digest-task-done".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        7,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_subagents",
        harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
            task_id: "task_done".to_string(),
            result_summary: "summarized an intentionally long repository behavior report"
                .to_string(),
            result_digest: "digest-done".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        8,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_subagents",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_task_done".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("Task Result: summary".to_string()),
                output_digest: Some("digest-task-done-output".to_string()),
                output_json: None,
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        9,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_subagents",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_task_failed".to_string(),
                tool_id: "task".to_string(),
                args_summary: serde_json::json!({
                    "description": "cancelled",
                    "subagent_type": "plan"
                })
                .to_string(),
                args_digest: "digest-task-failed".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        10,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_subagents",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_task_failed".to_string(),
                status: harness_core::event::ToolCallStatus::Failed,
                output_summary: Some("operator cancelled".to_string()),
                output_digest: Some("digest-task-failed-output".to_string()),
                output_json: None,
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        11,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_subagents",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_task_wrapped_child".to_string(),
                tool_id: "task".to_string(),
                args_summary: serde_json::json!({
                    "description": "open wrapped child session after reviewing enough sidebar text to wrap cleanly",
                    "subagent_type": "navigator"
                })
                .to_string(),
                args_digest: "digest-task-wrapped-child".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        12,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_subagents",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_task_wrapped_child".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("Task Result: wrapped child".to_string()),
                output_digest: Some("digest-task-wrapped-child-output".to_string()),
                output_json: Some(serde_json::json!({
                    "profile": "navigator",
                    "status": "scheduled",
                    "session_id": "child_running_session"
                })),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        13,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        "req_subagents",
        harness_core::event::EventV1::RunStarted(harness_core::event::RunStartedEvent {
            run_name: "subagent sidebar".to_string(),
            workspace_root: "/tmp/subagent-sidebar-footer".to_string(),
        }),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        14,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_orphan_running",
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_orphan_running".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:running:orphan".to_string()),
        }),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        15,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_orphan_queued",
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_orphan_queued".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("agent:queued:queued".to_string()),
        }),
    ));

    let sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(sidebar.contains("▼ Subagents"));
    assert!(sidebar.contains("▶ Explore ⠋ 2 tasks · 1 active"));
    assert!(!sidebar.contains("  ⠋ inspect README"));
    assert!(!sidebar.contains("  ✓ cross-check docs"));
    assert!(!sidebar.contains("            bullets"));
    assert!(sidebar.contains("• ✓ General Task"));
    assert!(sidebar.contains("• ✗ Plan Task"));
    assert!(sidebar.contains("• ⠋ Navigator Task"));
    assert!(sidebar.contains("• ⠋ Orphan Task"));
    assert!(sidebar.contains("• ⠋ Queued Task"));
    assert!(!sidebar.contains("intentionally long repository"));

    let theme = *app.theme();
    let lines = operator_sidebar_lines_for_test(&app);
    let explore_group = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .contains("▶ Explore ⠋ 2 tasks · 1 active")
        })
        .unwrap_or_abort();
    assert_eq!(explore_group.spans[0].style.fg, Some(theme.status.success));
    assert_eq!(explore_group.spans[1].style.fg, Some(theme.text.primary));
    assert_eq!(explore_group.spans[2].style.fg, Some(theme.status.success));
    assert_eq!(explore_group.spans[3].style.fg, Some(theme.text.primary));

    let cancelled = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .contains("• ✗ Plan Task")
        })
        .unwrap_or_abort();
    assert_eq!(cancelled.spans[0].style.fg, Some(theme.text.primary));
    assert_eq!(cancelled.spans[1].style.fg, Some(theme.text.secondary));
    assert_eq!(cancelled.spans[2].style.fg, Some(theme.text.secondary));

    let sidebar_area = Rect::new(0, 0, 32, 40);
    let theme = &theme;
    let inner =
        operator_sidebar_inner_area(&app, sidebar_area, theme, OperatorSidebarChrome::Persistent)
            .unwrap_or_abort();
    let rail = build_operator_rail_model(&app);
    let title_height = u16::try_from(
        build_operator_rail_title_text(rail.title.as_ref(), theme, inner.width)
            .lines
            .len()
            .min(usize::from(u16::MAX)),
    )
    .unwrap_or(u16::MAX);
    let wrapped_layout = build_operator_rail_body_layout(&rail.body, theme, inner.width, 0);
    let explore_group_region = wrapped_layout
        .subagent_group_hit_regions
        .iter()
        .find(|region| region.agent_name == "Explore")
        .unwrap_or_abort();
    assert_eq!(
        operator_sidebar_subagent_group_hit_target_in_surface(
            &app,
            sidebar_area,
            theme,
            OperatorSidebarChrome::Persistent,
            inner.x,
            inner
                .y
                .saturating_add(title_height)
                .saturating_add(u16::try_from(explore_group_region.top_row).unwrap_or(u16::MAX)),
        ),
        Some("Explore".to_string())
    );
    assert!(
        wrapped_layout
            .subagent_hit_regions
            .iter()
            .all(|region| region.session_id != "child_explore_session"),
        "collapsed subagent group should hide child-session hit regions"
    );
    let wrapped_region = wrapped_layout
        .subagent_hit_regions
        .iter()
        .find(|region| region.session_id == "child_running_session")
        .unwrap_or_abort();
    assert!(
        wrapped_region.height == 1,
        "long subagent row should stay compact and keep a one-row hit region"
    );
    let body_y = inner.y.saturating_add(title_height);
    let wrapped_row =
        body_y.saturating_add(u16::try_from(wrapped_region.top_row).unwrap_or(u16::MAX));
    assert_eq!(
        operator_sidebar_subagent_session_hit_target_in_surface(
            &app,
            sidebar_area,
            theme,
            OperatorSidebarChrome::Persistent,
            inner.x,
            wrapped_row,
        ),
        Some("child_running_session".to_string())
    );
    assert_ne!(
        operator_sidebar_subagent_session_hit_target_in_surface(
            &app,
            sidebar_area,
            theme,
            OperatorSidebarChrome::Persistent,
            inner.x,
            wrapped_row.saturating_add(1),
        ),
        Some("child_running_session".to_string())
    );

    let footer_height = operator_sidebar_footer_height(&app, theme, inner.width);
    assert!(
        footer_height > 0,
        "test setup should render a sidebar footer"
    );
    let footer_sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(title_height),
            Constraint::Min(0),
            Constraint::Length(footer_height),
        ])
        .split(inner);
    assert_eq!(
        operator_sidebar_subagent_session_hit_target_in_surface(
            &app,
            sidebar_area,
            theme,
            OperatorSidebarChrome::Persistent,
            inner.x,
            footer_sections[2].y,
        ),
        None,
        "footer clicks must not activate hidden subagent rows"
    );

    let frame_area = Rect::new(0, 0, 140, 40);
    let plan = crate::layout::FrameLayoutPlan::for_app(&app, frame_area);
    let sidebar_area = plan.operator_sidebar.unwrap_or_abort();
    let inner =
        operator_sidebar_inner_area(&app, sidebar_area, theme, OperatorSidebarChrome::Persistent)
            .unwrap_or_abort();
    let rail = build_operator_rail_model(&app);
    let body_area =
        operator_sidebar_body_area(&app, inner, theme, rail.title.as_ref()).unwrap_or_abort();
    let layout = build_operator_rail_body_layout(&rail.body, theme, body_area.width, 0);
    let group_region = layout
        .subagent_group_hit_regions
        .iter()
        .find(|region| region.agent_name == "Explore")
        .unwrap_or_abort();
    let group_row = body_area
        .y
        .saturating_add(u16::try_from(group_region.top_row).unwrap_or(u16::MAX));
    let group_col = body_area.x;
    for kind in [
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left),
    ] {
        app.handle_mouse(
            crossterm::event::MouseEvent {
                kind,
                column: group_col,
                row: group_row,
                modifiers: crossterm::event::KeyModifiers::NONE,
            },
            frame_area,
            None,
            None,
            None,
        );
    }

    let expanded_sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(expanded_sidebar.contains("▼ Explore ⠋ 2 tasks · 1 active"));
    assert!(expanded_sidebar.contains("  ⠋ Explore Task"));
    assert!(expanded_sidebar.contains("  ✓ Explore Task 2"));
    let expanded_lines = operator_sidebar_lines_for_test(&app);
    let running = expanded_lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .contains("  ⠋ Explore Task")
        })
        .unwrap_or_abort();
    assert_eq!(running.spans[0].style.fg, Some(theme.status.success));
    assert_eq!(running.spans[1].style.fg, Some(theme.status.success));
    assert_eq!(running.spans[2].style.fg, Some(theme.text.primary));

    app.handle_mouse(
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::NONE,
        },
        Rect::new(0, 0, 140, 40),
        None,
        Some(OperatorSidebarSection::Subagents),
        None,
    );

    let collapsed_sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(collapsed_sidebar.contains("▶ Subagents (6 types)"));
    assert!(!collapsed_sidebar.contains("inspect README"));
    assert!(!collapsed_sidebar.contains("✓ General Task"));
    assert!(!collapsed_sidebar.contains("✗ Plan Task"));
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_marks_background_subagent_terminal_from_notification() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(operator_rail_test_event_with_correlation(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
        "req_background_parent",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_background_parent".to_string(),
                text: "Start a background subagent".to_string(),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        2,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_background_parent",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_background_task".to_string(),
                tool_id: "task".to_string(),
                args_summary: serde_json::json!({
                    "description": "summarize README",
                    "subagent_type": "general"
                })
                .to_string(),
                args_digest: "digest-background-task-args".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        3,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_background_parent",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_background_task".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("Background task scheduled".to_string()),
                output_digest: Some("digest-background-task-output".to_string()),
                output_json: Some(serde_json::json!({
                    "profile": "general",
                    "background": true,
                    "status": "scheduled",
                    "child_session_id": "agent_child",
                    "child_request_id": "req_child"
                })),
                metadata: None,
            },
        ),
    ));

    let active_sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(active_sidebar.contains("• ⠋ General Task"));

    app.ingest_event(operator_rail_test_event_with_correlation(
        4,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        "background_task_notification:req_child",
        harness_core::event::EventV1::BackgroundTaskNotification(
            harness_core::event::BackgroundTaskNotificationEvent {
                parent_session_id: "run_fixture".to_string(),
                parent_agent_id: Some("agent_parent".to_string()),
                child_session_id: "agent_child".to_string(),
                child_request_id: "req_child".to_string(),
                task_id: "agent_child".to_string(),
                description: "summarize README".to_string(),
                status: harness_core::event::BackgroundTaskNotificationStatus::Completed,
                summary: "README summarized".to_string(),
                terminal_event_id: "evt_child_done".to_string(),
                terminal_task_id: "agent_child".to_string(),
                delivered_turn_request_id: Some("req_parent_wakeup".to_string()),
            },
        ),
    ));

    let completed_sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(completed_sidebar.contains("• ✓ General Task"));
    assert!(!completed_sidebar.contains("• ⠋ General Task"));
    assert!(!completed_sidebar.contains("1 active"));
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_uses_simple_subagent_task_labels() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(operator_rail_test_event_with_correlation(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
        "req_simple_subagents",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_simple_subagents".to_string(),
                text: "Run several subagents".to_string(),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        2,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_simple_subagents",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_explore_background".to_string(),
                tool_id: "task".to_string(),
                args_summary: serde_json::json!({
                    "description": "Delegation context from parent: - investigate everything and return a detailed report",
                    "subagent_type": "explore",
                    "run_in_background": true
                })
                .to_string(),
                args_digest: "digest-explore-background".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        3,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_simple_subagents",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_explore_background".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("Background task scheduled".to_string()),
                output_digest: Some("digest-explore-background-output".to_string()),
                output_json: Some(serde_json::json!({
                    "profile": "explore",
                    "background": true,
                    "status": "scheduled",
                    "child_session_id": "agent_explore",
                    "child_request_id": "req_explore"
                })),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        4,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        "background_task_notification:req_explore",
        harness_core::event::EventV1::BackgroundTaskNotification(
            harness_core::event::BackgroundTaskNotificationEvent {
                parent_session_id: "run_fixture".to_string(),
                parent_agent_id: Some("agent_parent".to_string()),
                child_session_id: "agent_explore".to_string(),
                child_request_id: "req_explore".to_string(),
                task_id: "agent_explore".to_string(),
                description: "Delegation context from parent: - investigate everything".to_string(),
                status: harness_core::event::BackgroundTaskNotificationStatus::Completed,
                summary: "{\"sessionId\":\"term-1\",\"cols\":80}".to_string(),
                terminal_event_id: "evt_explore_done".to_string(),
                terminal_task_id: "agent_explore".to_string(),
                delivered_turn_request_id: Some("req_parent_wakeup".to_string()),
            },
        ),
    ));
    for (seq, tool_call_id, request_id, status) in [
        (
            5,
            "tool_call_librarian_one",
            "req_librarian_one",
            harness_core::event::BackgroundTaskNotificationStatus::Completed,
        ),
        (
            8,
            "tool_call_librarian_two",
            "req_librarian_two",
            harness_core::event::BackgroundTaskNotificationStatus::Failed,
        ),
    ] {
        app.ingest_event(operator_rail_test_event_with_correlation(
            seq,
            harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
            "req_simple_subagents",
            harness_core::event::EventV1::ToolCallRequested(
                harness_core::event::ToolCallRequestedEvent {
                    tool_call_id: tool_call_id.to_string(),
                    tool_id: "task".to_string(),
                    args_summary: serde_json::json!({
                        "description": "Use remote repositories and official docs; return references",
                        "subagent_type": "librarian",
                        "run_in_background": true
                    })
                    .to_string(),
                    args_digest: format!("digest-{tool_call_id}"),
                    metadata: None,
                },
            ),
        ));
        app.ingest_event(operator_rail_test_event_with_correlation(
            seq + 1,
            harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
            "req_simple_subagents",
            harness_core::event::EventV1::ToolCallFinished(
                harness_core::event::ToolCallFinishedEvent {
                    tool_call_id: tool_call_id.to_string(),
                    status: harness_core::event::ToolCallStatus::Succeeded,
                    output_summary: Some("Background task scheduled".to_string()),
                    output_digest: Some(format!("digest-{tool_call_id}-output")),
                    output_json: Some(serde_json::json!({
                        "profile": "librarian",
                        "background": true,
                        "status": "scheduled",
                        "child_session_id": format!("agent_{request_id}"),
                        "child_request_id": request_id
                    })),
                    metadata: None,
                },
            ),
        ));
        app.ingest_event(operator_rail_test_event_with_correlation(
            seq + 2,
            harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
            &format!("background_task_notification:{request_id}"),
            harness_core::event::EventV1::BackgroundTaskNotification(
                harness_core::event::BackgroundTaskNotificationEvent {
                    parent_session_id: "run_fixture".to_string(),
                    parent_agent_id: Some("agent_parent".to_string()),
                    child_session_id: format!("agent_{request_id}"),
                    child_request_id: request_id.to_string(),
                    task_id: format!("agent_{request_id}"),
                    description: "Use remote repositories and official docs".to_string(),
                    status,
                    summary: "remote search finished".to_string(),
                    terminal_event_id: format!("evt_{request_id}_done"),
                    terminal_task_id: format!("agent_{request_id}"),
                    delivered_turn_request_id: None,
                },
            ),
        ));
    }

    let model = build_operator_rail_model(&app);
    let groups = match &model.body.sections[0] {
        OperatorRailBodySection::Subagents { groups, .. } => groups,
        section => panic!("expected subagent section, got {}", section.heading()),
    };
    let explore = groups
        .iter()
        .find(|group| group.agent_name == "Explore")
        .unwrap_or_abort();
    assert_eq!(explore.items.len(), 1);
    assert_eq!(explore.items[0].description, "Explore Task");
    assert_eq!(explore.items[0].status, SubagentRailStatus::Completed);

    let librarian = groups
        .iter()
        .find(|group| group.agent_name == "Librarian")
        .unwrap_or_abort();
    assert_eq!(librarian.items.len(), 2);
    assert_eq!(librarian.items[0].description, "Librarian Task");
    assert_eq!(librarian.items[0].status, SubagentRailStatus::Completed);
    assert_eq!(librarian.items[1].description, "Librarian Task 2");
    assert_eq!(librarian.items[1].status, SubagentRailStatus::Error);

    let sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(!sidebar.contains("Delegation context"));
    assert!(!sidebar.contains("{\"sessionId\""));
    assert!(!sidebar.contains("Use remote repositories"));
    assert!(!sidebar.contains(" · 1 active"));
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_shows_wakeup_report_without_task_tool_row() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(operator_rail_test_event_with_correlation(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        "agent_child",
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_child".to_string(),
            profile: "plan".to_string(),
            parent_agent_id: Some("agent_parent".to_string()),
        }),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        2,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        "background_task_notification:req_child",
        harness_core::event::EventV1::BackgroundTaskNotification(
            harness_core::event::BackgroundTaskNotificationEvent {
                parent_session_id: "run_fixture".to_string(),
                parent_agent_id: Some("agent_parent".to_string()),
                child_session_id: "agent_child".to_string(),
                child_request_id: "req_child".to_string(),
                task_id: "task_child".to_string(),
                description: "Inspect sidebar wakeup".to_string(),
                status: harness_core::event::BackgroundTaskNotificationStatus::Completed,
                summary: "Wakeup report finished".to_string(),
                terminal_event_id: "evt_child_done".to_string(),
                terminal_task_id: "task_child".to_string(),
                delivered_turn_request_id: Some("req_parent_wakeup".to_string()),
            },
        ),
    ));

    let sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(sidebar.contains("▼ Subagents"));
    assert!(sidebar.contains("• ✓ Plan Task"));
    assert!(!sidebar.contains("• subagent ✓"));
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_shows_replay_wakeup_report_without_task_tool_row() {
    let events = vec![
        operator_rail_test_event_with_correlation(
            1,
            harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
            "agent_child",
            harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
                agent_id: "agent_child".to_string(),
                profile: "plan".to_string(),
                parent_agent_id: Some("agent_parent".to_string()),
            }),
        ),
        operator_rail_test_event_with_correlation(
            2,
            harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
            "background_task_notification:req_child",
            harness_core::event::EventV1::BackgroundTaskNotification(
                harness_core::event::BackgroundTaskNotificationEvent {
                    parent_session_id: "run_fixture".to_string(),
                    parent_agent_id: Some("agent_parent".to_string()),
                    child_session_id: "agent_child".to_string(),
                    child_request_id: "req_child".to_string(),
                    task_id: "task_child".to_string(),
                    description: "Inspect replay sidebar wakeup".to_string(),
                    status: harness_core::event::BackgroundTaskNotificationStatus::Completed,
                    summary: "Replay wakeup report finished".to_string(),
                    terminal_event_id: "evt_child_done".to_string(),
                    terminal_task_id: "task_child".to_string(),
                    delivered_turn_request_id: Some("req_parent_wakeup".to_string()),
                },
            ),
        ),
    ];
    let app = AppState::new_replay(std::path::PathBuf::from("/tmp/replay-wakeup"), events);

    let sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(sidebar.contains("▼ Subagents"));
    assert!(sidebar.contains("• ✓ Plan Task"));
    assert!(!sidebar.contains("Replay wakeup report finished"));
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_keeps_subagents_visible_in_replay() {
    let events = vec![
        operator_rail_test_event_with_correlation(
            1,
            harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
            "req_replay_subagent",
            harness_core::event::EventV1::UserMessageSubmitted(
                harness_core::event::UserMessageSubmittedEvent {
                    request_id: "req_replay_subagent".to_string(),
                    text: "Review replay sidebar parity".to_string(),
                },
            ),
        ),
        operator_rail_test_event_with_correlation(
            2,
            harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
            "req_replay_subagent",
            harness_core::event::EventV1::ToolCallRequested(
                harness_core::event::ToolCallRequestedEvent {
                    tool_call_id: "tool_call_replay_subagent".to_string(),
                    tool_id: "task".to_string(),
                    args_summary: serde_json::json!({
                        "description": "audit replay subagent sidebar",
                        "subagent_type": "researcher"
                    })
                    .to_string(),
                    args_digest: "digest-replay-subagent".to_string(),
                    metadata: Some(harness_core::event::ToolCallMetadata {
                        lineage: Some(harness_core::event::TaskLineageMetadata {
                            child_session_id: Some("child_replay_session".to_string()),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                },
            ),
        ),
        operator_rail_test_event_with_correlation(
            3,
            harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
            "req_replay_subagent",
            harness_core::event::EventV1::ToolCallFinished(
                harness_core::event::ToolCallFinishedEvent {
                    tool_call_id: "tool_call_replay_subagent".to_string(),
                    status: harness_core::event::ToolCallStatus::Succeeded,
                    output_summary: Some("Replay sidebar audited".to_string()),
                    output_digest: Some("digest-replay-subagent-output".to_string()),
                    output_json: None,
                    metadata: Some(harness_core::event::ToolCallMetadata {
                        lineage: Some(harness_core::event::TaskLineageMetadata {
                            child_session_id: Some("child_replay_session".to_string()),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                },
            ),
        ),
    ];
    let app = AppState::new_replay(std::path::PathBuf::from("/tmp/replay-subagent"), events);

    let sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(sidebar.contains("▼ Subagents"));
    assert!(sidebar.contains("• ✓ Researcher Task"));
}
