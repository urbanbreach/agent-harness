use crate::conversation::{
    project_conversation, ConversationProjection, ConversationProjectionError,
};
use crate::event::{EventEnvelopeV1, EventV1};
use crate::proj::{
    inspect_resume_plan_from_events, project_resume_plan, project_resume_plan_from_run_history,
    project_run_summary, project_session_catalog_entry, project_timeline_index, ProjectionError,
    ResumePlan, RunSummary, SessionCatalogEntry, SessionCatalogMetadata, TimelineIndex,
};
use crate::transcript_projection::{
    project_transcript, TranscriptProjection, TranscriptProjectionError,
};
use std::path::Path;

use super::legacy::{
    canonical_provider_fragment_for_event, latest_legacy_compaction,
    legacy_projection_update_for_event, CanonicalLegacyCompaction, CanonicalProviderFragment,
    LegacyAdapterError, LegacyAuditReference, LegacyEventLogAdapter, LegacyProvenance,
    LegacySessionSnapshot, LegacyWarning,
};
use super::CanonicalSession;

#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalSessionProjection {
    source_events: Vec<EventEnvelopeV1>,
    pub session: CanonicalSession,
    pub conversation: ConversationProjection,
    pub run_summary: RunSummary,
    pub resume_plan: ResumePlan,
    pub timeline: TimelineIndex,
    pub transcript: TranscriptProjection,
    pub source: LegacyProvenance,
    pub compatibility_warnings: Vec<LegacyWarning>,
    pub audit_timeline: Vec<LegacyAuditReference>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanonicalProviderRequestStart<'a> {
    pub seq: u64,
    pub mono_ms: u64,
    pub turn_request_id: Option<&'a str>,
    pub request_id: &'a str,
    pub agent_id: Option<&'a str>,
    pub provider_id: &'a str,
    pub model_id: &'a str,
    pub prompt_summary: &'a str,
    pub request_digest: &'a str,
    pub metadata: Option<&'a crate::event::ProviderRequestStartedMetadata>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanonicalProviderRequestFinish<'a> {
    pub seq: u64,
    pub mono_ms: u64,
    pub turn_request_id: Option<&'a str>,
    pub payload: &'a crate::event::ProviderRequestFinishedEvent,
}

#[derive(Debug, Clone, Copy)]
pub struct CanonicalBackgroundNotification<'a> {
    pub seq: u64,
    pub mono_ms: u64,
    pub timestamp: Option<&'a str>,
    pub actor_kind: crate::event::ActorKind,
    pub actor_agent_id: Option<&'a str>,
    pub correlation_id: Option<&'a str>,
    pub payload: &'a crate::event::BackgroundTaskNotificationEvent,
}

#[derive(Debug, Clone, Copy)]
pub struct CanonicalStaleDetection<'a> {
    pub seq: u64,
    pub mono_ms: u64,
    pub timestamp: Option<&'a str>,
    pub task_id: &'a str,
    pub stale_for_ms: u64,
}

