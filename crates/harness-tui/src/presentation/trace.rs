use std::fmt;

use serde::{Deserialize, Serialize};

use super::{
    CauseId, PresentationCause, PresentationFrame, PresentationOutcome, PresentationRevision,
    PresentationTimestamp, RenderDemand,
};
use crate::terminal::{FrameAck, FrameAckOutcome, FrameKind};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresentationAggregates {
    pub coalesced_requests: u64,
    pub queue_saturation: u64,
    pub resyncs: u64,
    pub full_repaints: u64,
    pub bytes_written: u64,
    pub idle_redraws: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresentationTrace {
    pub trace_id: String,
    pub causes: Vec<PresentationCause>,
    pub demands: Vec<RenderDemand>,
    pub frames: Vec<PresentationFrame>,
    pub acknowledgements: Vec<FrameAck>,
    pub outcomes: Vec<PresentationOutcome>,
    pub aggregates: PresentationAggregates,
}

impl PresentationTrace {
    pub fn new(trace_id: impl Into<String>) -> Self {
        Self {
            trace_id: trace_id.into(),
            causes: Vec::new(),
            demands: Vec::new(),
            frames: Vec::new(),
            acknowledgements: Vec::new(),
            outcomes: Vec::new(),
            aggregates: PresentationAggregates::default(),
        }
    }

    pub fn record_cause(&mut self, cause: PresentationCause) -> Result<(), PresentationTraceError> {
        if self
            .causes
            .iter()
            .any(|existing| existing.cause_id == cause.cause_id)
        {
            return Err(PresentationTraceError::DuplicateCause(cause.cause_id));
        }
        self.causes.push(cause);
        Ok(())
    }

    pub fn record_demand(&mut self, demand: RenderDemand) {
        self.aggregates.coalesced_requests = self
            .aggregates
            .coalesced_requests
            .saturating_add(demand.coalesced_request_count);
        for cause_id in &demand.cause_ids {
            if let Some(cause) = self
                .causes
                .iter_mut()
                .find(|cause| &cause.cause_id == cause_id)
            {
                cause.resulting_revision = Some(demand.target_revision);
                cause.outcome = PresentationOutcome::VisibleChange {
                    cause_id: cause_id.clone(),
                    revision: demand.target_revision,
                };
            }
        }
        self.demands.push(demand);
    }

    pub fn record_no_visible_change(
        &mut self,
        cause_id: CauseId,
        closed_at: PresentationTimestamp,
    ) -> Result<(), PresentationTraceError> {
        let cause = self
            .causes
            .iter_mut()
            .find(|cause| cause.cause_id == cause_id)
            .ok_or_else(|| PresentationTraceError::UnknownCause(cause_id.clone()))?;
        let outcome = PresentationOutcome::NoVisibleChange {
            cause_id,
            closed_at,
        };
        cause.outcome = outcome.clone();
        self.outcomes.push(outcome);
        Ok(())
    }

    pub fn record_resync_required(
        &mut self,
        rejected_revision: PresentationRevision,
        replacement_revision: PresentationRevision,
        recorded_at: PresentationTimestamp,
    ) -> Result<(), PresentationTraceError> {
        if replacement_revision <= rejected_revision {
            return Err(PresentationTraceError::InvalidResyncReplacement);
        }
        self.aggregates.resyncs = self.aggregates.resyncs.saturating_add(1);
        self.outcomes.push(PresentationOutcome::ResyncRequired {
            rejected_revision,
            replacement_revision,
            recorded_at,
        });
        Ok(())
    }

    pub fn record_acknowledgement(&mut self, ack: FrameAck) {
        if matches!(ack.outcome, FrameAckOutcome::Success) {
            self.aggregates.bytes_written = self
                .aggregates
                .bytes_written
                .saturating_add(u64::try_from(ack.byte_count).unwrap_or(u64::MAX));
        }
        if matches!(ack.frame_kind, FrameKind::FullRepaint) {
            self.aggregates.full_repaints = self.aggregates.full_repaints.saturating_add(1);
        }
        self.frames.push(ack.clone().into());
        self.acknowledgements.push(ack);
    }

    pub fn outcomes(&self) -> &[PresentationOutcome] {
        &self.outcomes
    }

    pub fn frames(&self) -> &[PresentationFrame] {
        &self.frames
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PresentationTraceError {
    DuplicateCause(CauseId),
    UnknownCause(CauseId),
    InvalidResyncReplacement,
}

impl fmt::Display for PresentationTraceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateCause(cause_id) => {
                write!(formatter, "duplicate cause {}", cause_id.as_str())
            }
            Self::UnknownCause(cause_id) => {
                write!(formatter, "unknown cause {}", cause_id.as_str())
            }
            Self::InvalidResyncReplacement => {
                formatter.write_str("resync replacement must advance revision")
            }
        }
    }
}

impl std::error::Error for PresentationTraceError {}
