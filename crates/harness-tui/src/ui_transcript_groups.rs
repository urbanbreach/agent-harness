//! Shared fold scan for painting, selection, navigation, and disclosure.
use super::*;

const DENSE_VISIBLE_ENTRIES: usize = 10;

#[derive(Clone, Debug)]
pub(super) struct TranscriptToolGroup {
    pub(super) start: usize,
    pub(super) members: Vec<usize>,
    pub(super) target_ids: Vec<String>,
    pub(super) hidden: Vec<usize>,
    pub(super) summary: TranscriptToolGroupSummary,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RunStep {
    Member,
    Thought,
    Transparent,
    Break,
}

fn context_step(turn: &TranscriptTurnSection, index: usize) -> RunStep {
    match &turn.assistant_parts[index] {
        TranscriptAssistantPart::Reasoning(_) => {
            if turn.reasoning_expanded || turn.reasoning_active(index) {
                RunStep::Transparent
            } else {
                RunStep::Thought
            }
        }
        TranscriptAssistantPart::ToolCall(tool)
            if tool.header.presentation.status != ToolCallPresentationStatus::Waiting
                && TranscriptToolVerb::from_tool_call(tool)
                    .is_some_and(|verb| verb.group_kind() == TranscriptToolGroupKind::Context) =>
        {
            if tool.expanded || tool.details_preview_visible {
                RunStep::Transparent
            } else {
                RunStep::Member
            }
        }
        _ => RunStep::Break,
    }
}

fn tool_at(turn: &TranscriptTurnSection, index: usize) -> Option<&TranscriptToolCallSection> {
    turn.assistant_parts[index].tool_call()
}

pub(super) fn scan(turn: &TranscriptTurnSection) -> Vec<TranscriptToolGroup> {
    let len = turn.assistant_parts.len();
    let mut claimed = vec![false; len];
    let mut groups = Vec::new();
    let mut index = 0;
    while index < len {
        if !matches!(
            context_step(turn, index),
            RunStep::Member | RunStep::Thought
        ) {
            index += 1;
            continue;
        }
        let start = index;
        let mut end = start;
        let mut participants = Vec::new();
        let mut members = Vec::new();
        while index < len {
            match context_step(turn, index) {
                RunStep::Member => {
                    members.push(index);
                    participants.push(index);
                    end = index + 1;
                }
                RunStep::Thought => {
                    participants.push(index);
                    end = index + 1;
                }
                RunStep::Transparent => {}
                RunStep::Break => break,
            }
            index += 1;
        }
        let tools = members
            .iter()
            .filter_map(|&i| tool_at(turn, i))
            .collect::<Vec<_>>();
        let Some(mut summary) = TranscriptToolGroupSummary::from_tool_calls(&tools) else {
            continue;
        };
        // Opened context tools stay transparent, including the first tool of an
        // expanded run. Keep that tool's disclosure key when the header moves.
        let mut anchor_start = start;
        while anchor_start > 0 && context_step(turn, anchor_start - 1) == RunStep::Transparent {
            anchor_start -= 1;
        }
        let anchor = (anchor_start..end).find_map(|i| tool_at(turn, i));
        let expanded = anchor.is_some_and(|tool| tool.group.expanded);
        let target_ids = anchor
            .into_iter()
            .chain(tools.iter().copied().filter(|tool| {
                anchor.is_none_or(|anchor| anchor.tool_call_id != tool.tool_call_id)
            }))
            .map(|tool| tool.tool_call_id.clone())
            .collect();
        summary.disclosure = disclosure(expanded);
        summary.span_len = end - start;
        for &i in &participants {
            claimed[i] = true;
        }
        groups.push(TranscriptToolGroup {
            start,
            members,
            target_ids,
            hidden: if expanded { Vec::new() } else { participants },
            summary,
        });
    }

    // Context runs have priority. Remaining collapsed tools and thoughts share
    // one ten-entry tail budget, even when the tool kinds differ.
    index = 0;
    while index < len {
        if claimed[index] || !dense_participant(turn, index) {
            index += 1;
            continue;
        }
        let start = index;
        while index < len && !claimed[index] && dense_participant(turn, index) {
            index += 1;
        }
        if index - start <= DENSE_VISIBLE_ENTRIES + 1 {
            continue;
        }
        let members = (start..index)
            .filter(|&i| tool_at(turn, i).is_some())
            .collect::<Vec<_>>();
        let tools = members
            .iter()
            .filter_map(|&i| tool_at(turn, i))
            .collect::<Vec<_>>();
        let Some(mut summary) = TranscriptToolGroupSummary::from_tool_calls(&tools) else {
            continue;
        };
        let expanded = tools.first().is_some_and(|tool| tool.group.expanded);
        let target_ids = tools.iter().map(|tool| tool.tool_call_id.clone()).collect();
        summary.kind = TranscriptToolGroupKind::Commands;
        summary.disclosure = disclosure(expanded);
        summary.span_len = index - start;
        groups.push(TranscriptToolGroup {
            start,
            members,
            target_ids,
            hidden: if expanded {
                // The truncation header owns the first participant's row.
                vec![start]
            } else {
                (start..index - DENSE_VISIBLE_ENTRIES).collect()
            },
            summary,
        });
    }
    groups.sort_unstable_by_key(|group| group.start);
    groups
}

fn disclosure(expanded: bool) -> TranscriptToolDisclosureMode {
    if expanded {
        TranscriptToolDisclosureMode::Expanded
    } else {
        TranscriptToolDisclosureMode::Collapsed
    }
}

fn dense_participant(turn: &TranscriptTurnSection, index: usize) -> bool {
    match &turn.assistant_parts[index] {
        TranscriptAssistantPart::Reasoning(_) => {
            !turn.reasoning_expanded && !turn.reasoning_active(index)
        }
        TranscriptAssistantPart::ToolCall(tool) => {
            !tool.expanded
                && !tool.details_preview_visible
                && tool.header.presentation.status != ToolCallPresentationStatus::Waiting
                && !matches!(tool.header.tool_id.as_str(), "question" | "user.question")
        }
        _ => false,
    }
}
