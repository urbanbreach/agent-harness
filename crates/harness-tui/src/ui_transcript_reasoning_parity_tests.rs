use ratatui::style::Modifier;

use super::*;

fn reasoning_turn(text: &str) -> TranscriptTurnSection {
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

fn reasoning_surface(
    surfaces: &[ResolvedTranscriptVisualEntryDraft],
) -> &ResolvedTranscriptVisualEntryDraft {
    surfaces
        .iter()
        .find(|surface| surface.kind == TranscriptRenderSurfaceKind::AssistantReasoning)
        .expect("reasoning surface")
}

fn line_text(line: &ratatui::text::Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

#[test]
fn active_reasoning_matches_groks_truncated_soft_wrapped_rows() {
    let theme = Theme::default();
    let turn = reasoning_turn(
        "**Plan**\n\nAnalyze the reference thinking surface.\nCompare active reasoning cells.\nVerify rail and marker motion.\nPACKET2_STREAM_SENTINEL",
    );

    let surfaces = build_transcript_render_surfaces(&turn, &theme, 120, theme.surface.shell);
    let reasoning = reasoning_surface(&surfaces);
    let rows = reasoning
        .lines
        .iter()
        .map(|line| line_text(line).trim_end().to_string())
        .collect::<Vec<_>>();

    assert_eq!(
        rows,
        [
            "   Thinking…",
            "",
            "   …",
            "",
            "   Analyze the reference thinking surface. Compare active reasoning cells. Verify rail and marker motion.",
            "   PACKET2_STREAM_SENTINEL",
        ]
    );
    assert_eq!(reasoning.lines[2].spans[0].content.as_ref(), "   ");
    assert_eq!(
        reasoning.lines[2].spans[0].style.add_modifier,
        Modifier::empty()
    );
    assert_eq!(
        reasoning.lines[2].spans[1].style.add_modifier,
        Modifier::empty()
    );
}

#[test]
fn active_reasoning_keeps_distinct_summary_parts_on_separate_rows() {
    let theme = Theme::default();
    let turn = reasoning_turn(
        "Planning worktree inspection and diff analysis\n\nInspecting uncommitted TUI feature changes",
    );

    let surfaces = build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.shell);
    let reasoning = reasoning_surface(&surfaces);
    let rows = reasoning
        .lines
        .iter()
        .map(|line| line_text(line).trim_end().to_string())
        .collect::<Vec<_>>();

    assert_eq!(
        rows,
        [
            "   Thinking…",
            "",
            "   Planning worktree inspection and diff analysis",
            "",
            "   Inspecting uncommitted TUI feature changes",
        ]
    );
    assert!(reasoning.show_outer_rail);
    assert!(matches!(
        reasoning.tool_rail_motion,
        Some(ToolRailMotion::Running { .. })
    ));
}

#[test]
fn active_reasoning_uses_the_shared_grok_wave_without_animating_its_label() {
    let theme = Theme::default();
    let turn = reasoning_turn("active reasoning");

    let surfaces = build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.shell);
    let reasoning = reasoning_surface(&surfaces);

    assert!(reasoning.show_outer_rail);
    assert_eq!(reasoning.rail_color, theme.text.tertiary);
    assert!(matches!(
        reasoning.tool_rail_motion,
        Some(ToolRailMotion::Running { .. })
    ));
    let label = reasoning.lines[0]
        .spans
        .iter()
        .find(|span| span.content.as_ref() == "Thinking…")
        .expect("reasoning label");
    assert_eq!(
        reasoning.lines[0].spans[0].style.add_modifier,
        Modifier::empty()
    );
    assert_eq!(label.style.add_modifier, Modifier::BOLD);
}

#[test]
fn active_reasoning_rebuilds_keep_the_shared_wave_phase() {
    let theme = Theme::default();
    let mut initial_turn = reasoning_turn("first reasoning delta");
    initial_turn.animation_phase = 0;
    let initial_surfaces =
        build_transcript_render_surfaces(&initial_turn, &theme, 80, theme.surface.shell);
    let initial = reasoning_surface(&initial_surfaces);

    let mut rebuilt_turn = reasoning_turn("second reasoning delta");
    rebuilt_turn.animation_phase = 10;
    rebuilt_turn.header.is_hovered = true;
    let rebuilt_surfaces =
        build_transcript_render_surfaces(&rebuilt_turn, &theme, 80, theme.surface.shell);
    let rebuilt = reasoning_surface(&rebuilt_surfaces);

    let initial_color = crate::ui::ui_transcript_surface::tool_rail_motion_color(
        initial.surface,
        initial.rail_color,
        initial.tool_rail_motion,
        0,
        10,
    );
    let rebuilt_color = crate::ui::ui_transcript_surface::tool_rail_motion_color(
        rebuilt.surface,
        rebuilt.rail_color,
        rebuilt.tool_rail_motion,
        0,
        10,
    );

    assert_eq!(initial_color, rebuilt_color);
}

