use super::*;

#[path = "ui_transcript_block_grammar_types.rs"]
mod types;
pub(in crate::ui) use types::*;

#[path = "ui_transcript_block_grammar_lifecycle_types.rs"]
mod lifecycle_types;
pub(in crate::ui) use lifecycle_types::*;

#[path = "ui_transcript_block_grammar_spec.rs"]
mod spec;
pub(in crate::ui) use spec::*;

#[path = "ui_transcript_block_grammar_content.rs"]
mod content;

#[path = "ui_transcript_block_grammar_tool.rs"]
mod tool;
pub(in crate::ui::ui_transcript) use tool::tool_family;

#[path = "ui_transcript_block_grammar_compaction.rs"]
mod compaction;

#[path = "ui_transcript_block_grammar_normalize.rs"]
mod normalize;
#[cfg(test)]
pub(in crate::ui) use normalize::test_spec;
pub(super) use normalize::{normalize_turn_blocks, normalized_part_spec};

#[path = "ui_transcript_block_grammar_resolve.rs"]
mod resolve;
pub(in crate::ui) use resolve::resolve_block_surface;
#[cfg(test)]
pub(super) use resolve::resolve_compatibility_surfaces;
pub(super) use resolve::resolve_entry_surfaces;

pub(in crate::ui) fn validate_block_spec(
    spec: &TranscriptBlockSpec,
) -> Result<(), TranscriptGrammarError> {
    if spec.interaction.selected && !spec.interaction.selectable {
        return Err(TranscriptGrammarError::InvalidInteraction);
    }
    if spec.motion != TranscriptBlockMotionDemand::None && !spec.chrome.accent {
        return Err(TranscriptGrammarError::InvalidMotion);
    }
    if matches!(
        spec.placement,
        TranscriptBlockPlacement::PinnedFooter { .. }
    ) && spec.role != TranscriptBlockRole::Footer
    {
        return Err(TranscriptGrammarError::InvalidPlacement);
    }
    if (spec.fold.expanded && !spec.fold.foldable)
        || (spec.disclosure.expanded && !spec.disclosure.available)
    {
        return Err(TranscriptGrammarError::InvalidDisclosure);
    }
    if spec.grouping.member_count > 1 && spec.grouping.group_id.is_none() {
        return Err(TranscriptGrammarError::InvalidGrouping);
    }
    if let TranscriptBlockContent::Tool {
        family,
        ids,
        policy,
        subagent,
    } = &spec.content
    {
        let group = *family == TranscriptToolFamily::Group;
        if ids.is_empty()
            || group != policy.group_class.is_some()
            || group != (policy.member_count > 1)
            || policy.visible_start > policy.member_count
            || (*family == TranscriptToolFamily::Task) != subagent.is_some()
        {
            return Err(TranscriptGrammarError::InvalidGrouping);
        }
    }
    Ok(())
}

fn grammar_leading_gap(
    previous: Option<TranscriptBlockRole>,
    current: TranscriptBlockRole,
) -> usize {
    match (previous, current) {
        (None, _) => 0,
        (
            Some(TranscriptBlockRole::Tool),
            TranscriptBlockRole::Tool | TranscriptBlockRole::Reasoning,
        )
        | (Some(TranscriptBlockRole::Reasoning), TranscriptBlockRole::Tool)
        | (
            Some(TranscriptBlockRole::AssistantBody),
            TranscriptBlockRole::Reasoning | TranscriptBlockRole::AssistantBody,
        ) => 0,
        (Some(_), _) => 1,
    }
}

const fn footer_placement(lifecycle: TranscriptFooterLifecycle) -> TranscriptBlockPlacement {
    match lifecycle {
        TranscriptFooterLifecycle::Permission | TranscriptFooterLifecycle::Question => {
            TranscriptBlockPlacement::PinnedFooter { outdent_cells: 1 }
        }
        TranscriptFooterLifecycle::Retry | TranscriptFooterLifecycle::Responding => {
            TranscriptBlockPlacement::PinnedFooter { outdent_cells: 0 }
        }
        TranscriptFooterLifecycle::Settled => TranscriptBlockPlacement::Flow,
    }
}

#[cfg(test)]
fn role_for_surface(kind: TranscriptRenderSurfaceKind) -> TranscriptBlockRole {
    match kind {
        TranscriptRenderSurfaceKind::User => TranscriptBlockRole::UserPrompt,
        TranscriptRenderSurfaceKind::AssistantReasoning => TranscriptBlockRole::Reasoning,
        TranscriptRenderSurfaceKind::AssistantBody => TranscriptBlockRole::AssistantBody,
        TranscriptRenderSurfaceKind::AssistantTool
        | TranscriptRenderSurfaceKind::AssistantCommandTool => TranscriptBlockRole::Tool,
        TranscriptRenderSurfaceKind::AssistantError => TranscriptBlockRole::Error,
        TranscriptRenderSurfaceKind::AssistantFooter => TranscriptBlockRole::Footer,
        TranscriptRenderSurfaceKind::Compaction => TranscriptBlockRole::Compaction,
    }
}
