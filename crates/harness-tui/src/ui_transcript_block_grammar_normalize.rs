use super::build::{base_spec, normalized_part_spec_without_spacing, normalized_user_spec};
use super::content::{footer_content, footer_lifecycle, lifecycle_state};
use super::tool::{apply_tool_policy, group_policy};
use super::*;

pub(in crate::ui) fn normalize_turn_blocks(
    turn: &TranscriptTurnSection,
) -> Vec<TranscriptBlockSpec> {
    let mut specs = Vec::with_capacity(turn.assistant_parts.len().saturating_add(2));
    if let Some(user) = normalized_user_spec(turn) {
        specs.push(user);
    }
    let mut index = 0;
    let mut grouped_until = 0;
    while index < turn.assistant_parts.len() {
        if let Some(group) = (index >= grouped_until)
            .then(|| TranscriptToolGroupSummary::from_adjacent(&turn.assistant_parts[index..]))
            .flatten()
            .filter(TranscriptToolGroupSummary::folds_as_group)
        {
            let ids = turn.assistant_parts[index..index + group.span_len]
                .iter()
                .filter_map(|part| match part {
                    TranscriptAssistantPart::ToolCall(tool) => Some(tool.as_ref()),
                    TranscriptAssistantPart::Reasoning(_)
                    | TranscriptAssistantPart::Body(_)
                    | TranscriptAssistantPart::Error(_)
                    | TranscriptAssistantPart::Compaction(_) => None,
                })
                .map(|tool| tool.tool_call_id.clone())
                .collect();
            let mut spec = base_spec(
                turn,
                Some(index),
                TranscriptBlockRole::Tool,
                TranscriptBlockContent::Tool {
                    family: TranscriptToolFamily::Group,
                    ids,
                    policy: group_policy(&group),
                    subagent: None,
                },
            );
            spec.grouping = TranscriptBlockGrouping {
                group_id: Some(spec.id.clone()),
                member_count: group.member_count,
            };
            apply_tool_policy(&mut spec);
            specs.push(spec);
            grouped_until = index + group.span_len;
        }
        specs.push(normalized_part_spec_without_spacing(turn, index));
        index += 1;
    }
    if turn.show_footer {
        let lifecycle = footer_lifecycle(turn);
        let mut footer = base_spec(
            turn,
            None,
            TranscriptBlockRole::Footer,
            TranscriptBlockContent::Footer {
                lifecycle,
                state: lifecycle_state(turn),
                content: footer_content(turn, lifecycle),
            },
        );
        footer.placement = footer_placement(lifecycle);
        footer.compact = TranscriptBlockCompactPolicy::ElideDetails;
        specs.push(footer);
    }
    specs
}

pub(in crate::ui) fn normalized_part_spec(
    turn: &TranscriptTurnSection,
    part_index: usize,
) -> TranscriptBlockSpec {
    let mut spec = normalized_part_spec_without_spacing(turn, part_index);
    let previous_spec = part_index
        .checked_sub(1)
        .map(|index| normalized_part_spec_without_spacing(turn, index))
        .or_else(|| normalized_user_spec(turn));
    spec.spacing.leading_gap_rows = grammar_leading_gap(previous_spec.as_ref(), &spec);
    spec
}

#[cfg(test)]
pub(in crate::ui) fn test_spec(
    role: TranscriptBlockRole,
    content: TranscriptBlockContent,
) -> TranscriptBlockSpec {
    let tool = role == TranscriptBlockRole::Tool;
    TranscriptBlockSpec {
        id: TranscriptBlockId("test:0".into()),
        role,
        content,
        chrome: TranscriptBlockChrome {
            accent: tool,
            rail: false,
        },
        spacing: TranscriptBlockSpacing {
            leading_gap_rows: 0,
            trailing_gap_rows: 0,
        },
        grouping: TranscriptBlockGrouping {
            group_id: None,
            member_count: 1,
        },
        fold: TranscriptBlockFold {
            foldable: false,
            expanded: false,
        },
        interaction: TranscriptBlockInteraction {
            selectable: role == TranscriptBlockRole::AssistantBody,
            selected: false,
            hoverable: tool,
            focusable: tool,
        },
        disclosure: TranscriptBlockDisclosure {
            available: false,
            expanded: false,
        },
        compact: TranscriptBlockCompactPolicy::Preserve,
        placement: TranscriptBlockPlacement::Flow,
        motion: TranscriptBlockMotionDemand::None,
    }
}