#[test]
fn settled_collapsed_reasoning_is_a_single_muted_grok_header() {
    let theme = Theme::default();
    let mut turn = reasoning_turn("settled reasoning");
    turn.header.status = ActivityStatus::Done;
    turn.header.thinking_duration_ms = Some(2_300);

    let surfaces = build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.shell);
    let reasoning = reasoning_surface(&surfaces);
    let header = reasoning.lines.first().expect("reasoning header");

    assert_eq!(
        reasoning.lines.iter().map(line_text).collect::<Vec<_>>(),
        ["  Thought for 2.3s"]
    );
    assert!(!reasoning.show_outer_rail);
    assert!(reasoning.tool_rail_motion.is_none());
    let label_index = header
        .spans
        .iter()
        .position(|span| span.content.as_ref() == "Thought")
        .expect("reasoning label");
    assert_eq!(header.spans[label_index].style.add_modifier, Modifier::BOLD);
    assert!(header.spans[label_index + 1..].iter().all(|span| {
        span.style.fg == Some(theme.text.secondary) && span.style.add_modifier == Modifier::empty()
    }));
}

#[test]
fn terminal_native_reasoning_dims_muted_emphasis() {
    let theme = Theme::terminal_native();
    let turn = reasoning_turn(
        "**Plan**\n\nAnalyze the reference thinking surface.\nCompare active reasoning cells.\nVerify rail and marker motion.\nPACKET2_STREAM_SENTINEL",
    );

    let surfaces = build_transcript_render_surfaces(&turn, &theme, 120, theme.surface.shell);
    let reasoning = reasoning_surface(&surfaces);
    let label = reasoning.lines[0]
        .spans
        .iter()
        .find(|span| span.content.as_ref() == "Thinking…")
        .expect("reasoning label");
    let ellipsis = reasoning.lines[2]
        .spans
        .iter()
        .find(|span| span.content.as_ref() == "…")
        .expect("reasoning preview ellipsis");

    assert_eq!(
        (label.style.add_modifier, ellipsis.style.add_modifier),
        (Modifier::BOLD | Modifier::DIM, Modifier::DIM)
    );
}

#[test]
fn settled_expanded_reasoning_has_one_internal_gap_and_a_static_rail() {
    let theme = Theme::default();
    let mut turn = reasoning_turn("first\nsecond");
    turn.header.status = ActivityStatus::Done;
    turn.header.thinking_duration_ms = Some(2_300);
    turn.reasoning_expanded = true;

    let surfaces = build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.shell);
    let reasoning = reasoning_surface(&surfaces);
    let rows = reasoning.lines.iter().map(line_text).collect::<Vec<_>>();

    assert_eq!(rows, ["   Thought for 2.3s", "", "   first second"]);
    assert!(reasoning.show_outer_rail);
    assert!(reasoning.tool_rail_motion.is_none());
    assert_eq!(
        reasoning.rail_glyph,
        theme.live_shell.transcript_glyphs.rail
    );
}

#[test]
fn selected_folded_reasoning_replaces_the_diamond_with_groks_disclosure_indicator() {
    let theme = Theme::default();
    let mut turn = reasoning_turn("selected reasoning");
    turn.header.is_selected = true;

    let surfaces = build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.shell);
    let reasoning = reasoning_surface(&surfaces);

    assert!(line_text(&reasoning.lines[0]).starts_with("   › Thinking…"));
}

#[test]
fn hovered_folded_reasoning_uses_the_same_grok_disclosure_indicator() {
    let theme = Theme::default();
    let mut turn = reasoning_turn("hovered reasoning");
    turn.header.is_hovered = true;

    let surfaces = build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.shell);
    let reasoning = reasoning_surface(&surfaces);

    assert!(line_text(&reasoning.lines[0]).starts_with("   › Thinking…"));
}

#[test]
fn reduced_motion_keeps_the_active_reasoning_rail_visible_and_static() {
    let theme = Theme::default();
    let mut turn = reasoning_turn("reduced motion reasoning");
    turn.motion_enabled = false;

    let surfaces = build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.shell);
    let reasoning = reasoning_surface(&surfaces);

    assert!(reasoning.show_outer_rail);
    assert!(reasoning.tool_rail_motion.is_none());
    assert_eq!(
        reasoning.rail_glyph,
        theme.live_shell.transcript_glyphs.rail
    );
}

#[test]
fn reasoning_selection_excludes_header_chrome_and_preserves_body_rows() {
    let theme = Theme::default();
    let mut turn = reasoning_turn("selectable body");
    turn.reasoning_expanded = true;

    let surfaces = build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.shell);
    let reasoning = reasoning_surface(&surfaces);
    let rows = reasoning
        .selection_rows
        .as_ref()
        .expect("reasoning selection rows");

    assert!(rows[0].cells.iter().all(|cell| cell.trim().is_empty()));
    assert!(rows[1].cells.iter().all(|cell| cell.trim().is_empty()));
    assert!(rows[2].cells.concat().contains("selectable body"));
}

#[test]
fn expanded_literal_ellipsis_remains_selectable_body_content() {
    let theme = Theme::default();
    let mut turn = reasoning_turn("…");
    turn.reasoning_expanded = true;

    let surfaces = build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.shell);
    let reasoning = reasoning_surface(&surfaces);
    let rows = reasoning
        .selection_rows
        .as_ref()
        .expect("reasoning selection rows");

    assert!(rows.last().expect("body row").cells.concat().contains('…'));
}

#[test]
fn ascii_reasoning_uses_label_only_header_and_semantic_rail_glyph() {
    let theme = Theme::default().with_glyph_mode(crate::theme::GlyphMode::Ascii);
    let turn = reasoning_turn("ascii reasoning");

    let surfaces = build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.shell);
    let reasoning = reasoning_surface(&surfaces);

    assert!(line_text(&reasoning.lines[0]).starts_with("   Thinking…"));
    assert_eq!(
        reasoning.rail_glyph,
        theme.live_shell.transcript_glyphs.rail
    );
}
