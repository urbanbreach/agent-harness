use super::*;
use crate::{
    event::*,
    proj::{ProjectionError, RunSummary},
    transcript_projection::{self, TranscriptIndex, TranscriptProjection},
};
use std::collections::HashSet;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CanonicalSessionProjection {
    source_events: Vec<EventEnvelopeV1>,
    index: TranscriptIndex,
    event_ids: HashSet<String>,
    finished_requests: HashSet<String>,
    pub session: CanonicalSession,
    pub transcript: TranscriptProjection,
    pub run_summary: RunSummary,
    pub compatibility_warnings: Vec<legacy::LegacyWarning>,
}

impl CanonicalSessionProjection {
    pub fn source_events(&self) -> &[EventEnvelopeV1] {
        &self.source_events
    }
    pub fn from_event_history(events: &[EventEnvelopeV1]) -> Result<Self, ProjectionError> {
        let mut projection = Self::default();
        projection.apply_events(events)?;
        Ok(projection)
    }
    pub fn apply_events(&mut self, events: &[EventEnvelopeV1]) -> Result<(), ProjectionError> {
        self.validate(events)?;
        let rewind = events
            .iter()
            .any(|e| matches!(e.payload, EventV1::ConversationRewound(_)));
        for event in events {
            self.run_summary.apply(event);
            self.session.apply(event);
            self.track_warnings(event);
            if !rewind {
                transcript_projection::apply(&mut self.transcript, &mut self.index, event);
            }
            self.event_ids.insert(event.event_id.clone());
            self.source_events.push(event.clone());
        }
        if rewind {
            self.transcript = TranscriptProjection::default();
            self.index = TranscriptIndex::default();
            for event in crate::conversation_rewind::active_events(&self.source_events).iter() {
                transcript_projection::apply(&mut self.transcript, &mut self.index, event);
            }
        }
        Ok(())
    }

    fn validate(&self, events: &[EventEnvelopeV1]) -> Result<(), ProjectionError> {
        let mut sequence = self.source_events.last().map_or(0, |e| e.seq);
        let run = self
            .source_events
            .first()
            .or_else(|| events.first())
            .map(|e| &e.run_id);
        let mut seen = HashSet::with_capacity(events.len());
        for event in events {
            sequence = sequence
                .checked_add(1)
                .ok_or_else(|| ProjectionError("sequence overflow".into()))?;
            if event.seq != sequence {
                return Err(ProjectionError(format!(
                    "expected sequence {sequence}, found {}",
                    event.seq
                )));
            }
            if event.schema_version != SCHEMA_VERSION
                || Some(&event.run_id) != run
                || event.run_id.as_str().is_empty()
            {
                return Err(ProjectionError(
                    "unsupported schema or mixed run identity".into(),
                ));
            }
            if event.event_id.is_empty()
                || self.event_ids.contains(&event.event_id)
                || !seen.insert(&event.event_id)
            {
                return Err(ProjectionError(format!(
                    "duplicate or empty event identity: {}",
                    event.event_id
                )));
            }
            if let EventV1::ConversationRewound(rewind) = &event.payload {
                if rewind.target_seq >= event.seq {
                    return Err(ProjectionError(
                        "rewind target must precede its event".into(),
                    ));
                }
                let target = self
                    .source_events
                    .iter()
                    .chain(events)
                    .find(|e| e.seq == rewind.target_seq);
                if !target.is_some_and(|e| matches!(&e.payload, EventV1::UserMessageSubmitted(user) if user.request_id.as_str() == rewind.request_id)) {
                    return Err(ProjectionError("rewind target is not the selected user message".into()));
                }
            }
        }
        Ok(())
    }

    fn track_warnings(&mut self, event: &EventEnvelopeV1) {
        use legacy::LegacyWarning as Warning;
        match &event.payload {
            EventV1::ProviderRequestFinished(data) => {
                self.finished_requests.insert(data.request_id.to_string());
                self.compatibility_warnings.retain(|w| !matches!(w, Warning::MissingProviderFinish { request_id } if request_id == data.request_id.as_str()));
            }
            EventV1::AssistantMessageFinished(data)
                if !self.finished_requests.contains(data.request_id.as_str()) =>
            {
                self.compatibility_warnings
                    .push(Warning::MissingProviderFinish {
                        request_id: data.request_id.to_string(),
                    });
            }
            EventV1::CompactionRequested(_)
            | EventV1::CompactionWritten(_)
            | EventV1::CompactionApplied(_)
            | EventV1::CompactionFailed(_) => {
                self.compatibility_warnings
                    .push(Warning::UnsupportedLegacyVariant {
                        event_id: event.event_id.clone(),
                    });
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanonicalProjectionUpdate {
    Buffer,
    Settle,
}
pub const fn canonical_projection_update_for_event(event: &EventV1) -> CanonicalProjectionUpdate {
    match event {
        EventV1::RunStarted(_)
        | EventV1::TaskScheduled(_)
        | EventV1::UserMessageSubmitted(_)
        | EventV1::PromptAttachmentsSubmitted(_)
        | EventV1::ProviderRequestStarted(_)
        | EventV1::ToolCallRequested(_)
        | EventV1::ToolCallStarted(_)
        | EventV1::CompactionRequested(_) => CanonicalProjectionUpdate::Buffer,
        _ => CanonicalProjectionUpdate::Settle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_batches_are_atomic_and_incremental_replay_matches_a_fresh_projection(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut events = Vec::new();
        for seq in 1..=3 {
            events.push(serde_json::from_value::<EventEnvelopeV1>(serde_json::json!({
                "schema_version":1,"event_id":format!("e-{seq}"),"seq":seq,"run_id":"run","mono_ms":seq,
                "actor":{"kind":"user"},"payload":{"event_type":"user_message_submitted","data":{"request_id":format!("q-{seq}"),"text":format!("question {seq}")}}
            }))?);
        }
        let mut projection = CanonicalSessionProjection::from_event_history(&events[..1])?;
        assert_eq!(projection.session.entries().len(), 1);
        projection.apply_events(&events[1..])?;
        assert_eq!(
            projection,
            CanonicalSessionProjection::from_event_history(&events)?
        );
        let before = projection.clone();
        let mut bad = events[0].clone();
        bad.seq = 4;
        assert!(projection.apply_events(&[bad]).is_err());
        assert_eq!(projection, before);
        let rewind = EventEnvelopeV1 {
            seq: 4,
            event_id: "rewind".into(),
            payload: EventV1::ConversationRewound(
                crate::conversation_rewind::ConversationRewoundEvent {
                    target_seq: 2,
                    request_id: "q-2".into(),
                },
            ),
            ..events[0].clone()
        };
        events.push(rewind.clone());
        projection.apply_events(&[rewind])?;
        assert_eq!(projection.transcript.messages.len(), 1);
        assert_eq!(projection.source_events().len(), 4);
        assert_eq!(
            projection,
            CanonicalSessionProjection::from_event_history(&events)?
        );
        Ok(())
    }
}
