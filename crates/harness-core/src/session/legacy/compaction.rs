use crate::event::EventV1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LegacyCompactionLifecycle {
    Started(String),
    Finished(Option<String>),
}

#[expect(
    deprecated,
    reason = "the single legacy adapter retains read-only V1 compaction decoding until G010"
)]
pub(crate) fn compaction_lifecycle(event: &EventV1) -> Option<LegacyCompactionLifecycle> {
    match event {
        EventV1::CompactionRequested(payload) => Some(LegacyCompactionLifecycle::Started(
            payload.checkpoint_id.clone(),
        )),
        EventV1::CompactionWritten(payload) => Some(LegacyCompactionLifecycle::Finished(Some(
            payload.checkpoint_id.clone(),
        ))),
        EventV1::CompactionApplied(payload) => Some(LegacyCompactionLifecycle::Finished(Some(
            payload.checkpoint_id.clone(),
        ))),
        EventV1::CompactionFailed(payload) => Some(LegacyCompactionLifecycle::Finished(
            payload.checkpoint_id.clone(),
        )),
        _ => None,
    }
}

#[expect(
    deprecated,
    reason = "the single legacy adapter retains read-only V1 compaction decoding until G010"
)]
pub(crate) fn event_type_name(event: &EventV1) -> Option<&'static str> {
    match event {
        EventV1::CompactionRequested(_) => Some("compaction_requested"),
        EventV1::CompactionWritten(_) => Some("compaction_written"),
        EventV1::CompactionApplied(_) => Some("compaction_applied"),
        EventV1::CompactionFailed(_) => Some("compaction_failed"),
        _ => None,
    }
}

pub(crate) fn is_compaction_event(event: &EventV1) -> bool {
    event_type_name(event).is_some()
}
