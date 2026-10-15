//! Archive summary rendering and request-only media projections.
use super::*;

mod caches;
mod import;
pub(in crate::coord) mod inline_cache;
mod projection;
mod reader;
mod source;
#[cfg(test)]
mod tests;
pub(in crate::coord) use caches::RenderCaches;
pub(in crate::coord) use import::{archive_frame_attachments, import_archive};
pub(in crate::coord) use inline_cache::InlineCache;
pub(in crate::coord) use projection::{archive_projection, ArchiveText, FrameLimits};
pub(in crate::coord) use reader::{needs_reader_view, ReaderView, ReaderViews};
pub(in crate::coord) use source::{ArchiveFrameSource, RunDirFrames};

fn archive_plain(text: &str) -> String {
    harness_compaction::archive::elide_data_urls(text, true)
        .replace(['\u{e}', '\u{f}'], "")
        .replace('█', "\n")
}

/// Full text of a compaction summary: the guide plus the ordered head and tail text of an
/// archive, with markers where frames sit. Frames themselves are never part of the text;
/// the full transcript and provider requests add them through [`archive_projection`].
pub(in crate::coord) fn summary_text(state: &CompactionState) -> String {
    match &state.archive {
        Some(record) => {
            let mut text = state.summary.clone();
            if !record.text_head.is_empty() {
                text.push_str("\n\n");
                text.push_str(&archive_plain(&record.text_head));
            }
            if !record.frames.is_empty() {
                text.push_str("\n-------------- imaged middle below\n");
            }
            if !record.text_tail.is_empty() {
                if !record.frames.is_empty() {
                    text.push_str("-------------- imaged middle above\n");
                } else if record.truncated_chars > 0 {
                    text.push_str("\n-------------- middle history omitted above\n");
                }
                text.push_str(&archive_plain(&record.text_tail));
            }
            text
        }
        None => harness_compaction::prompts::summary_message(Some(state.method), &state.summary),
    }
}

/// Collapsed text of a compaction summary: the guide only, with no archive text blocks
/// or images. Transcripts show this until the summary is expanded.
pub(in crate::coord) fn collapsed_text(state: &CompactionState) -> String {
    match &state.archive {
        Some(_) => state.summary.clone(),
        None => summary_text(state),
    }
}

/// The compaction sequence a reader view is keyed by: the journal sequence of the
/// compaction event, which a committed summary entry also carries. Zero is an uncommitted
/// draft, whose views are never cached or persisted.
pub(in crate::coord) fn compaction_seq(entry: &Entry, state: &CompactionState) -> u64 {
    if state.commit_seq != 0 {
        state.commit_seq
    } else {
        entry.seq
    }
}

pub(in crate::coord) fn summary_entry(state: CompactionState) -> Entry {
    let mut entry = Entry::new(
        CompletionMessage::text(MessageRole::User, summary_text(&state)),
        0,
        None,
    );
    entry.compaction = Some(Arc::new(state));
    entry
}