#[derive(Debug, Clone, Copy)]
pub enum CanonicalEditPayload<'a> {
    Proposed(&'a crate::event::EditProposedEvent),
    Applied(&'a crate::event::EditAppliedEvent),
    Rejected(&'a crate::event::EditRejectedEvent),
}

#[derive(Debug, Clone, Copy)]
pub struct CanonicalEditEvent<'a> {
    pub seq: u64,
    pub tool_call_id: Option<&'a str>,
    pub payload: CanonicalEditPayload<'a>,
}

impl CanonicalSessionProjection {
    pub(crate) fn conversation_from_event_history(
        events: &[EventEnvelopeV1],
    ) -> Result<ConversationProjection, ConversationProjectionError> {
        project_conversation(events, &[])
    }

    pub fn from_event_history(
        events: &[EventEnvelopeV1],
    ) -> Result<Self, CanonicalSessionProjectionError> {
        let snapshot = LegacyEventLogAdapter::new().project(events)?;
        let conversation = project_conversation(events, &[])?;
        let transcript = project_transcript(events)?;
        let (run_summary, resume_plan, timeline) = Self::project_operational_lossy(events)?;
        Ok(Self::from_parts(
            snapshot,
            conversation,
            run_summary,
            resume_plan,
            timeline,
            transcript,
            events,
        ))
    }

    pub fn from_strict_event_history(
        events: &[EventEnvelopeV1],
    ) -> Result<Self, CanonicalSessionProjectionError> {
        let snapshot = LegacyEventLogAdapter::new().project(events)?;
        let conversation = project_conversation(events, &[])?;
        let transcript = project_transcript(events)?;
        let (run_summary, resume_plan, timeline) = Self::project_operational_strict(events)?;
        Ok(Self::from_parts(
            snapshot,
            conversation,
            run_summary,
            resume_plan,
            timeline,
            transcript,
            events,
        ))
    }

    pub fn from_run_history(
        run_dir: &Path,
        fallback_run_id: &str,
        events: &[EventEnvelopeV1],
    ) -> Result<Self, CanonicalSessionProjectionError> {
        let snapshot = LegacyEventLogAdapter::new().project(events)?;
        let conversation = project_conversation(events, &[])?;
        let transcript = project_transcript(events)?;
        let run_summary = project_run_summary(events)?;
        let resume_plan = inspect_resume_plan_from_events(run_dir, fallback_run_id, events);
        let timeline = project_timeline_index(events)?;
        Ok(Self::from_parts(
            snapshot,
            conversation,
            run_summary,
            resume_plan,
            timeline,
            transcript,
            events,
        ))
    }

    pub fn from_strict_run_history(
        run_dir: &Path,
        fallback_run_id: &str,
        events: &[EventEnvelopeV1],
    ) -> Result<Self, CanonicalSessionProjectionError> {
        let snapshot = LegacyEventLogAdapter::new().project(events)?;
        let conversation = project_conversation(events, &[])?;
        let transcript = project_transcript(events)?;
        let run_summary = project_run_summary(events)?;
        let resume_plan = project_resume_plan_from_run_history(run_dir, fallback_run_id, events)?;
        let timeline = project_timeline_index(events)?;
        Ok(Self::from_parts(
            snapshot,
            conversation,
            run_summary,
            resume_plan,
            timeline,
            transcript,
            events,
        ))
    }

    pub(crate) fn from_owner_event_history(
        events: &[EventEnvelopeV1],
        owner_events: &[EventEnvelopeV1],
        agent_id: &str,
    ) -> Result<Self, CanonicalSessionProjectionError> {
        let snapshot = LegacyEventLogAdapter::new().project_owner(events, agent_id)?;
        let conversation = project_conversation(owner_events, &[])?;
        let transcript = project_transcript(events)?;
        let (run_summary, resume_plan, timeline) = Self::project_operational_strict(events)?;
        Ok(Self::from_parts(
            snapshot,
            conversation,
            run_summary,
            resume_plan,
            timeline,
            transcript,
            events,
        ))
    }

    fn from_parts(
        snapshot: LegacySessionSnapshot,
        conversation: ConversationProjection,
        run_summary: RunSummary,
        resume_plan: ResumePlan,
        timeline: TimelineIndex,
        transcript: TranscriptProjection,
        source_events: &[EventEnvelopeV1],
    ) -> Self {
        Self {
            source_events: source_events.to_vec(),
            session: snapshot.session,
            conversation,
            run_summary,
            resume_plan,
            timeline,
            transcript,
            source: snapshot.provenance,
            compatibility_warnings: snapshot.warnings,
            audit_timeline: snapshot.audit_timeline,
        }
    }

    pub fn apply_event(
        &mut self,
        event: EventEnvelopeV1,
    ) -> Result<(), CanonicalSessionProjectionError> {
        self.apply_events(std::slice::from_ref(&event))
    }

    pub fn apply_events(
        &mut self,
        new_events: &[EventEnvelopeV1],
    ) -> Result<(), CanonicalSessionProjectionError> {
        let mut events = self.source_events.clone();
        events.extend_from_slice(new_events);
        *self = Self::from_event_history(&events)?;
        Ok(())
    }

    pub fn project_catalog_entry(
        &self,
        fallback_run_id: &str,
        metadata: Option<&SessionCatalogMetadata>,
        last_updated_at: Option<String>,
        degraded_reason: Option<String>,
    ) -> Result<SessionCatalogEntry, ProjectionError> {
        project_session_catalog_entry(
            self.source_events.iter(),
            fallback_run_id,
            metadata,
            last_updated_at,
            degraded_reason,
        )
    }

    pub fn provider_request_starts(
        &self,
    ) -> impl Iterator<Item = CanonicalProviderRequestStart<'_>> {
        self.source_events.iter().filter_map(|event| {
            let EventV1::ProviderRequestStarted(payload) = &event.payload else {
                return None;
            };
            Some(CanonicalProviderRequestStart {
                seq: event.seq,
                mono_ms: event.mono_ms,
                turn_request_id: event.correlation_id.as_deref(),
                request_id: payload.request_id.as_str(),
                agent_id: event.actor.agent_id.as_deref(),
                provider_id: &payload.provider_id,
                model_id: &payload.model_id,
                prompt_summary: &payload.prompt_summary,
                request_digest: &payload.request_digest,
                metadata: payload.metadata.as_ref(),
            })
        })
    }

    pub fn provider_request_finishes(
        &self,
    ) -> impl Iterator<Item = CanonicalProviderRequestFinish<'_>> {
        self.source_events.iter().filter_map(|event| {
            let EventV1::ProviderRequestFinished(payload) = &event.payload else {
                return None;
            };
            Some(CanonicalProviderRequestFinish {
                seq: event.seq,
                mono_ms: event.mono_ms,
                turn_request_id: event.correlation_id.as_deref(),
                payload,
            })
        })
    }

    pub fn provider_fragments(&self) -> impl Iterator<Item = CanonicalProviderFragment<'_>> {
        self.source_events
            .iter()
            .filter_map(canonical_provider_fragment_for_event)
    }

    pub fn latest_legacy_compaction(&self) -> Option<CanonicalLegacyCompaction> {
        latest_legacy_compaction(&self.source_events)
    }

    pub fn background_notifications(
        &self,
    ) -> impl Iterator<Item = CanonicalBackgroundNotification<'_>> {
        self.source_events.iter().filter_map(|event| {
            let EventV1::BackgroundTaskNotification(payload) = &event.payload else {
                return None;
            };
            Some(CanonicalBackgroundNotification {
                seq: event.seq,
                mono_ms: event.mono_ms,
                timestamp: event.ts.as_deref(),
                actor_kind: event.actor.kind,
                actor_agent_id: event.actor.agent_id.as_deref(),
                correlation_id: event.correlation_id.as_deref(),
                payload,
            })
        })
    }

    pub fn stale_detections(&self) -> impl Iterator<Item = CanonicalStaleDetection<'_>> {
        self.source_events.iter().filter_map(|event| {
            let EventV1::StaleDetected(payload) = &event.payload else {
                return None;
            };
            Some(CanonicalStaleDetection {
                seq: event.seq,
                mono_ms: event.mono_ms,
                timestamp: event.ts.as_deref(),
                task_id: payload.task_id.as_str(),
                stale_for_ms: payload.stale_for_ms,
            })
        })
    }

    pub fn edit_events(&self) -> impl Iterator<Item = CanonicalEditEvent<'_>> {
        self.source_events.iter().filter_map(|event| {
            let payload = match &event.payload {
                EventV1::EditProposed(payload) => CanonicalEditPayload::Proposed(payload),
                EventV1::EditApplied(payload) => CanonicalEditPayload::Applied(payload),
                EventV1::EditRejected(payload) => CanonicalEditPayload::Rejected(payload),
                _ => return None,
            };
            Some(CanonicalEditEvent {
                seq: event.seq,
                tool_call_id: event.correlation_id.as_deref(),
                payload,
            })
        })
    }

    fn project_operational_lossy(
        events: &[EventEnvelopeV1],
    ) -> Result<(RunSummary, ResumePlan, TimelineIndex), ProjectionError> {
        let fallback_run_id = events
            .first()
            .map_or("unknown", |event| event.run_id.as_str());
        let resume_plan = project_resume_plan(events, fallback_run_id).unwrap_or_else(|error| {
            ResumePlan::blocked(
                fallback_run_id.to_string(),
                format!("event log cannot resume: {error}"),
            )
        });
        Ok((
            project_run_summary(events)?,
            resume_plan,
            project_timeline_index(events)?,
        ))
    }

    fn project_operational_strict(
        events: &[EventEnvelopeV1],
    ) -> Result<(RunSummary, ResumePlan, TimelineIndex), ProjectionError> {
        let fallback_run_id = events
            .first()
            .map_or("unknown", |event| event.run_id.as_str());
        let resume_plan = project_resume_plan(events, fallback_run_id)?;
        Ok((
            project_run_summary(events)?,
            resume_plan,
            project_timeline_index(events)?,
        ))
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum CanonicalSessionProjectionError {
    #[error(transparent)]
    Legacy(#[from] LegacyAdapterError),
    #[error(transparent)]
    Conversation(#[from] ConversationProjectionError),
    #[error(transparent)]
    Transcript(#[from] TranscriptProjectionError),
    #[error(transparent)]
    Operational(#[from] ProjectionError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanonicalProjectionUpdate {
    Buffer,
    Settle,
}

pub const fn canonical_projection_update_for_event(event: &EventV1) -> CanonicalProjectionUpdate {
    if let Some(update) = legacy_projection_update_for_event(event) {
        return update;
    }
    match event {
        EventV1::RunStarted(_)
        | EventV1::TaskScheduled(_)
        | EventV1::UserMessageSubmitted(_)
        | EventV1::PromptAttachmentsSubmitted(_)
        | EventV1::ProviderRequestStarted(_)
        | EventV1::ToolCallRequested(_)
        | EventV1::ToolCallStarted(_) => CanonicalProjectionUpdate::Buffer,
        _ => CanonicalProjectionUpdate::Settle,
    }
}
