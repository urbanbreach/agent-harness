use crate::event::{EventEnvelopeV1, EventV1};

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

pub fn canonical_provider_fragment_for_event(
    event: &EventEnvelopeV1,
) -> Option<CanonicalProviderFragment<'_>> {
    let (request_id, kind, delta) = match &event.payload {
        EventV1::ProviderReasoningDelta(payload) => (
            payload.request_id.as_str(),
            CanonicalProviderFragmentKind::Reasoning,
            payload.delta.as_str(),
        ),
        EventV1::ProviderStreamDelta(payload) => (
            payload.request_id.as_str(),
            CanonicalProviderFragmentKind::Text,
            payload.delta.as_str(),
        ),
        _ => return None,
    };
    Some(CanonicalProviderFragment {
        seq: event.seq,
        mono_ms: event.mono_ms,
        turn_request_id: event.correlation_id.as_deref(),
        request_id,
        kind,
        delta,
    })
}
