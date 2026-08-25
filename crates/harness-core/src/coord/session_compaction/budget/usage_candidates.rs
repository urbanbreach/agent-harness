use crate::context_budget::RequestBudgetSnapshot;
use crate::coord::compaction::ActivePathCompactionSnapshot;
use crate::event::{
    EventEnvelopeV1, EventV1, ProviderRequestFinishedEvent, ProviderRequestStartedEvent,
};
use crate::session::SessionEntryPayload;

use super::complete_request::{
    AnchorBudgetComponents, RequestTerminalStatus, StartedRequestMetadata, UsageCandidate,
};

pub(super) fn latest_request_budget(
    events: &[EventEnvelopeV1],
    agent_id: &str,
) -> Option<RequestBudgetSnapshot> {
    latest_request_start(events, agent_id)?
        .metadata
        .as_ref()?
        .context_budget
}

pub(super) fn latest_request_start<'a>(
    events: &'a [EventEnvelopeV1],
    agent_id: &str,
) -> Option<&'a ProviderRequestStartedEvent> {
    events.iter().rev().find_map(|event| {
        if event.actor.agent_id.as_deref() != Some(agent_id) {
            return None;
        }
        match &event.payload {
            EventV1::ProviderRequestStarted(started) => Some(started),
            _ => None,
        }
    })
}

pub(super) fn event_usage_candidates<'a>(
    events: &'a [EventEnvelopeV1],
    agent_id: &str,
) -> Vec<UsageCandidate<'a>> {
    events
        .iter()
        .filter_map(|event| {
            if event.actor.agent_id.as_deref() != Some(agent_id) {
                return None;
            }
            let EventV1::AssistantMessageFinished(assistant) = &event.payload else {
                return None;
            };
            let provenance = assistant.provenance.as_ref()?;
            let request_id = provenance.request_id.as_str();
            let started =
                find_request_start(events, agent_id, request_id).map(started_request_metadata);
            let finished = find_request_finish(events, agent_id, request_id);
            Some(UsageCandidate {
                terminal_status: terminal_status(finished),
                request_id,
                provider_id: &provenance.provider_id,
                model_id: &provenance.model_id,
                semantic_usage: provenance.usage.as_ref(),
                finished_usage: finished.and_then(|event| event.usage.as_ref()),
                started,
                through_index: usize::try_from(event.seq).unwrap_or(usize::MAX),
                includes_prior_summary: false,
            })
        })
        .collect()
}

pub(super) fn snapshot_usage_candidates<'a>(
    events: &'a [EventEnvelopeV1],
    snapshot: &'a ActivePathCompactionSnapshot,
) -> Vec<UsageCandidate<'a>> {
    snapshot
        .entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let SessionEntryPayload::AssistantMessage {
                provenance: Some(provenance),
                ..
            } = &entry.entry.payload
            else {
                return None;
            };
            let request_id = provenance.request_id.as_str();
            let started = find_request_start(events, &snapshot.owner.agent_id, request_id)
                .map(started_request_metadata);
            let finished = find_request_finish(events, &snapshot.owner.agent_id, request_id);
            let includes_prior_summary = snapshot
                .prior_active_summary
                .as_ref()
                .and_then(|summary| {
                    let summary_index = snapshot
                        .active_branch
                        .entry_ids
                        .iter()
                        .position(|entry_id| entry_id == &summary.entry_id)?;
                    let entry_index = snapshot
                        .active_branch
                        .entry_ids
                        .iter()
                        .position(|entry_id| entry_id == &entry.entry.id)?;
                    Some(entry_index > summary_index)
                })
                .unwrap_or(false);
            Some(UsageCandidate {
                terminal_status: terminal_status(finished),
                request_id,
                provider_id: &provenance.provider_id,
                model_id: &provenance.model_id,
                semantic_usage: provenance.usage.as_ref(),
                finished_usage: finished.and_then(|event| event.usage.as_ref()),
                started,
                through_index: index,
                includes_prior_summary,
            })
        })
        .collect()
}

fn terminal_status(finished: Option<&ProviderRequestFinishedEvent>) -> RequestTerminalStatus {
    match finished.map(|event| event.finish_reason.as_str()) {
        Some("error") | None => RequestTerminalStatus::Error,
        Some("abort" | "aborted" | "cancel" | "canceled" | "cancelled") => {
            RequestTerminalStatus::Aborted
        }
        Some(_) => RequestTerminalStatus::Completed,
    }
}

fn find_request_start<'a>(
    events: &'a [EventEnvelopeV1],
    agent_id: &str,
    request_id: &str,
) -> Option<&'a ProviderRequestStartedEvent> {
    events.iter().find_map(|event| {
        if event.actor.agent_id.as_deref() != Some(agent_id) {
            return None;
        }
        match &event.payload {
            EventV1::ProviderRequestStarted(started)
                if started.request_id.as_str() == request_id =>
            {
                Some(started)
            }
            _ => None,
        }
    })
}

fn find_request_finish<'a>(
    events: &'a [EventEnvelopeV1],
    agent_id: &str,
    request_id: &str,
) -> Option<&'a ProviderRequestFinishedEvent> {
    events.iter().find_map(|event| {
        if event.actor.agent_id.as_deref() != Some(agent_id) {
            return None;
        }
        match &event.payload {
            EventV1::ProviderRequestFinished(finished)
                if finished.request_id.as_str() == request_id =>
            {
                Some(finished)
            }
            _ => None,
        }
    })
}

fn started_request_metadata(started: &ProviderRequestStartedEvent) -> StartedRequestMetadata<'_> {
    StartedRequestMetadata {
        request_id: started.request_id.as_str(),
        provider_id: &started.provider_id,
        model_id: &started.model_id,
        budget: started
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.context_budget)
            .map(|budget| AnchorBudgetComponents {
                system_tokens: budget.components.system_tokens,
                tools_tokens: budget.components.tools_tokens,
                attachments_tokens: budget.components.attachments_tokens,
                framing_tokens: budget.components.framing_tokens,
            }),
    }
}
