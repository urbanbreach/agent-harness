use super::compaction::apply_compaction_policy;
use super::content::{
    apply_reasoning_policy, content_for_part, footer_content, footer_lifecycle, lifecycle_state,
};
use super::tool::{apply_tool_policy, group_policy};
use super::*;

pub(in crate::ui) fn normalize_turn_blocks(
    turn: &TranscriptTurnSection,
) -> Vec<TranscriptBlockSpec> {
    let mut specs = Vec::with_capacity(turn.assistant_parts.len().saturating_add(2));
    if let Some(user) = &turn.user_message {
        specs.push(base_spec(
            turn,
            0,
            TranscriptBlockRole::UserPrompt,
            TranscriptBlockContent::UserMessage {
                text: user.text.clone(),
                queued: user.queued,
                wall_clock: user.wall_clock.clone(),
                state: if turn.header.status == ActivityStatus::Streaming
                    && turn
                        .assistant_parts
                        .iter()
                        .any(|part| matches!(part, TranscriptAssistantPart::Reasoning(_)))
                    && !turn
                        .assistant_parts
                        .iter()
                        .any(|part| matches!(part, TranscriptAssistantPart::ToolCall(_)))
                {
                    TranscriptPromptState::ActiveThinking
                } else if turn.header.is_selected {
                    TranscriptPromptState::Selected
                } else {
                    TranscriptPromptState::Idle
                },
            },
        ));
    }
    let mut index = 0;
    let mut grouped_until = 0;
    while index < turn.assistant_parts.len() {
        if let Some(group) = (index >= grouped_until)
            .then(|| TranscriptToolGroupSummary::from_adjacent(&turn.assistant_parts[index..]))
            .flatten()
            .filter(|group| group.member_count > 1)
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
                index.saturating_add(1),
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
        let (role, content) = content_for_part(turn, index, &turn.assistant_parts[index]);
        let mut spec = base_spec(turn, index.saturating_add(1), role, content);
        apply_reasoning_policy(&mut spec);
        apply_tool_policy(&mut spec);
        apply_compaction_policy(&mut spec);
        specs.push(spec);
        index += 1;
    }
    if turn.show_footer {
        let lifecycle = footer_lifecycle(turn);
        let mut footer = base_spec(
            turn,
            turn.assistant_parts.len().saturating_add(1),
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
    let (role, content) = content_for_part(turn, part_index, &turn.assistant_parts[part_index]);
    let mut spec = base_spec(turn, part_index.saturating_add(1), role, content);
    let previous_role = part_index
        .checked_sub(1)
        .map(|index| content_for_part(turn, index, &turn.assistant_parts[index]).0)
        .or_else(|| {
            turn.user_message
                .as_ref()
                .map(|_| TranscriptBlockRole::UserPrompt)
        });
    spec.spacing.leading_gap_rows = grammar_leading_gap(previous_role, role);
    apply_reasoning_policy(&mut spec);
    apply_tool_policy(&mut spec);
    apply_compaction_policy(&mut spec);
    spec
}

fn base_spec(
    turn: &TranscriptTurnSection,
    index: usize,
    role: TranscriptBlockRole,
    content: TranscriptBlockContent,
) -> TranscriptBlockSpec {
    let tool = role == TranscriptBlockRole::Tool;
    TranscriptBlockSpec {
        id: TranscriptBlockId(format!("{}:{index}", turn.request_id)),
        role,
        content,
        chrome: TranscriptBlockChrome {
            accent: tool || role == TranscriptBlockRole::Error,
            rail: false,
        },
        spacing: TranscriptBlockSpacing {
            leading_gap_rows: 0,
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
        compact: if tool {
            TranscriptBlockCompactPolicy::ElideDetails
        } else {
            TranscriptBlockCompactPolicy::Preserve
        },
        placement: if role == TranscriptBlockRole::UserPrompt {
            TranscriptBlockPlacement::StickyPromptCandidate
        } else {
            TranscriptBlockPlacement::Flow
        },
        motion: TranscriptBlockMotionDemand::None,
    }
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
