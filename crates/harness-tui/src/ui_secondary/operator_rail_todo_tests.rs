use super::operator_rail_test_fixtures::*;
use super::*;
use crate::UnwrapOrAbort;

pub(crate) fn exact_test_operator_rail_renders_todo_items_from_tool_state() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(operator_rail_test_event_with_correlation(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
        "req-todo",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req-todo".into(),
                text: "Track the implementation".to_string(),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        2,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req-todo",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_todo".into(),
                tool_id: "todowrite".to_string(),
                args_summary:
                    r#"{"todos":[{"content":"Plan work","status":"pending","priority":"high"}]}"#
                        .to_string(),
                args_digest: "digest-todo".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        3,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req-todo",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_todo".into(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("todo list updated".to_string()),
                output_digest: Some("digest-todo-output".to_string()),
                output_json: Some(serde_json::json!({
                    "todos": [
                        {"content": "Plan work", "status": "completed", "priority": "high"},
                        {"content": "Implement UI", "status": "in_progress", "priority": "high"},
                        {"content": "Verify tests", "status": "pending", "priority": "medium"}
                    ]
                })),
                metadata: None,
            },
        ),
    ));

    let model = build_operator_rail_model(&app);
    assert_eq!(model.body.sections[0].heading(), "▼ Todo");
    assert_eq!(model.body.sections[1].heading(), "▼ MCP");

    let sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(sidebar.contains("▼ Todo"));
    assert!(sidebar.contains("[✓] Plan work"));
    assert!(sidebar.contains("[•] Implement UI"));
    assert!(sidebar.contains("[ ] Verify tests"));

    let theme = *app.theme();
    let lines = operator_sidebar_lines_for_test(&app);
    let in_progress = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .contains("[•] Implement UI")
        })
        .unwrap_or_abort();
    assert_eq!(in_progress.spans[0].style.fg, Some(theme.status.warning));
    assert_eq!(in_progress.spans[1].style.fg, Some(theme.status.warning));

    let pending = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .contains("[ ] Verify tests")
        })
        .unwrap_or_abort();
    assert_eq!(pending.spans[0].style.fg, Some(theme.text.secondary));
    assert_eq!(pending.spans[1].style.fg, Some(theme.text.secondary));
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_places_todo_below_subagents() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(operator_rail_test_event_with_correlation(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
        "req-sidebar-order",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req-sidebar-order".into(),
                text: "Inspect sidebar section order".to_string(),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        2,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req-sidebar-order",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_task_order".into(),
                tool_id: "task".to_string(),
                args_summary: serde_json::json!({
                    "description": "inspect subagent ordering",
                    "subagent_type": "general"
                })
                .to_string(),
                args_digest: "digest-task-order".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        3,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req-sidebar-order",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_todo_order".into(),
                tool_id: "todowrite".to_string(),
                args_summary: r#"{"todos":[{"content":"Keep todo below subagents","status":"in_progress","priority":"high"}]}"#
                    .to_string(),
                args_digest: "digest-todo-order".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        4,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req-sidebar-order",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_todo_order".into(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("todo list updated".to_string()),
                output_digest: Some("digest-todo-order-output".to_string()),
                output_json: Some(serde_json::json!({
                    "todos": [
                        {"content": "Keep todo below subagents", "status": "in_progress", "priority": "high"}
                    ]
                })),
                metadata: None,
            },
        ),
    ));

    let model = build_operator_rail_model(&app);
    assert_eq!(model.body.sections[0].heading(), "▼ Subagents");
    assert_eq!(model.body.sections[1].heading(), "Todo");
    assert_eq!(model.body.sections[2].heading(), "▼ MCP");
}

