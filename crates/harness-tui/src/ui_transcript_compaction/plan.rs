//! Which compaction cards and notices the transcript shows, and from where.
//!
//! The full transcript shows every card at its commit point. The collapsed transcript hides
//! the originals before the latest summary except the ones the summary does not cover, and
//! shows only the latest card. Cards of a child agent never replace the root's card.
use super::super::ui_transcript_types::{
    TranscriptAssistantPart, TranscriptCompactionKind, TranscriptCompactionSection,
    TranscriptNoticeSection,
};
use crate::app::AppState;
use harness_core::event::{CompactionMethod, EventEnvelopeV1, EventV1, PruneKindRecord};
use harness_core::transcript_projection::{
    collapsed_window, compaction_cards, CompactionCard, CompactionCardKind,
};
use std::collections::BTreeSet;
use std::sync::Arc;

/// A part that belongs to the journal position `seq` rather than to a turn.
pub(in crate::ui::ui_transcript) struct TimelineItem {
    pub(in crate::ui::ui_transcript) seq: u64,
    pub(in crate::ui::ui_transcript) part: TranscriptAssistantPart,
}

pub(in crate::ui::ui_transcript) struct CompactionTimeline {
    /// Settled turns that end before this sequence are not shown.
    pub(in crate::ui::ui_transcript) hide_before: Option<u64>,
    /// Cards and notices in journal order.
    pub(in crate::ui::ui_transcript) items: Vec<TimelineItem>,
}

pub(in crate::ui::ui_transcript) fn plan(app: &AppState) -> CompactionTimeline {
    let events = app.retained_events();
    let events = events.as_ref();
    let collapse = app.collapse_compacted_history();
    let children = child_agents(events);
    let own_session = app.current_subagent_session_present();
    let shown_agent = |agent: &str| own_session || !children.contains(agent);
    let cards = compaction_cards(events)
        .into_iter()
        .filter(|card| shown_agent(&card.agent_id))
        .collect::<Vec<_>>();
    let window = collapse
        .then(|| {
            cards
                .iter()
                .rev()
                .find(|card| card.kind == CompactionCardKind::Session && !card.superseded)
                .and_then(|card| collapsed_window(events, &card.agent_id))
        })
        .flatten();
    let hide_before = window.map(|window| window.first_visible_seq);
    let hint = app.compaction_expand_hint();
    let mut items = Vec::new();
    for card in &cards {
        let latest = window.is_some_and(|window| window.compaction_seq == card.seq);
        let hidden = match card.kind {
            CompactionCardKind::Session => window.is_some() && !latest,
            CompactionCardKind::Branch => hide_before.is_some_and(|first| card.seq < first),
        };
        if !hidden {
            items.push(TimelineItem {
                seq: card.seq,
                part: card_part(app, events, card, collapse, &hint),
            });
        }
    }
    let active = harness_core::conversation_rewind::active_events(events);
    for event in active.iter() {
        let EventV1::ContextPruned(data) = &event.payload else {
            continue;
        };
        if data.kind != PruneKindRecord::Shake
            || !shown_agent(&data.agent_id)
            || hide_before.is_some_and(|first| event.seq < first)
        {
            continue;
        }
        items.push(TimelineItem {
            seq: event.seq,
            part: TranscriptAssistantPart::Compaction(TranscriptCompactionSection {
                seq: event.seq,
                expanded: app.compaction_expanded(event.seq),
                kind: TranscriptCompactionKind::SessionCompaction,
                summary: format!(
                    "Locally pruned context. Freed {} tokens.",
                    data.tokens_saved
                ),
                short_summary: None,
                at_ms: harness_core::transcript_projection::event_time_ms(event),
                superseded: false,
                rollover: false,
                tokens_before: None,
                tokens_after: None,
                method: Some(CompactionMethod::Shake),
                warning: data.warning.clone(),
                archive: None,
                archive_frames: 0,
                read_files: Vec::new(),
                modified_files: Vec::new(),
                expand_hint: hint.clone(),
            }),
        });
    }
    items.extend(
        app.transcript_notices()
            .iter()
            .enumerate()
            .filter(|(_, notice)| hide_before.is_none_or(|first| notice.anchor_seq >= first))
            .map(|(ordinal, notice)| TimelineItem {
                seq: notice.anchor_seq,
                part: TranscriptAssistantPart::Notice(TranscriptNoticeSection {
                    ordinal,
                    anchor_seq: notice.anchor_seq,
                    kind: notice.kind,
                    text: notice.text.clone(),
                }),
            }),
    );
    // A notice follows a card journaled at the same position.
    items.sort_by_key(|item| {
        (
            item.seq,
            matches!(item.part, TranscriptAssistantPart::Notice(_)),
        )
    });
    CompactionTimeline { hide_before, items }
}

fn card_part(
    app: &AppState,
    events: &[EventEnvelopeV1],
    card: &CompactionCard,
    collapse: bool,
    hint: &str,
) -> TranscriptAssistantPart {
    // Frames and archive text belong to the active card of the full transcript.
    let archive = card
        .archive
        .clone()
        .filter(|_| !collapse && !card.superseded)
        .map(Arc::new);
    TranscriptAssistantPart::Compaction(TranscriptCompactionSection {
        seq: card.seq,
        expanded: app.compaction_expanded(card.seq),
        kind: match card.kind {
            CompactionCardKind::Session => TranscriptCompactionKind::SessionCompaction,
            CompactionCardKind::Branch => TranscriptCompactionKind::BranchSummary,
        },
        summary: card.summary.clone(),
        short_summary: card.short_summary.clone(),
        at_ms: card.at_ms,
        superseded: card.superseded,
        rollover: is_rollover(events, card.seq),
        tokens_before: card.tokens_before,
        tokens_after: card.tokens_after,
        method: card.method,
        warning: card.warning.clone(),
        archive_frames: archive.as_ref().map_or(0, |archive| archive.frames.len()),
        archive,
        read_files: card.read_files.clone(),
        modified_files: card.modified_files.clone(),
        expand_hint: hint.to_owned(),
    })
}

fn is_rollover(events: &[EventEnvelopeV1], seq: u64) -> bool {
    events
        .binary_search_by_key(&seq, |event| event.seq)
        .ok()
        .and_then(|index| match &events[index].payload {
            EventV1::SessionCompaction(data) => data.details.as_ref(),
            _ => None,
        })
        .and_then(|details| details.get("kind"))
        .and_then(|kind| kind.as_str())
        == Some("notes_rollover")
}

fn child_agents(events: &[EventEnvelopeV1]) -> BTreeSet<&str> {
    events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::AgentSpawned(data) if data.parent_agent_id.is_some() => {
                Some(data.agent_id.as_str())
            }
            EventV1::NativeSubagentRegistered(data) => Some(data.child_id.as_str()),
            _ => None,
        })
        .collect()
}
