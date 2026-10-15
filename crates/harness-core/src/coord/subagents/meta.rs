//! Capturing and restoring the per-entry facts of a semantic finalized snapshot.
use super::{super::context::Entry, *};
use harness_compaction::{ItemPart, NoteKind, Origin, Stop, ToolFacts};
use std::sync::Arc;

pub(super) fn capture(entry: &Entry) -> FinalizedItemMeta {
    FinalizedItemMeta {
        at_ms: entry.at_ms,
        stop: match entry.stop {
            Stop::Complete => FinalizedStop::Complete,
            Stop::Truncated => FinalizedStop::Truncated,
            Stop::Aborted => FinalizedStop::Aborted,
            Stop::Error => FinalizedStop::Error,
        },
        origin: match entry.origin {
            Origin::User => FinalizedOrigin::User,
            Origin::Agent => FinalizedOrigin::Agent,
            Origin::Host => FinalizedOrigin::Host,
        },
        note_kind: entry.note_kind.map(note_kind),
        usage: entry.usage.clone(),
        pruned_at_ms: entry.pruned_at_ms,
        elided_output_tokens: entry.elided_output_tokens,
        parts: entry
            .parts
            .iter()
            .filter_map(|part| match part {
                ItemPart::Text(text) => Some(FinalizedItemPart::Text {
                    text: text.to_string(),
                }),
                ItemPart::Call(index) => Some(FinalizedItemPart::Call { index: *index }),
                ItemPart::Reasoning(_) | ItemPart::RedactedReasoning => None,
            })
            .collect(),
        tool: FinalizedToolFacts {
            is_error: entry.tool.is_error,
            useless: entry.tool.useless,
            pruned: entry.tool.pruned,
            read_complete: entry.tool.read_complete,
            internal_artifact: entry.tool.internal_artifact,
        },
        compaction: entry.compaction.as_deref().map(compaction_event),
        branch_summary: entry.branch_summary.as_deref().cloned(),
    }
}

/// The durable record of a live compaction state, for a snapshot.
fn compaction_event(
    state: &super::super::context::CompactionState,
) -> crate::event::SessionCompactionEvent {
    crate::event::SessionCompactionEvent {
        summary: state.summary.clone(),
        first_kept_event_seq: state.commit_seq,
        tokens_before: state.tokens_before,
        tokens_after: state.tokens_after,
        read_files: state.read_files.clone(),
        modified_files: state.modified_files.clone(),
        from_hook: state.from_hook,
        short_summary: state.short_summary.clone(),
        method: Some(state.method),
        warning: state.warning.clone(),
        native: state.native.clone(),
        archive: state.archive.clone(),
        details: state.details.clone(),
        preserve_data: state.preserve_data.clone(),
        ..Default::default()
    }
}

fn note_kind(kind: NoteKind) -> FinalizedNoteKind {
    match kind {
        NoteKind::Reminder => FinalizedNoteKind::Reminder,
        NoteKind::CompactionResume => FinalizedNoteKind::CompactionResume,
        NoteKind::LengthStop => FinalizedNoteKind::LengthStop,
        NoteKind::ChildCompletion => FinalizedNoteKind::ChildCompletion,
        NoteKind::Wake => FinalizedNoteKind::Wake,
        NoteKind::Peer => FinalizedNoteKind::Peer,
        NoteKind::Handoff => FinalizedNoteKind::Handoff,
        NoteKind::Hook => FinalizedNoteKind::Hook,
        NoteKind::NotesWindow => FinalizedNoteKind::NotesWindow,
        NoteKind::Other => FinalizedNoteKind::Other,
    }
}

fn restored_note_kind(kind: FinalizedNoteKind) -> NoteKind {
    match kind {
        FinalizedNoteKind::Reminder => NoteKind::Reminder,
        FinalizedNoteKind::CompactionResume => NoteKind::CompactionResume,
        FinalizedNoteKind::LengthStop => NoteKind::LengthStop,
        FinalizedNoteKind::ChildCompletion => NoteKind::ChildCompletion,
        FinalizedNoteKind::Wake => NoteKind::Wake,
        FinalizedNoteKind::Peer => NoteKind::Peer,
        FinalizedNoteKind::Handoff => NoteKind::Handoff,
        FinalizedNoteKind::Hook => NoteKind::Hook,
        FinalizedNoteKind::NotesWindow => NoteKind::NotesWindow,
        FinalizedNoteKind::Other => NoteKind::Other,
    }
}

