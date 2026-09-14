use super::*;
use crate::UnwrapOrAbort;

pub(crate) fn parent_transcript_hides_child_prompt_before_task_tool_finishes() {
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let parent_path = run_dir.path().join("parent");
    fs::create_dir_all(&parent_path).unwrap_or_abort();
    let mut app = AppState::new_live(Some(parent_path), false, None);
    app.ingest_event(agent_spawned(1, "parent", "build"));
    app.ingest_event(envelope(
        2,
        "req_parent",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_parent".into(),
            text: "Start parent work".to_string(),
        }),
    ));
    app.ingest_event(provider_started(3, "req_parent", "default", "model-parent"));
    app.ingest_event(envelope(
        4,
        "req_parent",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_child_pending".into(),
            tool_id: "task".to_string(),
            args_summary: r#"{"description":"inspect child","subagent_type":"explore"}"#
                .to_string(),
            args_digest: "digest-child-pending".to_string(),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("task".to_string()),
                ..ToolCallMetadata::default()
            }),
        }),
    ));
    app.ingest_event(child_agent_spawned(5, "agent_child", "explore", "parent"));
    let mut child_prompt = envelope_with_actor(
        6,
        "req_child",
        EventActor::new(ActorKind::Supervisor, None),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_child".into(),
            text: "Inspect child prompt that belongs to the subagent".to_string(),
        }),
    );
    child_prompt.stream_key = Some("agent:agent_child".to_string());
    app.ingest_event(child_prompt);

    let parent_debug = render_debug(&app, 140, 40);
    assert!(
        !parent_debug.contains("Inspect child prompt that belongs to the subagent"),
        "parent transcript should hide the child prompt immediately after submission: {parent_debug}"
    );

    app.ingest_event(envelope_with_actor(
        7,
        "req_child",
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_child_turn".to_string().into(),
            state: TaskScheduleState::Queued,
            queue_key: Some("provider_model:default:model-child".to_string()),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope_with_actor(
        8,
        "req_child",
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_child".into(),
            provider_id: "default".to_string(),
            model_id: "model-child".to_string(),
            prompt_summary: "Inspect child prompt that belongs to the subagent".to_string(),
            request_digest: "digest-child-prompt".to_string(),
            metadata: None,
        }),
    ));
    app.navigate_to_child_session_id("agent_child".to_string());
    assert_eq!(app.current_session_id(), Some("agent_child"));
    assert!(
        render_text(&app, 140, 40).contains("Inspect child prompt that belongs to the subagent")
    );
    app.navigate_to_parent_session();
    assert_eq!(app.current_session_id(), Some("parent"));
    let parent_debug = render_debug(&app, 140, 40);
    assert!(
        !parent_debug.contains("Inspect child prompt that belongs to the subagent"),
        "returning to the parent must preserve child ownership before the spawn finishes: {parent_debug}"
    );

    app.ingest_event(envelope(
        9,
        "req_parent",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_child_pending".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("child task scheduled".to_string()),
            output_digest: Some("digest-child-output".to_string()),
            output_json: Some(serde_json::json!({
                "child_session_id": "agent_child",
                "child_request_id": "req_child",
            })),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("task".to_string()),
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: Some("tc_child_pending".to_string()),
                    parent_request_id: Some("req_parent".to_string()),
                    child_session_id: Some("agent_child".to_string()),
                    child_request_id: Some("req_child".to_string()),
                    ..TaskLineageMetadata::default()
                }),
                ..ToolCallMetadata::default()
            }),
        }),
    ));

    assert!(app
        .activities
        .iter()
        .any(|activity| activity.request_id == "req_child"));
    let parent_debug = render_debug(&app, 140, 40);
    assert!(
        !parent_debug.contains("Inspect child prompt that belongs to the subagent"),
        "parent transcript should hide child prompts before the task tool finishes: {parent_debug}"
    );

    app.navigate_to_child_session_id("agent_child".to_string());
    let child_debug = render_debug(&app, 140, 40);
    assert!(
        child_debug.contains("Inspect child prompt that belongs to the subagent"),
        "the inline child session should still render its own prompt: {child_debug}"
    );

    let mut followup = envelope_with_actor(
        10,
        "req_child_followup",
        EventActor::new(ActorKind::Supervisor, None),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_child_followup".into(),
            text: "Child followup prompt".to_string(),
        }),
    );
    followup.stream_key = Some("agent:agent_child".to_string());
    app.ingest_event(followup);
    app.ingest_event(envelope_with_actor(
        11,
        "req_child_followup",
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_child_followup".into(),
            provider_id: "default".to_string(),
            model_id: "model-child".to_string(),
            prompt_summary: "Child followup prompt".to_string(),
            request_digest: "digest-child-followup".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_runtime_event(harness_core::event::RuntimeEvent::Live(Box::new(
        harness_core::event::LiveEventEnvelope {
            event_id: "live-child-followup".to_string(),
            run_id: "run_app_tests".into(),
            mono_ms: 11,
            ts: None,
            actor: EventActor::new(ActorKind::Worker, None),
            correlation_id: Some("req_child_followup".to_string()),
            causation_id: None,
            stream_key: Some("agent:agent_child".to_string()),
            payload: harness_core::event::LiveEventV1::ProviderTextDelta {
                request_id: "req_child_followup".into(),
                delta: "Child followup response".to_string(),
            },
        },
    )));
    let child_text = render_text(&app, 140, 40);
    assert!(child_text.contains("Child followup prompt"));
    assert!(child_text.contains("Child followup response"));
    app.ingest_event(envelope_with_actor(
        12,
        "req_child_followup",
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        EventV1::AssistantMessageFinished(harness_core::event::AssistantMessageFinishedEvent {
            request_id: "req_child_followup".into(),
            tool_call_count: 0,
            parts: vec![harness_core::session::AssistantPart::Text {
                text: "Child followup response".to_string(),
            }],
            provenance: None,
            assistant_message: None,
        }),
    ));
    let completed_child_text = render_text(&app, 140, 40);
    assert_eq!(
        completed_child_text
            .matches("Child followup response")
            .count(),
        1
    );
    assert!(!completed_child_text.contains("QUEUED"));
    app.navigate_to_parent_session();
    assert_eq!(app.canonical_projection_error(), None);
    let parent_text = render_text(&app, 140, 40);
    assert!(parent_text.contains("Start parent work"));
    assert!(!parent_text.contains("Child followup"));
    assert!(!parent_text.contains("Inspect child prompt"));
    let replay = AppState::new_replay(
        app.session_path.clone().unwrap_or_abort(),
        app.events.clone(),
    );
    assert!(!render_text(&replay, 140, 40).contains("Child followup"));
    app.navigate_to_child_session_id("agent_child".to_string());
    assert_eq!(app.canonical_projection_error(), None);
    assert!(render_text(&app, 140, 40).contains("Child followup response"));
}

#[test]
fn nested_subagent_navigation_keeps_updates_in_their_own_sessions() {
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let parent_path = run_dir.path().join("parent");
    fs::create_dir_all(&parent_path).unwrap_or_abort();
    let delta = |seq, request: &str, text: &str| {
        envelope(
            seq,
            request,
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request.into(),
                delta: text.to_string(),
            }),
        )
    };
    let mut app = AppState::new_live(Some(parent_path), false, None);
    for event in [
        run_started(1),
        agent_spawned(2, "parent", "build"),
        provider_started(3, "req_parent", "mock", "model-parent"),
        child_task_requested(4, "req_parent", "tc_child", "child", "req_child"),
        child_agent_spawned(5, "child", "explore", "parent"),
        provider_started(6, "req_child", "mock", "model-child"),
        delta(7, "req_child", "Child transcript"),
        child_task_requested(
            8,
            "req_child",
            "tc_grandchild",
            "grandchild",
            "req_grandchild",
        ),
        child_agent_spawned(9, "grandchild", "explore", "child"),
        provider_started(10, "req_grandchild", "mock", "model-grandchild"),
        delta(11, "req_grandchild", "Grandchild transcript"),
    ] {
        app.ingest_event(event);
    }
    app.navigate_to_child_session_id("child".to_string());
    app.navigate_to_child_session_id("grandchild".to_string());
    assert_eq!(app.current_session_id(), Some("grandchild"));
    assert!(render_text(&app, 140, 40).contains("Grandchild transcript"));

    app.ingest_event(delta(12, "req_parent", "Parent update while nested"));
    app.ingest_event(delta(13, "req_child", " and child update while nested"));
    app.ingest_event(delta(
        14,
        "req_grandchild",
        " and grandchild update while nested",
    ));
    let grandchild = render_text(&app, 140, 40);
    assert!(grandchild.contains("Grandchild transcript and grandchild update while nested"));
    assert!(!grandchild.contains("Parent update while nested"));
    assert!(!grandchild.contains("Child transcript"));
    app.ingest_event(child_task_requested(
        15,
        "req_child",
        "tc_second_grandchild",
        "grandchild2",
        "req_grandchild2",
    ));

    app.navigate_to_parent_session();
    assert_eq!(app.current_session_id(), Some("child"));
    assert_eq!(
        app.current_subagent_session_info().unwrap_or_abort().total,
        1
    );
    let child = render_text(&app, 140, 40);
    assert!(child.contains("Child transcript"));
    assert!(child.contains("child update while nested"), "{child}");
    assert!(!child.contains("Grandchild transcript"));
    assert!(!child.contains("Parent update while nested"));
    app.navigate_to_parent_session();
    let parent = render_text(&app, 140, 40);
    assert!(parent.contains("Parent update while nested"));
    assert!(!parent.contains("Child transcript"));
    assert!(!parent.contains("Grandchild transcript"));
}
