use super::*;

fn normalized_footer_spec(turn: &TranscriptTurnSection) -> TranscriptBlockSpec {
    normalize_turn_blocks(turn)
        .into_iter()
        .find(|spec| spec.role == TranscriptBlockRole::Footer)
        .expect("normalized footer")
}

#[test]
fn grammar_permission_footer_uses_typed_pin_and_outdent() {
    let mut turn = canonical_turn();
    turn.show_footer = true;
    turn.assistant_parts = vec![grammar_tool(
        "permission",
        "write",
        ToolCallPresentationStatus::Waiting,
    )];
    let spec = normalized_footer_spec(&turn);
    let TranscriptBlockContent::Footer { content, .. } = &spec.content else {
        panic!("footer")
    };
    assert!(matches!(
        content,
        TranscriptFooterContent::Permission { tool_id, .. } if tool_id == "write"
    ));
    assert_eq!(
        spec.placement,
        TranscriptBlockPlacement::PinnedFooter { outdent_cells: 1 }
    );
    assert_eq!(spec.motion, TranscriptBlockMotionDemand::None);
}

#[test]
fn grammar_question_footer_uses_typed_pin_and_frozen_motion() {
    let mut turn = canonical_turn();
    turn.show_footer = true;
    turn.assistant_parts = vec![grammar_tool(
        "question",
        "question",
        ToolCallPresentationStatus::Waiting,
    )];
    let spec = normalized_footer_spec(&turn);
    let TranscriptBlockContent::Footer { content, .. } = &spec.content else {
        panic!("footer")
    };
    assert!(matches!(content, TranscriptFooterContent::Question { .. }));
    assert_eq!(
        spec.placement,
        TranscriptBlockPlacement::PinnedFooter { outdent_cells: 1 }
    );
    assert_eq!(spec.motion, TranscriptBlockMotionDemand::None);
}

#[test]
fn grammar_prompt_copy_independent_pin_preserves_geometry() {
    let mut turn = canonical_turn();
    turn.show_footer = true;
    turn.assistant_parts = vec![grammar_tool(
        "permission-copy",
        "write",
        ToolCallPresentationStatus::Waiting,
    )];
    let spec = normalized_footer_spec(&turn);
    let surface = TranscriptRenderSurface {
        kind: TranscriptRenderSurfaceKind::AssistantFooter,
        leading_gap_rows: 0,
        placement: TranscriptBlockPlacement::Flow,
        show_outer_rail: false,
        rail_glyph: "│",
        rail_color: ratatui::style::Color::Reset,
        surface: ratatui::style::Color::Reset,
        lines: vec![ratatui::text::Line::from("completely alternate copy")],
        interaction_rows: None,
        selection_rows: None,
        diff_hunk_offsets: Vec::new(),
        selected_rail: false,
        tool_rail_motion: None,
    };
    let resolved = resolve_block_surface(&spec, surface).expect("typed footer resolves");
    assert_eq!(resolved.lines.len(), 1);
    assert_eq!(resolved.placement, spec.placement);
}

#[test]
fn grammar_prompt_dismiss_restores_state_without_footer_copy() {
    let mut turn = canonical_turn();
    turn.show_footer = true;
    turn.assistant_parts = vec![grammar_tool(
        "permission-dismiss",
        "write",
        ToolCallPresentationStatus::Waiting,
    )];
    let pinned = normalized_footer_spec(&turn);
    let user_id = normalize_turn_blocks(&turn)[0].id.clone();
    let TranscriptAssistantPart::ToolCall(tool) = &mut turn.assistant_parts[0] else {
        panic!("tool")
    };
    tool.header.presentation.status = ToolCallPresentationStatus::Succeeded;
    let restored = normalized_footer_spec(&turn);
    assert_eq!(normalize_turn_blocks(&turn)[0].id, user_id);
    assert_eq!(restored.placement, TranscriptBlockPlacement::Flow);
    assert_ne!(pinned.placement, restored.placement);
}

fn footer_lifecycle_state(spec: &TranscriptBlockSpec) -> TranscriptLifecycleState {
    let TranscriptBlockContent::Footer { state, .. } = &spec.content else {
        panic!("footer")
    };
    *state
}

