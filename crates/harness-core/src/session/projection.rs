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
    pub request_id: &'a str,
    pub agent_id: Option<&'a str>,
    pub provider_id: &'a str,
    pub model_id: &'a str,
    pub prompt_summary: &'a str,
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
                request_id: payload.request_id.as_str(),
                agent_id: event.actor.agent_id.as_deref(),
                provider_id: &payload.provider_id,
                model_id: &payload.model_id,
                prompt_summary: &payload.prompt_summary,
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
