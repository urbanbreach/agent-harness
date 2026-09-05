use std::collections::BTreeSet;

use crate::config::ResolvedModelLimits;
use crate::event::{EventEnvelopeV1, EventV1};
use crate::session::{CanonicalRuntimeSelection, CanonicalSession, SessionEntryPayload};

pub(super) fn from_session(session: &CanonicalSession) -> Option<CanonicalRuntimeSelection> {
    let provenance = session
        .active_path()
        .ok()?
        .into_iter()
        .rev()
        .find_map(|entry| {
            let SessionEntryPayload::AssistantMessage {
                provenance: Some(provenance),
                ..
            } = &entry.payload
            else {
                return None;
            };
            Some(provenance)
        })?;
    provenance.runtime_selection.as_deref().cloned()
}

pub(super) fn from_completed_request(
    events: &[EventEnvelopeV1],
    agent_id: &str,
    runtime_fallback: Option<&CanonicalRuntimeSelection>,
) -> Option<CanonicalRuntimeSelection> {
    let completed_requests = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::AssistantMessageFinished(finished) => {
                Some(finished.request_id.as_str().to_string())
            }
            EventV1::ProviderRequestFinished(finished) if finished.finish_reason != "error" => {
                Some(finished.request_id.as_str().to_string())
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let cancelled_turns = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::TaskCancelled(cancelled)
                if matches!(
                    cancelled.task_scope,
                    Some(crate::event::TaskTerminalScope::AgentTurn)
                ) =>
            {
                event.correlation_id.clone()
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let started = events.iter().rev().find_map(|event| {
        let EventV1::ProviderRequestStarted(started) = &event.payload else {
            return None;
        };
        let turn_key = started
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.turn_id.as_deref())
            .or(event.correlation_id.as_deref())
            .unwrap_or(started.request_id.as_str());
        (event.actor.agent_id.as_deref() == Some(agent_id)
            && (completed_requests.contains(started.request_id.as_str())
                || cancelled_turns.contains(turn_key)))
        .then_some(started)
    });
    match started {
        Some(started) => started
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.runtime_selection.as_deref())
            .cloned()
            .or_else(|| runtime_fallback.cloned())
            .or_else(|| compatibility_selection(&started.provider_id, &started.model_id)),
        None => runtime_fallback.cloned(),
    }
}

fn compatibility_selection(provider_id: &str, model_id: &str) -> Option<CanonicalRuntimeSelection> {
    CanonicalRuntimeSelection::new(
        None,
        provider_id,
        model_id,
        Default::default(),
        ResolvedModelLimits::default(),
        blake3::hash(format!("legacy-profile-shape\0{provider_id}\0{model_id}").as_bytes())
            .to_hex()
            .to_string(),
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{
        ActorKind, EventActor, ProviderRequestFinishedEvent, ProviderRequestStartedEvent,
        ProviderRequestStartedMetadata, SCHEMA_VERSION,
    };

    #[test]
    fn runtime_selection_ignores_a_later_incomplete_request() {
        let completed = selection("provider-completed", "model-completed");
        let incomplete = selection("provider-incomplete", "model-incomplete");
        let events = vec![
            envelope(
                1,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "request-completed".into(),
                    provider_id: "provider-completed".to_string(),
                    model_id: "model-completed".to_string(),
                    prompt_summary: "prompt".to_string(),
                    request_digest: "digest-completed".to_string(),
                    metadata: Some(ProviderRequestStartedMetadata {
                        runtime_selection: Some(Box::new(completed.clone())),
                        ..ProviderRequestStartedMetadata::default()
                    }),
                }),
            ),
            envelope(
                2,
                EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                    request_id: "request-completed".into(),
                    finish_reason: "stop".to_string(),
                    output_digest: None,
                    usage: None,
                    metadata: None,
                }),
            ),
            envelope(
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "request-incomplete".into(),
                    provider_id: "provider-incomplete".to_string(),
                    model_id: "model-incomplete".to_string(),
                    prompt_summary: "retry".to_string(),
                    request_digest: "digest-incomplete".to_string(),
                    metadata: Some(ProviderRequestStartedMetadata {
                        runtime_selection: Some(Box::new(incomplete)),
                        ..ProviderRequestStartedMetadata::default()
                    }),
                }),
            ),
        ];

        assert_eq!(
            from_completed_request(&events, "agent_1", None),
            Some(completed)
        );
    }

    #[test]
    fn runtime_selection_uses_validated_fallback_without_a_completed_request() {
        let fallback = selection("provider-fallback", "model-fallback");
        let events = vec![envelope(
            1,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "request-incomplete".into(),
                provider_id: "provider-incomplete".to_string(),
                model_id: "model-incomplete".to_string(),
                prompt_summary: "prompt".to_string(),
                request_digest: "digest-incomplete".to_string(),
                metadata: None,
            }),
        )];

        assert_eq!(
            from_completed_request(&events, "agent_1", Some(&fallback)),
            Some(fallback)
        );
    }

    fn selection(provider_id: &str, model_id: &str) -> CanonicalRuntimeSelection {
        CanonicalRuntimeSelection {
            profile: None,
            provider_id: provider_id.to_string(),
            model_id: model_id.to_string(),
            variant: None,
            reasoning_effort: None,
            text_verbosity: None,
            reasoning_summary: None,
            thinking: None,
            resolved_limits: ResolvedModelLimits::default(),
            profile_tool_shape_digest: "a".repeat(64),
        }
    }

    fn envelope(seq: u64, payload: EventV1) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("event-{seq}"),
            seq,
            run_id: "run-selection".into(),
            mono_ms: seq,
            ts: None,
            actor: EventActor::new(ActorKind::Worker, Some("agent_1".to_string())),
            correlation_id: None,
            causation_id: None,
            stream_key: None,
            payload,
        }
    }
}
