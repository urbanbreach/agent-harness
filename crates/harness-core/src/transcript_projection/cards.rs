//! Compaction cards for the full transcript and the window of the collapsed transcript.
use super::active::{event_time_ms, warning_of};
use crate::event::{
    ActorKind, ArchiveRecord, CompactionMethod, CompactionPatchedEvent, EventEnvelopeV1, EventV1,
    SessionCompactionEvent,
};
use std::collections::{BTreeMap, BTreeSet};

/// Shown instead of the summary of a compaction a newer one replaced.
pub const SUPERSEDED_SUMMARY: &str =
    "[Superseded compaction summary elided after a newer compaction]";
/// Shown instead of the short summary of a replaced compaction.
pub const SUPERSEDED_SHORT: &str = "Superseded compaction elided";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionCardKind {
    Session,
    Branch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionCard {
    pub seq: u64,
    pub agent_id: String,
    pub at_ms: Option<u64>,
    pub kind: CompactionCardKind,
    pub summary: String,
    pub short_summary: Option<String>,
    pub tokens_before: Option<u32>,
    pub tokens_after: Option<u32>,
    pub method: Option<CompactionMethod>,
    pub warning: Option<String>,
    pub archive: Option<ArchiveRecord>,
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
    pub first_kept_seq: Option<u64>,
    /// A newer compaction of the same agent replaced this card's summary.
    pub superseded: bool,
}

/// Where the collapsed transcript starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CollapsedWindow {
    pub compaction_seq: u64,
    /// Originals before this sequence are hidden; the divider follows the kept originals.
    pub first_visible_seq: u64,
}

/// Cards in journal order over the active fold.
///
/// Supersession is per agent, so a child's compaction never replaces the root's card. A
/// compaction a later one names in `supersedes_seq`, or one an identical rescue boundary
/// followed immediately, is omitted.
pub fn compaction_cards(events: &[EventEnvelopeV1]) -> Vec<CompactionCard> {
    let active = crate::conversation_rewind::active_events(events);
    let mut omitted = BTreeSet::new();
    let mut latest: BTreeMap<&str, u64> = BTreeMap::new();
    let mut patches: BTreeMap<u64, Vec<CompactionPatchedEvent>> = BTreeMap::new();
    let mut previous: BTreeMap<&str, (u64, &SessionCompactionEvent, bool)> = BTreeMap::new();
    for event in active.iter() {
        match &event.payload {
            EventV1::SessionCompaction(data) => {
                omitted.extend(data.supersedes_seq);
                if let Some((seq, before, quiet)) = previous.get(data.agent_id.as_str())
                    && *quiet
                    && before.method == data.method
                    && before.first_kept_event_seq == data.first_kept_event_seq
                    && before.tokens_before == data.tokens_before
                {
                    omitted.insert(*seq);
                }
                previous.insert(data.agent_id.as_str(), (event.seq, data, true));
            }
            EventV1::CompactionPatched(patch) => {
                patches
                    .entry(patch.compaction_seq)
                    .or_default()
                    .push(patch.clone());
            }
            EventV1::UserMessageSubmitted(_) | EventV1::AssistantMessageFinished(_) => {
                if let Some(agent) = event.actor.agent_id.as_deref()
                    && let Some(entry) = previous.get_mut(agent)
                {
                    entry.2 = false;
                }
            }
            _ => {}
        }
    }
    for event in active.iter() {
        if let EventV1::SessionCompaction(data) = &event.payload
            && !omitted.contains(&event.seq)
        {
            latest.insert(data.agent_id.as_str(), event.seq);
        }
    }
    let mut cards = Vec::new();
    for event in active.iter() {
        match &event.payload {
            EventV1::SessionCompaction(data) if !omitted.contains(&event.seq) => {
                let current = latest.get(data.agent_id.as_str()) == Some(&event.seq);
                let own = patches.get(&event.seq).map_or(&[][..], Vec::as_slice);
                cards.push(CompactionCard {
                    seq: event.seq,
                    agent_id: data.agent_id.clone(),
                    at_ms: event_time_ms(event),
                    kind: CompactionCardKind::Session,
                    summary: if current {
                        data.summary.clone()
                    } else {
                        SUPERSEDED_SUMMARY.into()
                    },
                    short_summary: if current {
                        data.short_summary.clone()
                    } else {
                        Some(SUPERSEDED_SHORT.into())
                    },
                    tokens_before: Some(data.tokens_before),
                    tokens_after: data.tokens_after,
                    method: data.method,
                    warning: warning_of(own)
                        .map(str::to_owned)
                        .or_else(|| data.warning.clone()),
                    archive: data.archive.clone().filter(|_| current),
                    read_files: data.read_files.clone(),
                    modified_files: data.modified_files.clone(),
                    first_kept_seq: Some(data.first_kept_event_seq),
                    superseded: !current,
                });
            }
            EventV1::BranchSummary(data) if !data.summary.trim().is_empty() => {
                cards.push(CompactionCard {
                    seq: event.seq,
                    agent_id: data.agent_id.clone(),
                    at_ms: event_time_ms(event),
                    kind: CompactionCardKind::Branch,
                    summary: data.summary.clone(),
                    short_summary: None,
                    tokens_before: None,
                    tokens_after: None,
                    method: None,
                    warning: None,
                    archive: None,
                    read_files: data.read_files.clone(),
                    modified_files: data.modified_files.clone(),
                    first_kept_seq: None,
                    superseded: false,
                });
            }
            _ => {}
        }
    }
    cards
}

/// The collapsed transcript window of `agent`.
///
/// Originals before the latest compaction are hidden except the kept ones. A kept interval
/// that begins mid-turn drops its orphan head up to the first user-typed turn start (notes
/// and steering from other agents are not boundaries); with no such boundary the whole kept
/// suffix stays.
pub fn collapsed_window(events: &[EventEnvelopeV1], agent: &str) -> Option<CollapsedWindow> {
    let active = super::active::latest_compaction(events, agent)?;
    let kept = active.event.first_kept_event_seq;
    let after = crate::conversation_rewind::active_events(events);
    let mut messages = after.iter().filter(|event| {
        event.seq >= kept
            && event.actor.agent_id.as_deref().is_none_or(|id| id == agent)
            && matches!(
                event.payload,
                EventV1::UserMessageSubmitted(_)
                    | EventV1::RuntimeReminder(_)
                    | EventV1::AssistantMessageFinished(_)
                    | EventV1::ToolCallFinished(_)
            )
    });
    let first_visible_seq = match messages.next() {
        None => active.seq,
        Some(first) if first.seq > active.seq => active.seq,
        Some(first) if typed_turn_start(first) => first.seq,
        Some(first) => messages
            .take_while(|event| event.seq < active.seq)
            .find(|event| typed_turn_start(event))
            .map_or(first.seq, |boundary| boundary.seq),
    };
    Some(CollapsedWindow {
        compaction_seq: active.seq,
        first_visible_seq,
    })
}

fn typed_turn_start(event: &EventEnvelopeV1) -> bool {
    matches!(event.payload, EventV1::UserMessageSubmitted(_))
        && matches!(event.actor.kind, ActorKind::User | ActorKind::Supervisor)
}
