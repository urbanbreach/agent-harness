use super::operator_rail_test_fixtures::*;
use super::*;

#[test]
fn operator_rail_collapses_live_child_internal_tasks_into_one_active_subagent() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(operator_rail_test_event_with_correlation(
        1,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
        "req_parent",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_parent".to_string(),
                text: "Use one subagent for coding work".to_string(),
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        2,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_parent",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_task_active".to_string(),
                tool_id: "task".to_string(),
                args_summary: serde_json::json!({
                    "description": "implement the requested fix",
                    "subagent_type": "explore"
                })
                .to_string(),
                args_digest: "digest-task-active".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(operator_rail_test_event_with_correlation(
        3,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, None),
        "req_parent",
        harness_core::event::EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
            tool_call_id: "tool_call_task_active".to_string(),
        }),
    ));
    app.ingest_event(operator_rail_test_event(
        4,
        harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_child".to_string(),
            profile: "explore".to_string(),
            parent_agent_id: Some("agent_parent".to_string()),
        }),
    ));

    for (seq, task_id, queue_key) in [
        (5, "task_child_turn", "provider_model:mock:model-1"),
        (6, "task_child_read", "tool:read"),
        (7, "task_child_edit", "tool:edit"),
    ] {
        app.ingest_event(operator_rail_test_event_with_correlation(
            seq,
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::Worker,
                Some("agent_child".to_string()),
            ),
            "req_child",
            harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
                task_id: task_id.to_string(),
                state: harness_core::event::TaskScheduleState::Started,
                queue_key: Some(queue_key.to_string()),
            }),
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
        .expect("explore group");
    assert_eq!(explore.items.len(), 1);
    assert_eq!(explore.items[0].status, SubagentRailStatus::Running);

    let sidebar = operator_sidebar_text_for_test(&app).join("\n");
    assert!(sidebar.contains("• ⠋ Explore Task"));
    assert!(!sidebar.contains("Explore Task 2"));
    assert!(!sidebar.contains("tasks ·"));
}
