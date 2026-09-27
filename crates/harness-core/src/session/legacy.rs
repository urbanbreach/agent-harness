use crate::event::{EventEnvelopeV1, EventV1};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyWarning {
    MissingProviderFinish { request_id: String },
    MissingFinalAssistantContent { request_id: String },
    UnsupportedLegacyVariant { event_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanonicalProviderFragmentKind {
    Reasoning,
    Text,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanonicalProviderFragment<'a> {
    pub seq: u64,
    pub mono_ms: u64,
    pub turn_request_id: Option<&'a str>,
    pub request_id: &'a str,
    pub kind: CanonicalProviderFragmentKind,
    pub delta: &'a str,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanonicalProviderFragmentPayload<'a> {
    pub request_id: &'a str,
    pub kind: CanonicalProviderFragmentKind,
    pub delta: &'a str,
}
pub fn canonical_provider_fragment_payload(
    event: &EventV1,
) -> Option<CanonicalProviderFragmentPayload<'_>> {
    let (data, kind) = match event {
        EventV1::ProviderStreamDelta(data) => (data, CanonicalProviderFragmentKind::Text),
        EventV1::ProviderReasoningDelta(data) => (data, CanonicalProviderFragmentKind::Reasoning),
        _ => return None,
    };
    Some(CanonicalProviderFragmentPayload {
        request_id: data.request_id.as_str(),
        kind,
        delta: &data.delta,
    })
}
pub fn canonical_provider_fragment_for_event(
    event: &EventEnvelopeV1,
) -> Option<CanonicalProviderFragment<'_>> {
    let fragment = canonical_provider_fragment_payload(&event.payload)?;
    Some(CanonicalProviderFragment {
        seq: event.seq,
        mono_ms: event.mono_ms,
        turn_request_id: event.correlation_id.as_deref(),
        request_id: fragment.request_id,
        kind: fragment.kind,
        delta: fragment.delta,
    })
}

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

pub fn latest_legacy_compaction(events: &[EventEnvelopeV1]) -> Option<CanonicalLegacyCompaction> {
    use CanonicalLegacyCompactionStatus as Status;
    events.iter().rev().find_map(|event| {
        let (status, agent, checkpoint, reason, fallback) = match &event.payload {
            EventV1::CompactionRequested(data) => (
                Status::Requested,
                &data.agent_id,
                Some(&data.checkpoint_id),
                data.trigger_reason.as_str(),
                false,
            ),
            EventV1::CompactionWritten(data) => (
                Status::Written,
                &data.agent_id,
                Some(&data.checkpoint_id),
                data.trigger_reason.as_str(),
                data.summary_source
                    .as_ref()
                    .is_some_and(|s| s.deterministic_fallback),
            ),
            EventV1::CompactionApplied(data) => (
                Status::Applied,
                &data.agent_id,
                Some(&data.checkpoint_id),
                "legacy_compatibility",
                false,
            ),
            EventV1::CompactionFailed(data) => (
                Status::Failed,
                &data.agent_id,
                data.checkpoint_id.as_ref(),
                data.trigger_reason.as_str(),
                false,
            ),
            _ => return None,
        };
        Some(CanonicalLegacyCompaction {
            status,
            agent_id: agent.clone(),
            checkpoint_id: checkpoint.cloned(),
            trigger_reason: reason.into(),
            deterministic_fallback: fallback,
        })
    })
}
