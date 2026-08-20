use super::compaction::apply_compaction_policy;
use super::content::{apply_reasoning_policy, content_for_part};
use super::tool::apply_tool_policy;
use super::*;

pub(super) fn normalized_part_spec_without_spacing(
    turn: &TranscriptTurnSection,
    part_index: usize,
) -> TranscriptBlockSpec {
    let (role, content) = content_for_part(turn, part_index, &turn.assistant_parts[part_index]);
    let mut spec = base_spec(turn, Some(part_index), role, content);
    apply_reasoning_policy(&mut spec);
    apply_tool_policy(&mut spec);
    apply_compaction_policy(&mut spec);
    spec
}

pub(super) fn normalized_user_spec(turn: &TranscriptTurnSection) -> Option<TranscriptBlockSpec> {
    let user = turn.user_message.as_ref()?;
    let active_thinking = turn.header.status == ActivityStatus::Streaming
        && turn
            .assistant_parts
            .iter()
            .any(|part| matches!(part, TranscriptAssistantPart::Reasoning(_)))
        && !turn
            .assistant_parts
            .iter()
            .any(|part| matches!(part, TranscriptAssistantPart::ToolCall(_)));
    Some(base_spec(
        turn,
        None,
        TranscriptBlockRole::UserPrompt,
        TranscriptBlockContent::UserMessage {
            text: user.text.clone(),
            queued: user.queued,
            wall_clock: user.wall_clock.clone(),
            state: if active_thinking {
                TranscriptPromptState::ActiveThinking
            } else {
                TranscriptPromptState::Idle
            },
        },
    ))
}

pub(super) fn base_spec(
    turn: &TranscriptTurnSection,
    part_index: Option<usize>,
    role: TranscriptBlockRole,
    content: TranscriptBlockContent,
) -> TranscriptBlockSpec {
    let tool = role == TranscriptBlockRole::Tool;
    TranscriptBlockSpec {
        id: source_derived_block_id(turn, part_index, role, &content),
        role,
        content,
        chrome: TranscriptBlockChrome {
            accent: tool || role == TranscriptBlockRole::Error,
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

fn source_derived_block_id(
    turn: &TranscriptTurnSection,
    part_index: Option<usize>,
    role: TranscriptBlockRole,
    content: &TranscriptBlockContent,
) -> TranscriptBlockId {
    let role_label = match role {
        TranscriptBlockRole::UserPrompt => "user",
        TranscriptBlockRole::AssistantBody => "body",
        TranscriptBlockRole::Reasoning => "reasoning",
        TranscriptBlockRole::Tool => "tool",
        TranscriptBlockRole::Footer => "footer",
        TranscriptBlockRole::Error => "error",
        TranscriptBlockRole::Compaction => "compaction",
        #[cfg(test)]
        TranscriptBlockRole::Synthetic => "synthetic",
    };
    if matches!(
        role,
        TranscriptBlockRole::UserPrompt | TranscriptBlockRole::Footer
    ) {
        return TranscriptBlockId(format!(
            "{}:{role_label}:{}",
            turn.request_id, turn.activity_first_seq
        ));
    }
    if let TranscriptBlockContent::Tool { ids, .. } = content {
        return TranscriptBlockId(format!(
            "{}:{role_label}:{:016x}",
            turn.request_id,
            super::super::ui_transcript_entry::semantic_key(ids.iter().map(String::as_str))
        ));
    }
    let source_seq = (turn.assistant_part_source_ids.len() == turn.assistant_parts.len())
        .then(|| part_index.and_then(|index| turn.assistant_part_source_ids.get(index)))
        .flatten()
        .map(|source| source.0);
    if let Some(source_seq) = source_seq {
        return TranscriptBlockId(format!(
            "{}:{role_label}:event:{source_seq}",
            turn.request_id
        ));
    }
    TranscriptBlockId(format!(
        "{}:{role_label}:fixture:{:016x}",
        turn.request_id,
        fallback_content_key(content)
    ))
}

fn fallback_content_key(content: &TranscriptBlockContent) -> u64 {
    let value = match content {
        TranscriptBlockContent::AssistantBody { text, .. }
        | TranscriptBlockContent::Reasoning { text, .. } => text.as_str(),
        TranscriptBlockContent::Error { message } => message.as_str(),
        TranscriptBlockContent::Compaction { summary, .. } => summary.as_str(),
        #[cfg(test)]
        TranscriptBlockContent::Synthetic { value } => value.as_str(),
        TranscriptBlockContent::UserMessage { .. }
        | TranscriptBlockContent::Tool { .. }
        | TranscriptBlockContent::Footer { .. } => "",
    };
    super::super::ui_transcript_entry::semantic_key([value])
}
