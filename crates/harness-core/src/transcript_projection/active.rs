//! The compaction that currently defines an agent's context, independent of whether its
//! provider-native payload can be replayed.
use crate::event::{
    ArchiveRecord, CompactionPatch, CompactionPatchedEvent, EventEnvelopeV1, EventV1,
    SessionCompactionEvent,
};

/// A compaction with the amendments recorded after it was committed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveCompaction {
    pub seq: u64,
    pub event: SessionCompactionEvent,
    pub patches: Vec<CompactionPatchedEvent>,
}

impl ActiveCompaction {
    /// The warning shown with the compaction: the newest warning patch, else the original.
    pub fn warning(&self) -> Option<&str> {
        warning_of(&self.patches).or(self.event.warning.as_deref())
    }
}

/// The newest warning patch.
pub fn warning_of(patches: &[CompactionPatchedEvent]) -> Option<&str> {
    patches.iter().rev().find_map(|patch| match &patch.patch {
        CompactionPatch::Warning { text } => Some(text.as_str()),
        CompactionPatch::ReaderView { .. } => None,
    })
}

/// The newest reasoning-free view recorded for `reader_key`.
pub fn reader_view<'a>(
    patches: &'a [CompactionPatchedEvent],
    reader_key: &str,
) -> Option<(&'a str, Option<&'a ArchiveRecord>)> {
    patches.iter().rev().find_map(|patch| match &patch.patch {
        CompactionPatch::ReaderView {
            reader_key: key,
            summary,
            archive,
        } if key == reader_key => Some((summary.as_str(), archive.as_ref())),
        _ => None,
    })
}

/// The latest compaction of `agent` on the active journal fold, with its patches in
/// journal order. A rewind that removed a compaction removes it here too.
pub fn latest_compaction(events: &[EventEnvelopeV1], agent: &str) -> Option<ActiveCompaction> {
    let active = crate::conversation_rewind::active_events(events);
    let (seq, event) = active.iter().rev().find_map(|event| match &event.payload {
        EventV1::SessionCompaction(data) if data.agent_id == agent => {
            Some((event.seq, data.clone()))
        }
        _ => None,
    })?;
    let patches = active
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::CompactionPatched(patch)
                if patch.agent_id == agent && patch.compaction_seq == seq =>
            {
                Some(patch.clone())
            }
            _ => None,
        })
        .collect();
    Some(ActiveCompaction {
        seq,
        event,
        patches,
    })
}

/// Wall-clock time of an event in Unix milliseconds, when it carries a valid timestamp.
pub fn event_time_ms(event: &EventEnvelopeV1) -> Option<u64> {
    let timestamp = humantime::parse_rfc3339(event.ts.as_deref()?).ok()?;
    u64::try_from(
        timestamp
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_millis(),
    )
    .ok()
}
