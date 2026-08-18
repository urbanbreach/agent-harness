use super::*;

fn reasoning_turn(text: &str) -> TranscriptTurnSection {
    let mut turn = canonical_turn();
    turn.header.status = ActivityStatus::Streaming;
    turn.show_footer = false;
    turn.reasoning_expanded = true;
    turn.assistant_parts = vec![TranscriptAssistantPart::Reasoning(
        TranscriptLabeledTextSection {
            label: "Thinking",
            text: text.into(),
        },
    )];
    turn
}

fn reasoning_surface(
    surfaces: &[ResolvedTranscriptVisualEntryDraft],
) -> &ResolvedTranscriptVisualEntryDraft {
    surfaces
        .iter()
        .find(|surface| surface.kind == TranscriptRenderSurfaceKind::AssistantReasoning)
        .expect("reasoning surface")
}

#[test]
fn wrapped_reasoning_selection_preserves_logical_paragraph_continuations() {
    let theme = Theme::default();
    let turn = reasoning_turn("alpha beta gamma delta epsilon zeta eta theta iota kappa");

    let surfaces = build_transcript_render_surfaces(&turn, &theme, 24, theme.surface.shell);
    let rows = reasoning_surface(&surfaces)
        .selection_rows
        .as_ref()
        .expect("reasoning selection rows");
    let body_rows = rows[2..]
        .iter()
        .filter(|row| row.cells.iter().any(|cell| !cell.trim().is_empty()))
        .collect::<Vec<_>>();

    assert!(body_rows.len() > 1, "fixture must wrap onto multiple rows");
    assert!(!body_rows[0].continues_previous);
    assert!(body_rows[1..].iter().all(|row| row.continues_previous));
}

#[test]
fn block_quote_selection_excludes_visual_quote_bar() {
    let theme = Theme::default();
    let turn = reasoning_turn("> quoted reasoning");

    let surfaces = build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.shell);
    let row = &reasoning_surface(&surfaces)
        .selection_rows
        .as_ref()
        .expect("reasoning selection rows")[2];
    let copied = row.cells[row.copy_offset..].concat();

    assert_eq!(copied.trim_end(), "quoted reasoning");
}