/// Restores the captured facts onto an entry rebuilt from a snapshot item. A compaction
/// replays its native payload only through the same loader a journal rebuild uses.
pub(super) fn restore(
    entry: &mut Entry,
    meta: &FinalizedItemMeta,
    run_dir: &std::path::Path,
    request: &harness_providers::CompletionRequest,
) {
    entry.at_ms = meta.at_ms;
    entry.stop = match meta.stop {
        FinalizedStop::Complete => Stop::Complete,
        FinalizedStop::Truncated => Stop::Truncated,
        FinalizedStop::Aborted => Stop::Aborted,
        FinalizedStop::Error => Stop::Error,
    };
    entry.origin = match meta.origin {
        FinalizedOrigin::User => Origin::User,
        FinalizedOrigin::Agent => Origin::Agent,
        FinalizedOrigin::Host => Origin::Host,
    };
    entry.note_kind = meta.note_kind.map(restored_note_kind);
    entry.usage = meta.usage.clone();
    entry.pruned_at_ms = meta.pruned_at_ms;
    entry.elided_output_tokens = meta.elided_output_tokens;
    entry.parts = meta
        .parts
        .iter()
        .map(|part| match part {
            FinalizedItemPart::Text { text } => ItemPart::text(text.clone()),
            FinalizedItemPart::Call { index } => ItemPart::Call(*index),
        })
        .collect();
    entry.tool = ToolFacts {
        is_error: meta.tool.is_error,
        useless: meta.tool.useless,
        pruned: meta.tool.pruned,
        read_complete: meta.tool.read_complete,
        internal_artifact: meta.tool.internal_artifact,
    };
    entry.branch_summary = meta.branch_summary.clone().map(Arc::new);
    if let Some(event) = &meta.compaction {
        let payload = event.native.as_ref().and_then(|native| {
            super::super::history::native_payload(native, Some(request), run_dir)
        });
        let replacement = super::super::history::compaction_entry(
            entry.seq,
            entry.at_ms,
            event,
            &[],
            payload,
            None,
        );
        entry.compaction = replacement.compaction;
        entry.attachments = replacement.attachments;
        entry.message.continuation = replacement.message.continuation;
    }
}

/// Fills each restored summary's rewrite marker from the entry that follows it.
pub(super) fn settle_rewrite_marks(entries: &mut [Entry]) {
    for index in 0..entries.len() {
        if entries[index].compaction.is_none() {
            continue;
        }
        let mark = super::super::history::history_rewrite_at_ms(entries, index + 1);
        if let Some(state) = entries[index].compaction.as_mut() {
            Arc::make_mut(state).history_rewrite_at_ms = mark;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_providers::{CompletionMessage, CompletionRequest, MessageRole};

    #[test]
    fn captured_facts_survive_the_durable_round_trip() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let event = crate::event::SessionCompactionEvent {
            summary: "summary".into(),
            warning: Some("careful".into()),
            ..Default::default()
        };
        let mut summary =
            super::super::super::history::compaction_entry(9, Some(5_000), &event, &[], None, None);
        summary.stop = Stop::Truncated;
        summary.note_kind = Some(NoteKind::Handoff);
        summary.pruned_at_ms = Some(7);
        let meta: FinalizedItemMeta =
            serde_json::from_value(serde_json::to_value(capture(&summary))?)?;
        let mut restored = Entry::new(
            CompletionMessage::text(MessageRole::User, "summary"),
            9,
            None,
        );
        restore(
            &mut restored,
            &meta,
            dir.path(),
            &CompletionRequest::default(),
        );
        let state = restored.compaction.as_ref().ok_or("compaction lost")?;
        assert_eq!(state.warning.as_deref(), Some("careful"));
        assert_eq!(state.commit_seq, 9);
        assert_eq!(restored.at_ms, Some(5_000));
        assert_eq!(restored.stop, Stop::Truncated);
        assert_eq!(restored.note_kind, Some(NoteKind::Handoff));
        assert_eq!(restored.pruned_at_ms, Some(7));
        Ok(())
    }
}
