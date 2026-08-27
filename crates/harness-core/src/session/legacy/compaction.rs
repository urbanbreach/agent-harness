use crate::event::EventV1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CompatibilityEventLifecycle {
    Started(String),
    Finished(Option<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompatibilityEvent {
    pub(crate) event_type: &'static str,
    pub(crate) lifecycle: CompatibilityEventLifecycle,
}

#[expect(
    deprecated,
    reason = "the single legacy adapter retains read-only V1 compaction decoding until G010"
)]
pub(crate) fn classify_compatibility_event(event: &EventV1) -> Option<CompatibilityEvent> {
    match event {
        EventV1::CompactionRequested(payload) => Some(CompatibilityEvent {
            event_type: "compaction_requested",
            lifecycle: CompatibilityEventLifecycle::Started(payload.checkpoint_id.clone()),
        }),
        EventV1::CompactionWritten(payload) => Some(CompatibilityEvent {
            event_type: "compaction_written",
            lifecycle: CompatibilityEventLifecycle::Finished(Some(payload.checkpoint_id.clone())),
        }),
        EventV1::CompactionApplied(payload) => Some(CompatibilityEvent {
            event_type: "compaction_applied",
            lifecycle: CompatibilityEventLifecycle::Finished(Some(payload.checkpoint_id.clone())),
        }),
        EventV1::CompactionFailed(payload) => Some(CompatibilityEvent {
            event_type: "compaction_failed",
            lifecycle: CompatibilityEventLifecycle::Finished(payload.checkpoint_id.clone()),
        }),
        _ => None,
    }
}
