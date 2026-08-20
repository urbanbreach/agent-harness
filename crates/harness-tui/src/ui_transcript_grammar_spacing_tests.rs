use super::super::ui_transcript_block_grammar::{
    resolve_entry_surfaces, test_spec, TranscriptBlockContent, TranscriptBlockDisclosure,
    TranscriptBlockFold, TranscriptBlockMotionDemand, TranscriptBlockPlacement,
    TranscriptBlockRole, TranscriptFooterContent, TranscriptFooterLifecycle,
    TranscriptFooterMetadata, TranscriptLifecycleState, TranscriptToolDisclosure,
    TranscriptToolFamily, TranscriptToolPolicy, TranscriptToolStatus,
};
use super::*;
use ratatui::style::Color;
use ratatui::text::Line;

fn tool_spec(disclosure: TranscriptToolDisclosure) -> TranscriptBlockSpec {
    let mut spec = test_spec(
        TranscriptBlockRole::Tool,
        TranscriptBlockContent::Tool {
            family: TranscriptToolFamily::Read,
            ids: vec!["tool-1".into()],
            policy: TranscriptToolPolicy {
                group_class: None,
                member_count: 1,
                visible_start: 0,
                disclosure,
                status: TranscriptToolStatus::Succeeded,
                motion: TranscriptBlockMotionDemand::None,
                trailing_gap_cells: 0,
            },
            subagent: None,
        },
    );
    spec.fold = TranscriptBlockFold {
        foldable: disclosure != TranscriptToolDisclosure::None,
        expanded: disclosure == TranscriptToolDisclosure::Expanded,
    };
    spec.disclosure = TranscriptBlockDisclosure {
        available: disclosure != TranscriptToolDisclosure::None,
        expanded: disclosure == TranscriptToolDisclosure::Expanded,
    };
    spec
}

fn reasoning_spec(expanded: bool) -> TranscriptBlockSpec {
    let mut spec = test_spec(
        TranscriptBlockRole::Reasoning,
        TranscriptBlockContent::Reasoning {
            text: "reasoning".into(),
            active: false,
            expanded,
            duration_ms: Some(10),
            motion_enabled: false,
        },
    );
    spec.fold = TranscriptBlockFold {
        foldable: true,
        expanded,
    };
    spec.disclosure = TranscriptBlockDisclosure {
        available: true,
        expanded,
    };
    spec
}

fn assistant_spec() -> TranscriptBlockSpec {
    test_spec(
        TranscriptBlockRole::AssistantBody,
        TranscriptBlockContent::AssistantBody {
            text: "answer".into(),
            streaming: false,
            wall_clock: None,
            has_tools: true,
        },
    )
}

fn pinned_footer_spec() -> TranscriptBlockSpec {
    let mut spec = test_spec(
        TranscriptBlockRole::Footer,
        TranscriptBlockContent::Footer {
            lifecycle: TranscriptFooterLifecycle::Permission,
            state: TranscriptLifecycleState::Responding,
            content: TranscriptFooterContent::Permission {
                tool_id: "read".into(),
                label: "Read file".into(),
                metadata: TranscriptFooterMetadata {
                    duration_ms: None,
                    total_tokens: None,
                },
            },
        },
    );
    spec.placement = TranscriptBlockPlacement::PinnedFooter { outdent_cells: 1 };
    spec
}

fn question_footer_spec() -> TranscriptBlockSpec {
    let mut spec = test_spec(
        TranscriptBlockRole::Footer,
        TranscriptBlockContent::Footer {
            lifecycle: TranscriptFooterLifecycle::Question,
            state: TranscriptLifecycleState::Responding,
            content: TranscriptFooterContent::Question {
                label: "Choose one".into(),
                metadata: TranscriptFooterMetadata {
                    duration_ms: None,
                    total_tokens: None,
                },
            },
        },
    );
    spec.placement = TranscriptBlockPlacement::PinnedFooter { outdent_cells: 1 };
    spec
}

fn surface(kind: TranscriptRenderSurfaceKind) -> TranscriptVisualEntryDraft {
    TranscriptVisualEntryDraft {
        kind,
        leading_gap_rows: 0,
        trailing_gap_rows: 0,
        placement: TranscriptBlockPlacement::Flow,
        show_outer_rail: false,
        rail_glyph: " ",
        rail_color: Color::Reset,
        surface: Color::Reset,
        lines: vec![Line::from("row")],
        interaction_rows: None,
        selection_rows: None,
        diff_hunk_offsets: Vec::new(),
        selected_rail: false,
        tool_rail_motion: None,
    }
}

fn resolved_gaps(specs: &[TranscriptBlockSpec]) -> Vec<usize> {
    let surfaces = specs
        .iter()
        .map(|spec| {
            let kind = match spec.role {
                TranscriptBlockRole::Tool => TranscriptRenderSurfaceKind::AssistantTool,
                TranscriptBlockRole::Reasoning => TranscriptRenderSurfaceKind::AssistantReasoning,
                TranscriptBlockRole::AssistantBody => TranscriptRenderSurfaceKind::AssistantBody,
                TranscriptBlockRole::Footer => TranscriptRenderSurfaceKind::AssistantFooter,
                TranscriptBlockRole::UserPrompt
                | TranscriptBlockRole::Error
                | TranscriptBlockRole::Compaction
                | TranscriptBlockRole::Synthetic => TranscriptRenderSurfaceKind::AssistantBody,
            };
            surface(kind)
        })
        .collect();
    resolve_entry_surfaces(1, specs, surfaces)
        .expect("spacing fixture resolves")
        .into_iter()
        .map(|entry| entry.leading_gap_rows)
        .collect()
}

