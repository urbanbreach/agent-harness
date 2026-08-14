use super::*;

#[test]
fn grammar_generic_tool_group_preserves_dense_stable_identity() {
    let mut turn = canonical_turn();
    turn.assistant_parts = (0..11)
        .map(|index| {
            grammar_tool(
                &format!("read-{index}"),
                "read",
                ToolCallPresentationStatus::Succeeded,
            )
        })
        .collect();
    turn.assistant_parts.insert(
        3,
        TranscriptAssistantPart::Reasoning(TranscriptLabeledTextSection {
            label: "Thought",
            text: "completed".into(),
        }),
    );
    let spec = normalized_tool_spec(&turn, TranscriptToolFamily::Group);
    let TranscriptBlockContent::Tool { ids, policy, .. } = &spec.content else {
        panic!("group spec")
    };
    assert_eq!(ids.len(), 11);
    assert_eq!(policy.member_count, 11);
    assert_eq!(policy.group_class, Some(TranscriptToolGroupClass::Context));
    assert!(spec.grouping.group_id.is_some());
}

#[test]
fn grammar_generic_tool_disclosure_preserves_preview_and_expand() {
    let mut turn = canonical_turn();
    turn.assistant_parts = vec![
        grammar_tool("read-1", "read", ToolCallPresentationStatus::Running),
        grammar_tool("read-2", "read", ToolCallPresentationStatus::Succeeded),
    ];
    let TranscriptAssistantPart::ToolCall(first) = &mut turn.assistant_parts[0] else {
        panic!("tool")
    };
    first.details_preview_visible = true;
    let preview = normalized_tool_spec(&turn, TranscriptToolFamily::Group);
    let TranscriptBlockContent::Tool { policy, .. } = preview.content else {
        panic!("tool policy")
    };
    assert_eq!(policy.disclosure, TranscriptToolDisclosure::Preview);
    let TranscriptAssistantPart::ToolCall(first) = &mut turn.assistant_parts[0] else {
        panic!("tool")
    };
    first.expanded = true;
    let expanded = normalized_tool_spec(&turn, TranscriptToolFamily::Group);
    let TranscriptBlockContent::Tool { policy, .. } = expanded.content else {
        panic!("tool policy")
    };
    assert_eq!(policy.disclosure, TranscriptToolDisclosure::Expanded);
}

#[test]
fn grammar_generic_tool_interaction_targets_group_members() {
    let theme = Theme::default();
    let mut turn = canonical_turn();
    turn.assistant_parts = vec![
        grammar_tool("read-1", "read", ToolCallPresentationStatus::Running),
        grammar_tool("read-2", "read", ToolCallPresentationStatus::Failed),
    ];
    let surfaces = build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.shell);
    let group = surfaces
        .iter()
        .find(|surface| surface.kind == TranscriptRenderSurfaceKind::AssistantTool)
        .expect("group");
    let target = group
        .interaction_rows
        .as_ref()
        .and_then(|rows| rows.first())
        .and_then(Option::as_ref)
        .expect("group target");
    assert!(matches!(
        target.target,
        TranscriptMouseTarget::ToolGroup { .. }
    ));
}

#[test]
fn grammar_generic_tool_rejects_missing_id() {
    let mut turn = canonical_turn();
    turn.assistant_parts = vec![
        grammar_tool("read-1", "read", ToolCallPresentationStatus::Succeeded),
        grammar_tool("read-2", "read", ToolCallPresentationStatus::Succeeded),
    ];
    let mut spec = normalized_tool_spec(&turn, TranscriptToolFamily::Group);
    spec.grouping.group_id = None;
    assert_eq!(
        validate_block_spec(&spec),
        Err(TranscriptGrammarError::InvalidGrouping)
    );
}

#[test]
fn grammar_generic_tool_rejects_cross_kind_group() {
    let mut turn = canonical_turn();
    turn.assistant_parts = vec![grammar_tool(
        "read-1",
        "read",
        ToolCallPresentationStatus::Succeeded,
    )];
    let mut spec = normalized_tool_spec(&turn, TranscriptToolFamily::Read);
    let TranscriptBlockContent::Tool { policy, .. } = &mut spec.content else {
        panic!("tool")
    };
    policy.group_class = Some(TranscriptToolGroupClass::Commands);
    assert_eq!(
        validate_block_spec(&spec),
        Err(TranscriptGrammarError::InvalidGrouping)
    );
}
