use super::*;

#[test]
fn grammar_tool_family_classifies_reference_shaped_strategies() {
    let mut turn = canonical_turn();

    for (tool_id, expected) in [
        ("read", TranscriptToolFamily::Read),
        ("grep", TranscriptToolFamily::Search),
        ("glob", TranscriptToolFamily::Search),
        ("list", TranscriptToolFamily::List),
        ("bash", TranscriptToolFamily::Execute),
        ("edit", TranscriptToolFamily::Edit),
        ("web.fetch", TranscriptToolFamily::Web),
        ("search.web", TranscriptToolFamily::Web),
        ("task", TranscriptToolFamily::Task),
        ("vendor.magic", TranscriptToolFamily::Unknown),
    ] {
        turn.assistant_parts = vec![grammar_tool(
            tool_id,
            tool_id,
            ToolCallPresentationStatus::Succeeded,
        )];

        let spec = normalized_tool_spec(&turn, expected);

        assert!(matches!(
            spec.content,
            TranscriptBlockContent::Tool { family, .. } if family == expected
        ));
    }
}

#[test]
fn grammar_settled_execute_and_edit_rows_have_no_permanent_rail() {
    let mut turn = canonical_turn();

    for (tool_id, family) in [
        ("bash", TranscriptToolFamily::Execute),
        ("edit", TranscriptToolFamily::Edit),
    ] {
        turn.assistant_parts = vec![grammar_tool(
            tool_id,
            tool_id,
            ToolCallPresentationStatus::Succeeded,
        )];

        let spec = normalized_tool_spec(&turn, family);

        assert!(!spec.chrome.rail);
        assert!(!spec.chrome.accent);
        assert_eq!(spec.motion, TranscriptBlockMotionDemand::None);
    }
}

#[test]
fn grammar_only_running_tool_requests_active_motion() {
    let mut turn = canonical_turn();
    let mut running = grammar_tool("run", "bash", ToolCallPresentationStatus::Running);
    if let TranscriptAssistantPart::ToolCall(tool) = &mut running {
        tool.rail_motion = ToolRailMotion::Running {
            elapsed: std::time::Duration::from_millis(20),
            sampled_phase: 1,
        };
    }
    turn.assistant_parts = vec![running];

    let spec = normalized_tool_spec(&turn, TranscriptToolFamily::Execute);

    assert!(spec.chrome.accent);
    assert_eq!(spec.motion, TranscriptBlockMotionDemand::Active);
}

#[test]
fn grammar_queued_only_group_does_not_request_active_motion() {
    let mut turn = canonical_turn();
    turn.assistant_parts = vec![
        grammar_tool("read-1", "read", ToolCallPresentationStatus::Queued),
        grammar_tool("read-2", "read", ToolCallPresentationStatus::Queued),
    ];

    let spec = normalized_tool_spec(&turn, TranscriptToolFamily::Group);

    assert_eq!(spec.motion, TranscriptBlockMotionDemand::None);
    assert!(!spec.chrome.accent);
}

#[test]
fn rendered_singleton_queued_tool_has_state_rail_without_motion() {
    let theme = Theme::default();
    let mut turn = canonical_turn();
    turn.assistant_parts = vec![grammar_tool(
        "queued-run",
        "bash",
        ToolCallPresentationStatus::Queued,
    )];

    let surfaces = build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.shell);
    let tool = surfaces
        .iter()
        .find(|surface| {
            surface.kind == TranscriptRenderSurfaceKind::AssistantCommandTool
                || surface.kind == TranscriptRenderSurfaceKind::AssistantTool
        })
        .expect("queued tool surface");

    assert!(tool.show_outer_rail);
    assert_eq!(tool.tool_rail_motion, None);
}

#[test]
fn rendered_running_group_propagates_motion_to_visible_scheduler() {
    let theme = Theme::default();
    let mut turn = canonical_turn();
    let mut running = grammar_tool("read-1", "read", ToolCallPresentationStatus::Running);
    if let TranscriptAssistantPart::ToolCall(tool) = &mut running {
        tool.rail_motion = ToolRailMotion::Running {
            elapsed: std::time::Duration::from_millis(20),
            sampled_phase: 1,
        };
    }
    turn.assistant_parts = vec![
        running,
        grammar_tool("read-2", "read", ToolCallPresentationStatus::Succeeded),
    ];

    let layout = measure_transcript_layout(
        std::slice::from_ref(&turn),
        &theme,
        80,
        theme.surface.shell,
        |section| section.activity_first_seq,
        |_, _| None,
        build_transcript_render_surfaces,
    );
    let tool = layout.sections[0]
        .surfaces
        .iter()
        .find(|surface| surface.kind == TranscriptRenderSurfaceKind::AssistantTool)
        .expect("running group surface");

    assert!(tool.show_outer_rail);
    assert!(matches!(
        tool.tool_rail_motion,
        Some(ToolRailMotion::Running { .. })
    ));
    assert!(transcript_layout_has_visible_running_tool(&layout, 40, 0));
}
