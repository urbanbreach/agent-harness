use super::super::super::context::{Context, Entry};
use harness_compaction::Origin;
use harness_providers::{FilesDue, MessageRole};

/// Which carried file metadata joins the summarized prefix request for a cut at `end`: none
/// when nothing is kept; when the kept tail starts with an assistant message, what is due
/// before that message's time; when it starts on the user side, what is due before the last
/// summarized message that is not a tool result (all of it when there is none). Entries
/// without a time fall back to position.
pub(super) fn files_due(entries: &[Entry], end: usize) -> FilesDue {
    let Some(first) = entries.get(end) else {
        return FilesDue::All;
    };
    let cutoff = if first.message.role == MessageRole::Assistant {
        Some(first.at_ms)
    } else {
        match entries[..end]
            .iter()
            .rev()
            .find(|entry| entry.message.role != MessageRole::Tool)
        {
            Some(last) => Some(last.at_ms),
            None => return FilesDue::Nothing,
        }
    };
    cutoff
        .flatten()
        .map_or(FilesDue::Position, FilesDue::Before)
}

/// The first safe boundary at or after `start` for a signed summary of the prefix: no tool
/// call is split from its result, the kept tail never starts with a system message or a
/// harness-authored note, and the boundary changes between assistant and other sides. With
/// no such boundary the whole range is summarized.
pub(super) fn anthropic_cut(context: &Context, start: usize, end: usize) -> usize {
    let end = end.min(context.entries.len());
    let mut protected = 0;
    let results: std::collections::HashMap<_, _> = context.entries[..end]
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| entry.message.tool_call_id.as_deref().map(|id| (id, index)))
        .collect();
    for (index, entry) in context.entries[..end].iter().enumerate() {
        for call in entry.message.assistant_tool_calls.iter().flatten() {
            if let Some(&result) = results
                .get(call.tool_call_id.as_str())
                .filter(|&&result| result > index)
            {
                protected = protected.max(result + 1);
            }
        }
        let cut = index + 1;
        if cut < start || cut < protected || cut >= end {
            continue;
        }
        let next = &context.entries[cut];
        let host_note = next.origin == Origin::Host
            && next.compaction.is_none()
            && next.branch_summary.is_none();
        if next.seq == 0 || next.message.role == MessageRole::System || host_note {
            continue;
        }
        if (entry.message.role == MessageRole::Assistant)
            != (next.message.role == MessageRole::Assistant)
        {
            return cut;
        }
    }
    end
}
