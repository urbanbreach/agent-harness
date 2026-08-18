use super::*;

fn group_surface_text(surface: &ResolvedTranscriptVisualEntryDraft) -> String {
    surface
        .lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect()
}

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
fn grammar_single_context_tool_uses_a_semantic_group_header() {
    let mut turn = canonical_turn();
    turn.assistant_parts = vec![grammar_tool(
        "read-1",
        "read",
        ToolCallPresentationStatus::Succeeded,
    )];

    let spec = normalized_tool_spec(&turn, TranscriptToolFamily::Group);

    assert_eq!(spec.grouping.member_count, 1);
}

#[test]
fn grammar_running_context_group_uses_heavy_animated_rail_and_hides_members() {
    let theme = Theme::default();
    let mut turn = canonical_turn();
    turn.assistant_parts = vec![
        grammar_tool("read-1", "read", ToolCallPresentationStatus::Succeeded),
        grammar_tool("search-1", "grep", ToolCallPresentationStatus::Running),
    ];
    let TranscriptAssistantPart::ToolCall(running) = &mut turn.assistant_parts[1] else {
        panic!("tool")
    };
    running.rail_motion = ToolRailMotion::Running {
        elapsed: std::time::Duration::ZERO,
        sampled_phase: 0,
    };

    let surfaces = build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.shell);
    let group = surfaces
        .iter()
        .find(|surface| {
            matches!(
                surface.metadata.id,
                TranscriptVisualEntryId::ToolGroup { .. }
            )
        })
        .expect("group");

    assert_eq!(group.lines.len(), 1);
    assert_eq!(group.rail_glyph, "┃");
    assert!(group.show_outer_rail);
    assert!(group.tool_rail_motion.is_some());
    assert!(group_surface_text(group).contains("◈ Reading 1 file, Searching 1 pattern"));
}

#[test]
fn grammar_failed_context_group_uses_error_rail_and_suffix() {
    let theme = Theme::default();
    let mut turn = canonical_turn();
    turn.assistant_parts = vec![
        grammar_tool("read-1", "read", ToolCallPresentationStatus::Running),
        grammar_tool("search-1", "grep", ToolCallPresentationStatus::Failed),
    ];
    let TranscriptAssistantPart::ToolCall(running) = &mut turn.assistant_parts[0] else {
        panic!("tool")
    };
    running.rail_motion = ToolRailMotion::Running {
        elapsed: std::time::Duration::ZERO,
        sampled_phase: 0,
    };

    let surfaces = build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.shell);
    let group = surfaces
        .iter()
        .find(|surface| {
            matches!(
                surface.metadata.id,
                TranscriptVisualEntryId::ToolGroup { .. }
            )
        })
        .expect("group");

    assert_eq!(group.rail_glyph, "┃");
    assert_eq!(group.rail_color, theme.status.error);
    assert!(group.tool_rail_motion.is_none());
    assert!(group_surface_text(group).contains(" · 1 failed"));
}

#[test]
fn grammar_settled_context_group_uses_dim_collapsed_rail() {
    let theme = Theme::default();
    let mut turn = canonical_turn();
    turn.assistant_parts = vec![grammar_tool(
        "read-1",
        "read",
        ToolCallPresentationStatus::Succeeded,
    )];

    let surfaces = build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.shell);
    let group = surfaces
        .iter()
        .find(|surface| {
            matches!(
                surface.metadata.id,
                TranscriptVisualEntryId::ToolGroup { .. }
            )
        })
        .expect("group");

    assert_eq!(group.rail_glyph, "❙");
    assert_ne!(group.rail_color, theme.status.success);
    assert!(group.show_outer_rail);
}

#[test]
fn grammar_expanded_context_group_renders_header_above_every_member() {
    let theme = Theme::default();
    let mut turn = canonical_turn();
    turn.assistant_parts = vec![
        grammar_tool("read-1", "read", ToolCallPresentationStatus::Succeeded),
        grammar_tool("search-1", "grep", ToolCallPresentationStatus::Succeeded),
    ];
    for part in &mut turn.assistant_parts {
        let TranscriptAssistantPart::ToolCall(tool) = part else {
            panic!("tool")
        };
        tool.expanded = true;
    }

    let surfaces = build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.shell);
    let group = surfaces
        .iter()
        .find(|surface| {
            matches!(
                surface.metadata.id,
                TranscriptVisualEntryId::ToolGroup { .. }
            )
        })
        .expect("group");
    let rows = group
        .lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    assert_eq!(rows.len(), 3);
    assert!(rows[0].contains("◈ Read 1 file, Searched 1 pattern"));
    assert!(rows[1].contains("◆ read"));
    assert!(rows[2].contains("◆ grep"));
}

#[test]
fn grammar_cancelled_context_group_is_settled_not_failed() {
    let theme = Theme::default();
    let mut turn = canonical_turn();
    turn.assistant_parts = vec![grammar_tool(
        "read-1",
        "read",
        ToolCallPresentationStatus::Cancelled,
    )];

    let surfaces = build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.shell);
    let group = surfaces
        .iter()
        .find(|surface| {
            matches!(
                surface.metadata.id,
                TranscriptVisualEntryId::ToolGroup { .. }
            )
        })
        .expect("group");

    assert_eq!(group.rail_glyph, "❙");
    assert_eq!(
        group.metadata.lifecycle,
        TranscriptVisualEntryLifecycle::Settled
    );
    assert!(!group_surface_text(group).contains("failed"));
}

#[test]
fn grammar_preserves_rails_for_context_groups_separated_by_an_edit() {
    let theme = Theme::default();
    let mut turn = canonical_turn();
    turn.assistant_parts = vec![
        grammar_tool("read-1", "read", ToolCallPresentationStatus::Succeeded),
        grammar_tool("edit-1", "edit", ToolCallPresentationStatus::Succeeded),
        grammar_tool("search-1", "grep", ToolCallPresentationStatus::Succeeded),
    ];

    let surfaces = build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.shell);
    let context_groups = surfaces
        .iter()
        .filter(|surface| {
            matches!(
                surface.metadata.id,
                TranscriptVisualEntryId::ToolGroup { .. }
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(context_groups.len(), 2);
    assert!(context_groups.iter().all(|surface| surface.show_outer_rail));
}

#[test]
fn grammar_context_group_header_stays_within_responsive_widths() {
    let theme = Theme::default();
    let mut turn = canonical_turn();
    turn.assistant_parts = vec![
        grammar_tool("read-1", "read", ToolCallPresentationStatus::Succeeded),
        grammar_tool("search-1", "grep", ToolCallPresentationStatus::Succeeded),
        grammar_tool("list-1", "list", ToolCallPresentationStatus::Succeeded),
        grammar_tool("skill-1", "skill", ToolCallPresentationStatus::Succeeded),
        grammar_tool(
            "webfetch-1",
            "webfetch",
            ToolCallPresentationStatus::Succeeded,
        ),
    ];

    for width in [60, 79, 80, 100, 120, 132] {
        let surfaces = build_transcript_render_surfaces(&turn, &theme, width, theme.surface.shell);
        let group = surfaces
            .iter()
            .find(|surface| {
                matches!(
                    surface.metadata.id,
                    TranscriptVisualEntryId::ToolGroup { .. }
                )
            })
            .expect("group");
        assert!(
            group
                .lines
                .iter()
                .all(|line| line.width() <= usize::from(width)),
            "context group overflowed at width {width}"
        );
    }
}

#[test]
fn grammar_context_tool_disclosure_collapses_preview_and_preserves_expand() {
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
    assert_eq!(policy.disclosure, TranscriptToolDisclosure::Collapsed);
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
