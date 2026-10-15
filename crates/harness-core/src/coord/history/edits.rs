//! Applying durable context edits to restored entries.
use super::*;
use harness_compaction::ItemPart;
use std::borrow::Cow;

/// Text a pruned result shows when its replacement is empty.
const EMPTY_RESULT: &str = "[Output truncated]";
/// Text an entry shows when removing its images left it empty.
pub(in crate::coord) const IMAGE_REMOVED: &str = "[image removed]";

pub(in crate::coord) fn apply_edits(
    entries: &mut [Entry],
    edits: &[crate::event::ContextEdit],
    tool_ids: &HashMap<String, (u64, String)>,
) {
    use crate::event::ContextEdit;
    for edit in edits {
        for entry in entries.iter_mut() {
            match edit {
                ContextEdit::ToolResult {
                    seq,
                    tool_call_id,
                    text,
                    pruned_at_ms,
                    drop_attachments,
                } if seq.map_or_else(
                    || {
                        tool_ids
                            .get(tool_call_id)
                            .is_some_and(|(seq, provider_id)| {
                                entry.seq == *seq
                                    && entry.message.tool_call_id.as_deref()
                                        == Some(provider_id.as_str())
                            })
                    },
                    |seq| {
                        entry.seq == seq
                            && entry.message.tool_call_id.as_deref() == Some(tool_call_id.as_str())
                    },
                ) =>
                {
                    replace_result(entry, text, *pruned_at_ms, *drop_attachments);
                }
                ContextEdit::TextRange {
                    seq,
                    start,
                    end,
                    text,
                    elided_output_tokens,
                } if entry.seq == *seq && entry.message.role != MessageRole::Tool => {
                    splice_text(
                        entry,
                        *start as usize,
                        *end as usize,
                        text,
                        *elided_output_tokens,
                    );
                }
                ContextEdit::DropReasoning {
                    seq,
                    elided_output_tokens,
                } if entry.seq == *seq => drop_reasoning(entry, *elided_output_tokens),
                ContextEdit::DropAttachments { seq, tool_call_id }
                    if entry.seq == *seq
                        && tool_call_id.as_ref().is_none_or(|id| {
                            entry.message.tool_call_id.as_deref()
                                == Some(
                                    tool_ids.get(id).map_or(id.as_str(), |(_, provider_id)| {
                                        provider_id.as_str()
                                    }),
                                )
                        }) =>
                {
                    drop_images(entry);
                }
                _ => {}
            }
        }
    }
}

/// Every mutating edit bumps the version so cached token counts for the entry are stale.
fn bump(entry: &mut Entry) {
    entry.version = entry.version.wrapping_add(1);
}

/// Replaces a tool result's text. The reducer stamps `pruned_at_ms`; the result's own time
/// is untouched so idle timing keeps measuring when the result arrived. Non-text blocks
/// (attachments) survive unless the edit asks to drop them.
fn replace_result(entry: &mut Entry, text: &str, pruned_at_ms: u64, drop_attachments: bool) {
    entry.message.content = if text.is_empty() {
        EMPTY_RESULT.to_owned()
    } else {
        text.to_owned()
    };
    entry.tool.pruned = true;
    entry.pruned_at_ms = Some(pruned_at_ms);
    if drop_attachments {
        entry.attachments.clear();
        entry.images.clear();
    }
    bump(entry);
}

/// Splices `text` over `start..end` of the entry's flat content, keeping ordered text parts
/// in step, and tallies the output tokens the splice removed.
fn splice_text(entry: &mut Entry, start: usize, end: usize, text: &str, elided: u32) {
    let content = &entry.message.content;
    if start > end
        || end > content.len()
        || !content.is_char_boundary(start)
        || !content.is_char_boundary(end)
    {
        return;
    }
    if !entry.parts.is_empty() && !splice_parts(&mut entry.parts, content, start, end, text) {
        entry.parts.clear();
    }
    entry.message.content.replace_range(start..end, text);
    entry.elided_output_tokens = entry.elided_output_tokens.saturating_add(elided);
    bump(entry);
}

/// Applies the same splice to the text blocks when they reassemble the content exactly.
fn splice_parts(
    parts: &mut [ItemPart<'static>],
    content: &str,
    start: usize,
    end: usize,
    text: &str,
) -> bool {
    let mut offset = 0;
    for part in parts.iter() {
        if let ItemPart::Text(block) = part {
            if !content[offset..].starts_with(block.as_ref()) {
                return false;
            }
            offset += block.len();
        }
    }
    if offset != content.len() {
        return false;
    }
    let (mut offset, mut placed) = (0, false);
    for part in parts.iter_mut() {
        let ItemPart::Text(block) = part else {
            continue;
        };
        let (from, to) = (offset, offset + block.len());
        offset = to;
        if from >= end || to <= start {
            continue;
        }
        let (keep_head, keep_tail) = (start.max(from) - from, end.min(to) - from);
        let mut spliced = String::with_capacity(block.len());
        spliced.push_str(&block[..keep_head]);
        if !placed {
            spliced.push_str(text);
            placed = true;
        }
        spliced.push_str(&block[keep_tail..]);
        *block = Cow::Owned(spliced);
    }
    placed
}

/// Removes visible and opaque reasoning. Anthropic continuations exist only to carry signed
/// thinking blocks, so they go with it; other protocols' continuations carry unrelated state.
fn drop_reasoning(entry: &mut Entry, elided: u32) {
    entry.settled_reasoning.clear();
    entry
        .parts
        .retain(|part| !matches!(part, ItemPart::Reasoning(_) | ItemPart::RedactedReasoning));
    if entry
        .message
        .continuation
        .as_ref()
        .is_some_and(|c| c.protocol() == harness_providers::Protocol::Anthropic)
    {
        entry.message.continuation = None;
    }
    entry.elided_output_tokens = entry.elided_output_tokens.saturating_add(elided);
    bump(entry);
}

/// Removes image attachments only; other attachments stay. An entry left with nothing to
/// show gets the image-removed placeholder.
fn drop_images(entry: &mut Entry) {
    entry.attachments.retain(|a| !a.mime.starts_with("image/"));
    entry.images.clear();
    if entry.message.content.trim().is_empty() && entry.attachments.is_empty() {
        entry.message.content = IMAGE_REMOVED.to_owned();
    }
    bump(entry);
}

#[cfg(test)]
mod tests;