pub(crate) fn exact_test_operator_rail_hides_completed_todo_state() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(operator_rail_test_event_with_correlation(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
        "req-todo-completed",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req-todo-completed".into(),
                text: "Finish the checklist".to_string(),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        2,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req-todo-completed",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_todo_completed".into(),
                tool_id: "todowrite".to_string(),
                args_summary: r#"{"todos":[{"content":"Ship todo panel","status":"completed","priority":"high"}]}"#
                    .to_string(),
                args_digest: "digest-todo-completed".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        3,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req-todo-completed",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_todo_completed".into(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("todo list completed".to_string()),
                output_digest: Some("digest-todo-completed-output".to_string()),
                output_json: Some(serde_json::json!({
                    "todos": [
                        {"content": "Ship todo panel", "status": "completed", "priority": "high"}
                    ]
                })),
                metadata: None,
            },
        ),
    ));

    let sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(!sidebar.contains("Todo"));
    assert!(!sidebar.contains("[✓] Ship todo panel"));
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_renders_todo_items_from_artifact_state() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let artifact_path = temp_dir
        .path()
        .join("artifacts")
        .join("toolcalls")
        .join("tool_call_todo_artifact")
        .join("result.json");
    std::fs::create_dir_all(artifact_path.parent().unwrap_or_abort()).unwrap_or_abort();
    std::fs::write(
        &artifact_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "todos": [
                {"content": "Persisted todo", "status": "in_progress", "priority": "high"}
            ]
        }))
        .unwrap_or_abort(),
    )
    .unwrap_or_abort();

    let mut app = AppState::new_live(Some(temp_dir.path().to_path_buf()), false, None);
    app.ingest_event(operator_rail_test_event_with_correlation(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
        "req-todo-artifact",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req-todo-artifact".into(),
                text: "Render persisted todo state".to_string(),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        2,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req-todo-artifact",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_todo_artifact".into(),
                tool_id: "todowrite".to_string(),
                args_summary: r#"{"todos":[{"content":"Persisted todo","status":"in_progress","priority":"high"}]}"#
                    .to_string(),
                args_digest: "digest-todo-artifact".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        3,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req-todo-artifact",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_todo_artifact".into(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("todo list updated".to_string()),
                output_digest: Some("digest-todo-artifact-output".to_string()),
                output_json: None,
                metadata: Some(harness_core::event::ToolCallMetadata {
                    artifact_refs: vec![harness_core::event::EventArtifactRef {
                        path: "artifacts/toolcalls/tool_call_todo_artifact/result.json".to_string(),
                        digest: None,
                    }],
                    ..Default::default()
                }),
            },
        ),
    ));

    let sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(sidebar.contains("Todo"));
    assert!(sidebar.contains("[•] Persisted todo"));
}

#[cfg(test)]
pub(crate) fn exact_test_operator_rail_collapses_todo_section_body() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(operator_rail_test_event_with_correlation(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
        "req-todo-collapse",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req-todo-collapse".into(),
                text: "Track several tasks".to_string(),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        2,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req-todo-collapse",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_todo_collapse".into(),
                tool_id: "todowrite".to_string(),
                args_summary: r#"{"todos":[{"content":"One","status":"completed","priority":"high"},{"content":"Two","status":"in_progress","priority":"high"},{"content":"Three","status":"pending","priority":"medium"}]}"#
                    .to_string(),
                args_digest: "digest-todo-collapse".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        3,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req-todo-collapse",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_todo_collapse".into(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("todo list updated".to_string()),
                output_digest: Some("digest-todo-collapse-output".to_string()),
                output_json: Some(serde_json::json!({
                    "todos": [
                        {"content": "One", "status": "completed", "priority": "high"},
                        {"content": "Two", "status": "in_progress", "priority": "high"},
                        {"content": "Three", "status": "pending", "priority": "medium"}
                    ]
                })),
                metadata: None,
            },
        ),
    ));

    let open_sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(open_sidebar.contains("▼ Todo"));
    assert!(open_sidebar.contains("[•] Two"));

    app.handle_mouse(
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::NONE,
        },
        Rect::new(0, 0, 140, 40),
        None,
        Some(OperatorSidebarSection::Todo),
        None,
    );

    let collapsed_sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(collapsed_sidebar.contains("▶ Todo"));
    assert!(!collapsed_sidebar.contains("[✓] One"));
    assert!(!collapsed_sidebar.contains("[•] Two"));
    assert!(!collapsed_sidebar.contains("[ ] Three"));
}
