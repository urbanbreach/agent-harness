use super::ui_transcript_block_grammar::{
    resolve_block_surface, test_spec, validate_block_spec, TranscriptBlockContent,
    TranscriptBlockMotionDemand, TranscriptBlockRole, TranscriptFooterContent,
    TranscriptGrammarError, TranscriptLifecycleState, TranscriptSubagentLifecycle,
    TranscriptSubagentMode, TranscriptToolDisclosure, TranscriptToolFamily,
    TranscriptToolGroupClass, TranscriptToolStatus,
};
use super::*;
use crate::app::ToolCallPresentation;
use harness_core::event::ProviderRequestRetryMetadata;

use crate::ui::ui_transcript_test_helpers::{
    assert_grammar_contract_error, grammar_contract_has_background_transition,
    grammar_contract_has_motion, transcript_grammar_test_app, validate_grammar_contract,
    GrammarFamily, GrammarMotion, GrammarPlacement, GrammarSurfaceContract, GRAMMAR_ALL_FAMILIES,
};

fn canonical_turn() -> TranscriptTurnSection {
    let app = transcript_grammar_test_app();
    let mut turn = build_transcript_sections(&app)
        .into_iter()
        .next()
        .expect("canonical turn exists");
    let body_index = turn
        .assistant_parts
        .iter()
        .position(|part| matches!(part, TranscriptAssistantPart::Body(_)))
        .expect("settled body exists");
    turn.assistant_parts.insert(
        body_index + 1,
        TranscriptAssistantPart::Body(TranscriptBodyBlock::StreamingRichText(
            "streaming body".to_string(),
        )),
    );
    turn.assistant_parts
        .push(TranscriptAssistantPart::Compaction(
            TranscriptCompactionSection {
                kind: TranscriptCompactionKind::BranchSummary,
                summary: "要約 🧭".to_string(),
                tokens_before: Some(1_024),
                read_files: vec!["src/lib.rs".to_string()],
                modified_files: vec!["src/ui.rs".to_string()],
            },
        ));
    turn
}

fn canonical_contract(width: u16) -> Vec<GrammarSurfaceContract> {
    let theme = Theme::default();
    let turn = canonical_turn();
    let layout = measure_transcript_layout(
        std::slice::from_ref(&turn),
        &theme,
        width,
        theme.surface.shell,
        |section| section.activity_first_seq,
        |_, _| None,
        build_transcript_render_surfaces,
    );
    let surfaces = &layout.sections[0].surfaces;
    surfaces
        .iter()
        .enumerate()
        .map(|(index, surface)| GrammarSurfaceContract {
            family: match surface.kind {
                TranscriptRenderSurfaceKind::User => GrammarFamily::User,
                TranscriptRenderSurfaceKind::AssistantReasoning => GrammarFamily::Reasoning,
                TranscriptRenderSurfaceKind::AssistantBody => GrammarFamily::Body,
                TranscriptRenderSurfaceKind::AssistantTool => GrammarFamily::Tool,
                TranscriptRenderSurfaceKind::AssistantCommandTool => GrammarFamily::Command,
                TranscriptRenderSurfaceKind::AssistantError => GrammarFamily::Error,
                TranscriptRenderSurfaceKind::AssistantFooter => GrammarFamily::Footer,
                TranscriptRenderSurfaceKind::Compaction => GrammarFamily::Compaction,
            },
            width: surface.width,
            leading_gap: surface.top_offset.saturating_sub(
                index
                    .checked_sub(1)
                    .and_then(|previous| surfaces.get(previous))
                    .map_or(0, |previous| previous.top_offset + previous.height),
            ),
            rail: surface.show_outer_rail,
            background: surface.surface,
            interaction_rows: surface.interaction_rows.as_ref().map(Vec::len),
            selection_rows: surface.selection_rows.as_ref().map(Vec::len),
            line_rows: surface.lines.len(),
            placement: match (index, surface.kind) {
                (0, TranscriptRenderSurfaceKind::User) => GrammarPlacement::StickyPrompt,
                (_, TranscriptRenderSurfaceKind::AssistantFooter)
                    if index + 1 == surfaces.len() =>
                {
                    GrammarPlacement::PinnedFooter
                }
                _ => GrammarPlacement::Flow,
            },
            motion: surface.tool_rail_motion.map(|motion| match motion {
                ToolRailMotion::Running { .. } => GrammarMotion::Running,
                ToolRailMotion::Waiting | ToolRailMotion::Queued => GrammarMotion::Waiting,
                ToolRailMotion::FinishFlash { .. } => GrammarMotion::FinishFlash,
                ToolRailMotion::Settled => GrammarMotion::Settled,
            }),
        })
        .collect()
}

fn families(contract: &[GrammarSurfaceContract]) -> Vec<GrammarFamily> {
    contract.iter().map(|surface| surface.family).collect()
}

fn grammar_tool(
    id: &str,
    tool_id: &str,
    status: ToolCallPresentationStatus,
) -> TranscriptAssistantPart {
    TranscriptAssistantPart::ToolCall(Box::new(TranscriptToolCallSection {
        tool_call_id: id.into(),
        coalesced_tool_call_ids: vec![id.into()],
        child_session_id: None,
        subagent_background: false,
        output_truncated: false,
        replay_read_only: false,
        hovered_target: None,
        header: TranscriptToolCallHeader {
            tool_id: tool_id.into(),
            title: format!("{tool_id} 界面"),
            subtitle: None,
            path_metadata: None,
            icon: None,
            presentation: ToolCallPresentation {
                status,
                duration_ms: None,
                result_count: None,
            },
            visual_style: TranscriptToolCallVisualStyle::Inline,
            struck_out: false,
            disclosure_state: Some(TranscriptToolCallDisclosureState::Collapsed),
        },
        detail_blocks: Vec::new(),
        details_collapsed_by_default: true,
        details_preview_visible: false,
        animation_phase: 0,
        expanded: false,
        rail_motion: ToolRailMotion::Settled,
    }))
}

fn normalized_tool_spec(
    turn: &TranscriptTurnSection,
    family: TranscriptToolFamily,
) -> TranscriptBlockSpec {
    normalize_turn_blocks(turn)
        .into_iter()
        .find(|spec| matches!(&spec.content, TranscriptBlockContent::Tool { family: actual, .. } if *actual == family))
        .expect("normalized tool spec")
}

#[path = "ui_transcript_grammar_characterization_tests.rs"]
mod characterization;
#[path = "ui_transcript_grammar_compaction_tests.rs"]
mod compaction;
#[path = "ui_transcript_grammar_extension_tests.rs"]
mod extension;
#[path = "ui_transcript_grammar_footer_lifecycle_tests.rs"]
mod footer_lifecycle;
#[path = "ui_transcript_grammar_generic_tool_tests.rs"]
mod generic_tool;
#[path = "ui_transcript_grammar_prompt_reasoning_tests.rs"]
mod prompt_reasoning;
#[path = "ui_transcript_reasoning_parity_tests.rs"]
mod reasoning_parity;
#[path = "ui_transcript_reasoning_selection_tests.rs"]
mod reasoning_selection;
#[path = "ui_transcript_grammar_shell_diff_tests.rs"]
mod shell_diff;
#[path = "ui_transcript_grammar_spacing_tests.rs"]
mod spacing;
#[path = "ui_transcript_grammar_subagent_tests.rs"]
mod subagent;
#[path = "ui_transcript_grammar_tool_family_tests.rs"]
mod tool_family;
#[path = "ui_transcript_grammar_visibility_tests.rs"]
mod visibility;
