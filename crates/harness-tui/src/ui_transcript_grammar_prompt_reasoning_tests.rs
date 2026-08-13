use super::*;

#[test]
fn grammar_user_prompt_and_assistant_body_preserve_compact_cjk_rows() {
    let theme = Theme::default();
    let mut turn = canonical_turn();
    turn.user_message.as_mut().expect("user").text = "界面 🧭 prompt".into();
    turn.assistant_parts.retain(|part| {
        matches!(
            part,
            TranscriptAssistantPart::Reasoning(_) | TranscriptAssistantPart::Body(_)
        )
    });

    for width in [120, 80, 60] {
        let surfaces = build_transcript_render_surfaces(&turn, &theme, width, theme.surface.shell);
        let user = surfaces.first().expect("user surface");
        let body = surfaces
            .iter()
            .find(|surface| surface.kind == TranscriptRenderSurfaceKind::AssistantBody)
            .expect("body surface");
        assert_eq!(
            user.placement,
            TranscriptBlockPlacement::StickyPromptCandidate
        );
        assert!(!user.show_outer_rail);
        assert!(body.selection_rows.is_some());
    }
}

#[test]
fn grammar_cache_uses_normalized_prompt_body_identity() {
    let turn = canonical_turn();
    let identical = turn.clone();

    assert!(normalized_turn_cache_matches(&turn, &identical));
}

#[test]
fn grammar_prompt_body_rejects_stale_cache() {
    let turn = canonical_turn();
    let mut semantic_change = turn.clone();
    let body = semantic_change
        .assistant_parts
        .iter_mut()
        .find_map(|part| match part {
            TranscriptAssistantPart::Body(body) => Some(body),
            TranscriptAssistantPart::Reasoning(_)
            | TranscriptAssistantPart::ToolCall(_)
            | TranscriptAssistantPart::Error(_)
            | TranscriptAssistantPart::Compaction(_) => None,
        })
        .expect("body");
    *body = TranscriptBodyBlock::RichText("semantic change".into());

    assert!(!normalized_turn_cache_matches(&turn, &semantic_change));
}

#[test]
fn grammar_prompt_body_rejects_orphan_marker() {
    let theme = Theme::default();
    let mut turn = canonical_turn();
    turn.show_footer = false;
    turn.assistant_parts = vec![TranscriptAssistantPart::Body(
        TranscriptBodyBlock::RichText(String::new()),
    )];

    let surfaces = build_transcript_render_surfaces(&turn, &theme, 60, theme.surface.shell);
    let body = surfaces
        .iter()
        .find(|surface| surface.kind == TranscriptRenderSurfaceKind::AssistantBody)
        .expect("body surface");
    assert!(!body.show_outer_rail);
    assert!(body.lines.iter().all(|line| {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
            .trim()
            .is_empty()
    }));
}

fn active_reasoning_turn(text: &str) -> TranscriptTurnSection {
    let mut turn = canonical_turn();
    turn.header.status = ActivityStatus::Streaming;
    turn.show_footer = false;
    turn.reasoning_expanded = false;
    turn.assistant_parts = vec![TranscriptAssistantPart::Reasoning(
        TranscriptLabeledTextSection {
            label: "Thinking",
            text: text.into(),
        },
    )];
    turn
}

#[test]
fn grammar_reasoning_active_completed_fold_and_motion() {
    let theme = Theme::default();
    let mut turn = active_reasoning_turn("**Plan**\n\nfirst\nsecond\nthird\nfourth");
    let collapsed = build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.shell);
    let collapsed_reasoning = collapsed
        .iter()
        .find(|surface| surface.kind == TranscriptRenderSurfaceKind::AssistantReasoning)
        .expect("reasoning");
    assert!(collapsed_reasoning.show_outer_rail);

    turn.reasoning_expanded = true;
    let expanded = build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.shell);
    let expanded_reasoning = expanded
        .iter()
        .find(|surface| surface.kind == TranscriptRenderSurfaceKind::AssistantReasoning)
        .expect("expanded reasoning");
    assert!(expanded_reasoning.lines.len() > collapsed_reasoning.lines.len());

    turn.header.status = ActivityStatus::Done;
    let spec = normalize_turn_blocks(&turn)
        .into_iter()
        .find(|spec| spec.role == TranscriptBlockRole::Reasoning)
        .expect("reasoning spec");
    assert_eq!(spec.motion, TranscriptBlockMotionDemand::None);
}