#[test]
fn grammar_lifecycle_transitions_keep_stable_id_and_typed_placement() {
    let mut turn = canonical_turn();
    turn.show_footer = true;
    turn.assistant_parts.clear();
    turn.header.status = ActivityStatus::Queued;
    let queued = normalized_footer_spec(&turn);
    assert_eq!(
        footer_lifecycle_state(&queued),
        TranscriptLifecycleState::Queued
    );
    assert_eq!(queued.placement, TranscriptBlockPlacement::Flow);

    turn.header.status = ActivityStatus::Streaming;
    turn.header.provider_request_open = true;
    let responding = normalized_footer_spec(&turn);
    assert_eq!(queued.id, responding.id);
    assert_eq!(
        footer_lifecycle_state(&responding),
        TranscriptLifecycleState::Responding
    );
    assert_eq!(
        responding.placement,
        TranscriptBlockPlacement::PinnedFooter { outdent_cells: 0 }
    );

    turn.header.retry = Some(ProviderRequestRetryMetadata {
        attempt: 2,
        max_attempts: 4,
        delay_ms: Some(50),
        category: None,
    });
    turn.header.retry_elapsed_ms = Some(25);
    let retrying = normalized_footer_spec(&turn);
    assert_eq!(queued.id, retrying.id);
    assert!(matches!(
        footer_lifecycle_state(&retrying),
        TranscriptLifecycleState::Retrying {
            attempt: 2,
            max_attempts: 4,
            ..
        }
    ));
    assert_eq!(
        retrying.placement,
        TranscriptBlockPlacement::PinnedFooter { outdent_cells: 0 }
    );
}

#[test]
fn grammar_lifecycle_failure_cancel_recovery_and_completion() {
    let mut turn = canonical_turn();
    turn.show_footer = true;
    turn.header.status = ActivityStatus::Error;
    turn.assistant_parts = vec![TranscriptAssistantPart::Error(TranscriptErrorSection {
        text: "provider failed safely".into(),
    })];
    assert_eq!(
        footer_lifecycle_state(&normalized_footer_spec(&turn)),
        TranscriptLifecycleState::Failed
    );
    let error = normalize_turn_blocks(&turn)
        .into_iter()
        .find(|spec| spec.role == TranscriptBlockRole::Error)
        .expect("error block");
    assert!(error.chrome.accent);

    turn.assistant_parts = vec![TranscriptAssistantPart::Error(TranscriptErrorSection {
        text: "Turn cancelled by user in 1s.".into(),
    })];
    assert_eq!(
        footer_lifecycle_state(&normalized_footer_spec(&turn)),
        TranscriptLifecycleState::Cancelled
    );

    turn.header.status = ActivityStatus::Done;
    turn.header.retry = Some(ProviderRequestRetryMetadata {
        attempt: 1,
        max_attempts: 3,
        delay_ms: None,
        category: None,
    });
    turn.assistant_parts.clear();
    let recovered = normalized_footer_spec(&turn);
    assert_eq!(
        footer_lifecycle_state(&recovered),
        TranscriptLifecycleState::Recovered
    );
    assert_eq!(recovered.motion, TranscriptBlockMotionDemand::None);
    turn.header.retry = None;
    assert_eq!(
        footer_lifecycle_state(&normalized_footer_spec(&turn)),
        TranscriptLifecycleState::Completed
    );
}

#[test]
fn grammar_lifecycle_missing_retry_metadata_fails_closed() {
    let mut turn = canonical_turn();
    turn.show_footer = true;
    turn.header.status = ActivityStatus::Streaming;
    turn.header.retry = None;
    turn.header.retry_elapsed_ms = Some(99);
    assert_eq!(
        footer_lifecycle_state(&normalized_footer_spec(&turn)),
        TranscriptLifecycleState::Responding
    );
}

#[test]
fn grammar_lifecycle_reduced_motion_has_zero_idle_demand() {
    let mut turn = canonical_turn();
    turn.show_footer = true;
    turn.header.status = ActivityStatus::Done;
    let settled = normalized_footer_spec(&turn);
    assert_eq!(settled.motion, TranscriptBlockMotionDemand::None);
    assert_eq!(
        footer_lifecycle_state(&settled),
        TranscriptLifecycleState::Completed
    );
}
