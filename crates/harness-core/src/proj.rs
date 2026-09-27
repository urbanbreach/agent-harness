mod background;
mod catalog;
mod in_flight;
pub use background::*;
pub(crate) use in_flight::InFlight;
mod metadata;
mod resume;
#[cfg(test)]
mod tests;
use crate::event::*;
pub use catalog::*;
pub use metadata::*;
pub use resume::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    #[default]
    Running,
    Finished,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunCounts {
    pub total_events: u64,
    pub by_type: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSummary {
    pub status: RunStatus,
    pub counts: RunCounts,
    pub last_error: Option<String>,
    pub tasks_in_flight: BTreeSet<String>,
    pub pending_permissions: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid history: {0}")]
pub struct ProjectionError(pub String);

impl RunSummary {
    pub(crate) fn apply(&mut self, event: &EventEnvelopeV1) {
        self.counts.total_events += 1;
        *self
            .counts
            .by_type
            .entry(event.payload.event_type().into())
            .or_default() += 1;
        match &event.payload {
            EventV1::RunStarted(_) => {
                self.status = RunStatus::Running;
                self.last_error = None;
            }
            EventV1::RunFinished(_) => self.status = RunStatus::Finished,
            EventV1::RunFailed(data) => {
                self.status = RunStatus::Failed;
                self.last_error = Some(data.error.clone());
            }
            EventV1::TaskScheduled(data) => {
                self.tasks_in_flight.insert(data.task_id.to_string());
            }
            EventV1::TaskCancelled(data) => {
                self.tasks_in_flight.remove(data.task_id.as_str());
            }
            EventV1::TaskCompleted(data) => {
                self.tasks_in_flight.remove(data.task_id.as_str());
            }
            EventV1::PermissionRequested(data) => {
                self.pending_permissions.insert(data.permission_id.clone());
            }
            EventV1::PermissionResolved(data) => {
                self.pending_permissions.remove(&data.permission_id);
            }
            _ => {}
        }
    }
}

pub fn project_run_summary<'a>(
    events: impl IntoIterator<Item = &'a EventEnvelopeV1>,
) -> Result<RunSummary, ProjectionError> {
    let mut summary = RunSummary::default();
    // Inline child sessions pass an ordered display slice with original sequences.
    for event in validate_history(events, false)? {
        summary.apply(event);
    }
    Ok(summary)
}

pub(crate) fn checked_history<'a>(
    events: impl IntoIterator<Item = &'a EventEnvelopeV1>,
) -> Result<Vec<&'a EventEnvelopeV1>, ProjectionError> {
    validate_history(events, true)
}

fn validate_history<'a>(
    events: impl IntoIterator<Item = &'a EventEnvelopeV1>,
    complete: bool,
) -> Result<Vec<&'a EventEnvelopeV1>, ProjectionError> {
    let events: Vec<_> = events.into_iter().collect();
    let run = events.first().map(|e| &e.run_id);
    let mut ids = std::collections::HashSet::with_capacity(events.len());
    let mut users = std::collections::HashMap::new();
    let mut previous = 0;
    for (index, event) in events.iter().enumerate() {
        if event.seq <= previous
            || complete && event.seq != u64::try_from(index).unwrap_or(u64::MAX).saturating_add(1)
            || event.schema_version != SCHEMA_VERSION
            || event.run_id.as_str().is_empty()
            || Some(&event.run_id) != run
            || event.event_id.is_empty()
            || !ids.insert(&event.event_id)
        {
            return Err(ProjectionError(
                "invalid event sequence, schema, or identity".into(),
            ));
        }
        previous = event.seq;
        if !complete {
            continue;
        }
        if let EventV1::UserMessageSubmitted(user) = &event.payload {
            users.insert(user.request_id.as_str(), event.seq);
        }
        if let EventV1::SessionCompaction(compaction) = &event.payload {
            let boundary = compaction.first_kept_event_seq;
            let invalid_turn = compaction
                .first_kept_request_id
                .as_deref()
                .is_some_and(|id| users.get(id).is_none_or(|seq| *seq > boundary));
            if boundary == 0 || boundary >= event.seq || invalid_turn {
                return Err(ProjectionError(
                    "compaction retention boundary is missing from earlier history".into(),
                ));
            }
        }
        if let EventV1::ConversationRewound(rewind) = &event.payload {
            let target = rewind
                .target_seq
                .checked_sub(1)
                .and_then(|seq| usize::try_from(seq).ok())
                .and_then(|i| events.get(i));
            if !target.is_some_and(|target| target.seq < event.seq && matches!(&target.payload, EventV1::UserMessageSubmitted(user) if user.request_id.as_str() == rewind.request_id)) {
                return Err(ProjectionError("invalid conversation rewind boundary".into()));
            }
        }
    }
    Ok(events)
}
