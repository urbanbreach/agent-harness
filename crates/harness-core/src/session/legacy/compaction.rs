use crate::event::{EventEnvelopeV1, EventV1};
use crate::session::CanonicalProjectionUpdate;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanonicalLegacyCompactionStatus {
    Requested,
    Written,
    Applied,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalLegacyCompaction {
    pub status: CanonicalLegacyCompactionStatus,
    pub agent_id: String,
    pub checkpoint_id: Option<String>,
    pub trigger_reason: String,
    pub deterministic_fallback: bool,
}

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

#[expect(
    deprecated,
    reason = "legacy projection cadence is classified only at the compatibility boundary"
)]
pub(crate) const fn legacy_projection_update_for_event(
    event: &EventV1,
) -> Option<CanonicalProjectionUpdate> {
    match event {
        EventV1::CompactionRequested(_) => Some(CanonicalProjectionUpdate::Buffer),
        EventV1::ProviderStreamDelta(_)
        | EventV1::ProviderReasoningDelta(_)
        | EventV1::CompactionWritten(_)
        | EventV1::CompactionApplied(_)
        | EventV1::CompactionFailed(_) => Some(CanonicalProjectionUpdate::Settle),
        _ => None,
    }
}

#[expect(
    deprecated,
    reason = "legacy compaction details are decoded only through the compatibility boundary"
)]
pub(crate) fn latest_legacy_compaction(
    events: &[EventEnvelopeV1],
) -> Option<CanonicalLegacyCompaction> {
    events.iter().rev().find_map(|event| match &event.payload {
        EventV1::CompactionRequested(payload) => Some(CanonicalLegacyCompaction {
            status: CanonicalLegacyCompactionStatus::Requested,
            agent_id: payload.agent_id.clone(),
            checkpoint_id: Some(payload.checkpoint_id.clone()),
            trigger_reason: payload.trigger_reason.clone(),
            deterministic_fallback: false,
        }),
        EventV1::CompactionWritten(payload) => Some(CanonicalLegacyCompaction {
            status: CanonicalLegacyCompactionStatus::Written,
            agent_id: payload.agent_id.clone(),
            checkpoint_id: Some(payload.checkpoint_id.clone()),
            trigger_reason: payload.trigger_reason.clone(),
            deterministic_fallback: payload
                .summary_source
                .as_ref()
                .is_some_and(|source| source.deterministic_fallback),
        }),
        EventV1::CompactionApplied(payload) => Some(CanonicalLegacyCompaction {
            status: CanonicalLegacyCompactionStatus::Applied,
            agent_id: payload.agent_id.clone(),
            checkpoint_id: Some(payload.checkpoint_id.clone()),
            trigger_reason: "legacy_compatibility".to_string(),
            deterministic_fallback: false,
        }),
        EventV1::CompactionFailed(payload) => Some(CanonicalLegacyCompaction {
            status: CanonicalLegacyCompactionStatus::Failed,
            agent_id: payload.agent_id.clone(),
            checkpoint_id: payload.checkpoint_id.clone(),
            trigger_reason: payload.trigger_reason.clone(),
            deterministic_fallback: false,
        }),
        _ => None,
    })
}