fn edit_turn(first_expanded: bool) -> TranscriptTurnSection {
    let mut turn = canonical_turn();
    let mut first = grammar_tool("edit-1", "edit", ToolCallPresentationStatus::Succeeded);
    if let TranscriptAssistantPart::ToolCall(tool) = &mut first {
        tool.detail_blocks
            .push(TranscriptToolCallDetailBlock::Message {
                text: "line one\nline two".into(),
                tone: TranscriptToolCallDetailTone::Primary,
            });
        tool.expanded = first_expanded;
        tool.header.disclosure_state = Some(if first_expanded {
            TranscriptToolCallDisclosureState::Expanded
        } else {
            TranscriptToolCallDisclosureState::Collapsed
        });
    }
    turn.assistant_parts = vec![
        first,
        grammar_tool("edit-2", "edit", ToolCallPresentationStatus::Succeeded),
    ];
    turn.assistant_part_source_ids.clear();
    turn.show_footer = false;
    turn
}

#[test]
fn collapsed_groupable_neighbors_pack_without_a_separator() {
    // arrange
    let specs = [
        tool_spec(TranscriptToolDisclosure::Collapsed),
        reasoning_spec(false),
        tool_spec(TranscriptToolDisclosure::None),
    ];

    // act
    let gaps = resolved_gaps(&specs);

    // assert
    assert_eq!(gaps, vec![0, 0, 0]);
}

#[test]
fn preview_or_expanded_neighbor_earns_a_separator_on_each_side() {
    // arrange
    let specs = [
        tool_spec(TranscriptToolDisclosure::Collapsed),
        tool_spec(TranscriptToolDisclosure::Preview),
        tool_spec(TranscriptToolDisclosure::Collapsed),
        reasoning_spec(true),
        tool_spec(TranscriptToolDisclosure::Collapsed),
    ];

    // act
    let gaps = resolved_gaps(&specs);

    // assert
    assert_eq!(gaps, vec![0, 1, 1, 1, 1]);
}

#[test]
fn non_groupable_assistant_body_breaks_a_dense_activity_run() {
    // arrange
    let specs = [
        tool_spec(TranscriptToolDisclosure::Collapsed),
        assistant_spec(),
        reasoning_spec(false),
        tool_spec(TranscriptToolDisclosure::Collapsed),
    ];

    // act
    let gaps = resolved_gaps(&specs);

    // assert
    assert_eq!(gaps, vec![0, 1, 1, 0]);
}

#[test]
fn pinned_footer_keeps_a_separator_after_collapsed_activity() {
    // arrange
    let specs = [
        tool_spec(TranscriptToolDisclosure::Collapsed),
        pinned_footer_spec(),
    ];

    // act
    let gaps = resolved_gaps(&specs);

    // assert
    assert_eq!(gaps, vec![0, 1]);
}

#[test]
fn final_visible_block_keeps_one_trailing_gap_row() {
    let specs = [tool_spec(TranscriptToolDisclosure::Collapsed)];
    let entries = resolve_entry_surfaces(
        1,
        &specs,
        vec![surface(TranscriptRenderSurfaceKind::AssistantTool)],
    )
    .expect("final entry resolves");

    assert_eq!(entries[0].trailing_gap_rows, 1);
}

#[test]
fn permission_and_question_footers_stay_separated_and_keep_the_final_gap() {
    for footer in [pinned_footer_spec(), question_footer_spec()] {
        let specs = [tool_spec(TranscriptToolDisclosure::Collapsed), footer];
        let entries = resolve_entry_surfaces(
            1,
            &specs,
            vec![
                surface(TranscriptRenderSurfaceKind::AssistantTool),
                surface(TranscriptRenderSurfaceKind::AssistantFooter),
            ],
        )
        .expect("pinned footer resolves");

        assert_eq!(entries[1].leading_gap_rows, 1);
        assert_eq!(entries[1].trailing_gap_rows, 1);
    }
}

#[test]
fn production_render_changes_only_the_separator_after_expanded_tool_content() {
    // arrange
    let theme = Theme::default();
    let collapsed = edit_turn(false);
    let expanded = edit_turn(true);

    // act
    let collapsed_surfaces =
        build_transcript_render_surfaces(&collapsed, &theme, 80, theme.surface.shell);
    let expanded_surfaces =
        build_transcript_render_surfaces(&expanded, &theme, 80, theme.surface.shell);
    let collapsed_gaps = collapsed_surfaces
        .iter()
        .map(|surface| surface.leading_gap_rows)
        .collect::<Vec<_>>();
    let expanded_gaps = expanded_surfaces
        .iter()
        .map(|surface| surface.leading_gap_rows)
        .collect::<Vec<_>>();

    // assert
    assert_eq!(collapsed_gaps, vec![0, 1, 0]);
    assert_eq!(expanded_gaps, vec![0, 1, 1]);
    assert!(expanded_surfaces[1].lines.len() > collapsed_surfaces[1].lines.len());
}