#[test]
fn grammar_reasoning_selection_and_fold_anchor_survive_resize() {
    let mut turn = active_reasoning_turn("**界面 🧭**\n\nselection body");
    let before = normalize_turn_blocks(&turn)
        .into_iter()
        .find(|spec| spec.role == TranscriptBlockRole::Reasoning)
        .expect("reasoning spec");
    turn.reasoning_expanded = true;
    let after = normalize_turn_blocks(&turn)
        .into_iter()
        .find(|spec| spec.role == TranscriptBlockRole::Reasoning)
        .expect("reasoning spec");

    assert_eq!(before.id, after.id);
    assert!(before.fold.foldable && !before.fold.expanded);
    assert!(after.fold.foldable && after.fold.expanded);
    assert!(after.interaction.hoverable && after.interaction.focusable);
}

#[test]
fn grammar_reasoning_empty_emits_no_orphan_block() {
    let theme = Theme::default();
    let turn = active_reasoning_turn("[REDACTED]");
    let surfaces = build_transcript_render_surfaces(&turn, &theme, 60, theme.surface.shell);
    let reasoning = surfaces
        .iter()
        .find(|surface| surface.kind == TranscriptRenderSurfaceKind::AssistantReasoning)
        .expect("reasoning surface");

    assert!(reasoning.lines.is_empty());
    assert!(!reasoning.selected_rail);
}

#[test]
fn grammar_reasoning_reduced_motion_has_no_timer_demand() {
    let mut turn = active_reasoning_turn("active reasoning");
    turn.motion_enabled = false;
    let spec = normalize_turn_blocks(&turn)
        .into_iter()
        .find(|spec| spec.role == TranscriptBlockRole::Reasoning)
        .expect("reasoning spec");

    assert_eq!(spec.motion, TranscriptBlockMotionDemand::None);
}

#[test]
fn grammar_reasoning_question_and_permission_force_completion() {
    let canonical = canonical_turn();
    let waiting_tool = canonical
        .assistant_parts
        .iter()
        .find_map(|part| match part {
            TranscriptAssistantPart::ToolCall(tool) => Some(tool.clone()),
            TranscriptAssistantPart::Reasoning(_)
            | TranscriptAssistantPart::Body(_)
            | TranscriptAssistantPart::Error(_)
            | TranscriptAssistantPart::Compaction(_) => None,
        })
        .expect("tool");
    let mut turn = active_reasoning_turn("reasoning");
    let mut waiting_tool = waiting_tool;
    waiting_tool.header.presentation.status = ToolCallPresentationStatus::Waiting;
    turn.assistant_parts
        .push(TranscriptAssistantPart::ToolCall(waiting_tool));

    let spec = normalize_turn_blocks(&turn)
        .into_iter()
        .find(|spec| spec.role == TranscriptBlockRole::Reasoning)
        .expect("reasoning spec");
    assert_eq!(spec.motion, TranscriptBlockMotionDemand::None);
}

#[test]
fn grammar_reasoning_ascii_preserves_row_count() {
    let unicode_theme = Theme::default();
    let ascii_theme = Theme::default().with_glyph_mode(crate::theme::GlyphMode::Ascii);
    let turn = active_reasoning_turn("active reasoning");
    let unicode =
        build_transcript_render_surfaces(&turn, &unicode_theme, 80, unicode_theme.surface.shell);
    let ascii =
        build_transcript_render_surfaces(&turn, &ascii_theme, 80, ascii_theme.surface.shell);
    let reasoning_rows = |surfaces: &[TranscriptRenderSurface]| {
        surfaces
            .iter()
            .find(|surface| surface.kind == TranscriptRenderSurfaceKind::AssistantReasoning)
            .map_or(0, |surface| surface.lines.len())
    };

    assert_eq!(reasoning_rows(&unicode), reasoning_rows(&ascii));
}
