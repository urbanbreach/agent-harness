use super::*;

#[test]
fn grammar_subagent_lifecycle_preserves_identity_and_child_target() {
    let mut turn = canonical_turn();
    let mut part = grammar_tool("task-1", "task", ToolCallPresentationStatus::Running);
    let TranscriptAssistantPart::ToolCall(tool) = &mut part else {
        panic!("task")
    };
    tool.child_session_id = Some("child-1".into());
    tool.subagent_background = false;
    tool.header.visual_style = TranscriptToolCallVisualStyle::TaskInline;
    turn.assistant_parts = vec![part];
    let running = normalized_tool_spec(&turn, TranscriptToolFamily::Subagent);
    let TranscriptBlockContent::Tool { subagent, .. } = &running.content else {
        panic!("task spec")
    };
    let running_policy = subagent.as_ref().expect("subagent policy");
    assert_eq!(running_policy.child_session_id.as_deref(), Some("child-1"));
    assert_eq!(
        running_policy.lifecycle,
        TranscriptSubagentLifecycle::Running
    );
    let theme = Theme::default();
    let surfaces = build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.shell);
    assert!(surfaces.iter().any(|surface| {
        surface.interaction_rows.as_ref().is_some_and(|rows| {
            rows.iter().flatten().any(|row| {
                matches!(
                    &row.target,
                    TranscriptMouseTarget::SubagentSession { session_id }
                        if session_id == "child-1"
                )
            })
        })
    }));

    let TranscriptAssistantPart::ToolCall(tool) = &mut turn.assistant_parts[0] else {
        panic!("task")
    };
    tool.header.presentation.status = ToolCallPresentationStatus::Succeeded;
    let completed = normalized_tool_spec(&turn, TranscriptToolFamily::Subagent);
    let TranscriptBlockContent::Tool { subagent, .. } = &completed.content else {
        panic!("task spec")
    };
    assert_eq!(running.id, completed.id);
    assert_eq!(
        subagent.as_ref().expect("subagent policy").lifecycle,
        TranscriptSubagentLifecycle::Completed
    );
}

#[test]
fn grammar_subagent_background_and_truncated_result_policy() {
    let mut turn = canonical_turn();
    let mut part = grammar_tool("task-bg", "agent.spawn", ToolCallPresentationStatus::Failed);
    let TranscriptAssistantPart::ToolCall(tool) = &mut part else {
        panic!("task")
    };
    tool.subagent_background = true;
    tool.output_truncated = true;
    tool.detail_blocks = vec![TranscriptToolCallDetailBlock::Message {
        text: "界面🧭".repeat(128),
        tone: TranscriptToolCallDetailTone::Primary,
    }];
    tool.expanded = true;
    turn.assistant_parts = vec![part];
    let spec = normalized_tool_spec(&turn, TranscriptToolFamily::Subagent);
    let TranscriptBlockContent::Tool { subagent, .. } = spec.content else {
        panic!("task spec")
    };
    let policy = subagent.expect("subagent policy");
    assert_eq!(policy.mode, TranscriptSubagentMode::Background);
    assert_eq!(policy.lifecycle, TranscriptSubagentLifecycle::Failed);
    assert!(policy.output_truncated);
    let theme = Theme::default();
    assert!(
        build_transcript_render_surfaces(&turn, &theme, 60, theme.surface.shell)
            .iter()
            .flat_map(|surface| &surface.lines)
            .all(|line| line.width() <= 60)
    );
}

#[test]
fn grammar_subagent_all_lifecycle_states_are_exhaustive() {
    let mut turn = canonical_turn();
    for (status, expected) in [
        (
            ToolCallPresentationStatus::Queued,
            TranscriptSubagentLifecycle::Queued,
        ),
        (
            ToolCallPresentationStatus::Running,
            TranscriptSubagentLifecycle::Running,
        ),
        (
            ToolCallPresentationStatus::Succeeded,
            TranscriptSubagentLifecycle::Completed,
        ),
        (
            ToolCallPresentationStatus::Failed,
            TranscriptSubagentLifecycle::Failed,
        ),
        (
            ToolCallPresentationStatus::Cancelled,
            TranscriptSubagentLifecycle::Cancelled,
        ),
    ] {
        turn.assistant_parts = vec![grammar_tool("task-state", "task", status)];
        let spec = normalized_tool_spec(&turn, TranscriptToolFamily::Subagent);
        let TranscriptBlockContent::Tool { subagent, .. } = spec.content else {
            panic!("task spec")
        };
        assert_eq!(subagent.expect("subagent policy").lifecycle, expected);
    }
}

#[test]
fn grammar_subagent_missing_child_id_fails_closed() {
    let mut turn = canonical_turn();
    turn.assistant_parts = vec![grammar_tool(
        "task-missing",
        "task",
        ToolCallPresentationStatus::Succeeded,
    )];
    let spec = normalized_tool_spec(&turn, TranscriptToolFamily::Subagent);
    let TranscriptBlockContent::Tool { subagent, .. } = spec.content else {
        panic!("task spec")
    };
    assert!(subagent
        .expect("subagent policy")
        .child_session_id
        .is_none());
}

#[test]
fn grammar_subagent_replay_read_only_fails_closed() {
    let mut turn = canonical_turn();
    let mut part = grammar_tool("task-replay", "task", ToolCallPresentationStatus::Succeeded);
    let TranscriptAssistantPart::ToolCall(tool) = &mut part else {
        panic!("task")
    };
    tool.child_session_id = Some("child-replay".into());
    tool.replay_read_only = true;
    turn.assistant_parts = vec![part];
    let spec = normalized_tool_spec(&turn, TranscriptToolFamily::Subagent);
    let TranscriptBlockContent::Tool { subagent, .. } = spec.content else {
        panic!("task spec")
    };
    assert!(subagent
        .expect("subagent policy")
        .navigation_target()
        .is_none());
    let theme = Theme::default();
    assert!(
        build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.shell)
            .iter()
            .flat_map(|surface| surface.interaction_rows.iter().flatten().flatten())
            .all(|row| !matches!(row.target, TranscriptMouseTarget::SubagentSession { .. }))
    );
}
