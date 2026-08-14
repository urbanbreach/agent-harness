use super::*;

#[test]
fn transcript_grammar_characterizes_all_family_order_at_80x24() {
    let contract = canonical_contract(80);

    validate_grammar_contract(&contract).unwrap_or_else(|error| panic!("{error}: {contract:#?}"));
    assert_eq!(families(&contract), GRAMMAR_ALL_FAMILIES);
}

#[test]
fn transcript_grammar_characterizes_compact_60x20_widths_and_gaps() {
    let compact = canonical_contract(60);

    validate_grammar_contract(&compact).expect("compact contract must be valid");
    assert!(compact.iter().all(|surface| surface.width <= 60));
}

#[test]
fn transcript_grammar_characterizes_prompt_reasoning_body_and_empty_content() {
    let contract = canonical_contract(80);
    let mut turn = canonical_turn();
    turn.assistant_parts.insert(
        2,
        TranscriptAssistantPart::Body(TranscriptBodyBlock::RichText(String::new())),
    );
    let empty_surfaces = build_transcript_render_surfaces(
        &turn,
        &Theme::default(),
        80,
        Theme::default().surface.shell,
    );

    assert_eq!(contract[0].placement, GrammarPlacement::StickyPrompt);
    assert!(!contract[1].rail);
    assert_eq!(contract[2].leading_gap, 1);
    assert_eq!(contract[3].leading_gap, 0);
    assert!(!turn.reasoning_expanded);
    assert_eq!(
        empty_surfaces
            .iter()
            .filter(|surface| surface.kind == TranscriptRenderSurfaceKind::AssistantBody)
            .count(),
        3
    );
}

#[test]
fn transcript_grammar_characterizes_generic_shell_diff_and_subagent_blocks() {
    let contract = canonical_contract(80);
    let tools = &contract[4..10];

    assert_eq!(tools[1].family, GrammarFamily::Command);
    assert_eq!(
        tools
            .iter()
            .map(|surface| surface.leading_gap)
            .collect::<Vec<_>>(),
        vec![1, 0, 0, 0, 0, 0]
    );
    assert!(tools.iter().any(|surface| surface.motion.is_some()));
}

#[test]
fn transcript_grammar_characterizes_permission_question_lifecycle_and_compaction() {
    let contract = canonical_contract(80);

    assert_eq!(contract[10].family, GrammarFamily::Error);
    assert_eq!(contract[11].family, GrammarFamily::Compaction);
    assert_eq!(contract[12].placement, GrammarPlacement::PinnedFooter);
    assert_eq!(contract[12].family, GrammarFamily::Footer);
}

#[test]
fn transcript_grammar_characterizes_selection_rows_backgrounds_and_motion() {
    let contract = canonical_contract(80);

    assert!(contract.iter().all(|surface| {
        surface
            .selection_rows
            .is_none_or(|rows| rows <= surface.line_rows)
    }));
    assert!(!grammar_contract_has_background_transition(&contract));
    assert!(
        contract
            .iter()
            .filter(|surface| surface.motion.is_some())
            .count()
            <= 1,
        "only the selected or active semantic entry may retain rail motion"
    );
}

#[test]
fn transcript_grammar_characterizes_cancel_and_retry_lifecycle() {
    let theme = Theme::default();
    let mut turn = canonical_turn();
    turn.header.status = ActivityStatus::Error;
    turn.header.retry = Some(harness_core::event::ProviderRequestRetryMetadata {
        attempt: 1,
        max_attempts: 3,
        delay_ms: Some(1_000),
        category: None,
    });
    let tool = turn
        .assistant_parts
        .iter_mut()
        .find_map(|part| match part {
            TranscriptAssistantPart::ToolCall(tool) => Some(tool),
            _ => None,
        })
        .expect("tool exists");
    tool.header.presentation.status = ToolCallPresentationStatus::Cancelled;

    let surfaces = build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.shell);

    assert_eq!(
        surfaces.last().map(|surface| surface.kind),
        Some(TranscriptRenderSurfaceKind::AssistantFooter)
    );
    assert!(surfaces
        .iter()
        .any(|surface| surface.kind == TranscriptRenderSurfaceKind::AssistantError));
}

#[test]
fn transcript_grammar_characterizes_recovery_completion_and_cache_reuse() {
    let app = transcript_grammar_test_app();
    reset_transcript_section_render_count_for_test();

    let first = build_measured_transcript_layout_for_width(&app, app.theme(), 80);
    let first_count = transcript_section_render_count_for_test();
    let second = build_measured_transcript_layout_for_width(&app, app.theme(), 80);

    assert!(Rc::ptr_eq(&first.sections[0], &second.sections[0]));
    assert_eq!(transcript_section_render_count_for_test(), first_count);
    assert_eq!(
        second.sections[0].surfaces[0].kind,
        TranscriptRenderSurfaceKind::User
    );
    assert_eq!(
        second.sections[0]
            .surfaces
            .last()
            .map(|surface| surface.kind),
        Some(TranscriptRenderSurfaceKind::AssistantFooter)
    );
}

#[test]
fn transcript_grammar_rejects_controlled_gap_defect() {
    let mut changed = canonical_contract(80);
    changed[2].leading_gap = 2;

    assert_grammar_contract_error(&changed, "inter-block gap drift");
}

#[test]
fn transcript_grammar_rejects_controlled_rail_defect() {
    let mut changed = canonical_contract(80);
    changed[1].rail = true;

    assert_grammar_contract_error(&changed, "reasoning rail drift");
}

#[test]
fn transcript_grammar_rejects_controlled_placement_defect() {
    let mut changed = canonical_contract(80);
    changed.last_mut().expect("footer exists").placement = GrammarPlacement::Flow;

    assert_grammar_contract_error(&changed, "footer placement drift");
}

#[test]
fn transcript_grammar_rejects_controlled_selection_defect() {
    let mut changed = canonical_contract(80);
    let surface = changed
        .iter_mut()
        .find(|surface| surface.selection_rows.is_some())
        .expect("selectable surface exists");
    surface.selection_rows = Some(surface.line_rows + 1);

    assert_grammar_contract_error(&changed, "row alignment drift");
}
