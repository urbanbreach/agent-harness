use serde::{Deserialize, Serialize};

use crate::terminal::{FrameAckOutcome, FrameKind};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CauseId(String);

impl CauseId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InteractionId(String);

impl InteractionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PresentationRevision(u64);

impl PresentationRevision {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }

    pub const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PresentationTimestamp(u64);

impl PresentationTimestamp {
    pub const fn from_micros(value: u64) -> Self {
        Self(value)
    }

    pub const fn as_micros(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresentationCauseKind {
    Startup,
    TerminalInput,
    LiveUpdate,
    AnimationTimer,
    Wheel,
    Resize,
    Focus,
    Expiry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderReason {
    Startup,
    TerminalInput,
    LiveUpdate,
    Animation,
    Wheel,
    Resize,
    Focus,
    Expiry,
    Resync,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PresentationOutcome {
    Pending,
    VisibleChange {
        cause_id: CauseId,
        revision: PresentationRevision,
    },
    NoVisibleChange {
        cause_id: CauseId,
        closed_at: PresentationTimestamp,
    },
    ResyncRequired {
        rejected_revision: PresentationRevision,
        replacement_revision: PresentationRevision,
        recorded_at: PresentationTimestamp,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresentationCause {
    pub cause_id: CauseId,
    pub interaction_id: Option<InteractionId>,
    pub received_at: PresentationTimestamp,
    pub kind: PresentationCauseKind,
    pub resulting_revision: Option<PresentationRevision>,
    pub outcome: PresentationOutcome,
}

impl PresentationCause {
    pub fn new(
        cause_id: CauseId,
        kind: PresentationCauseKind,
        received_at: PresentationTimestamp,
        interaction_id: Option<InteractionId>,
    ) -> Self {
        Self {
            cause_id,
            interaction_id,
            received_at,
            kind,
            resulting_revision: None,
            outcome: PresentationOutcome::Pending,
        }
    }

    pub const fn id(&self) -> &CauseId {
        &self.cause_id
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderDemand {
    pub target_revision: PresentationRevision,
    pub earliest_requested_at: PresentationTimestamp,
    pub latest_requested_at: PresentationTimestamp,
    pub cause_ids: Vec<CauseId>,
    pub reason: RenderReason,
    pub coalesced_request_count: u64,
}

impl RenderDemand {
    pub fn new(
        target_revision: PresentationRevision,
        cause_id: CauseId,
        requested_at: PresentationTimestamp,
        reason: RenderReason,
    ) -> Self {
        Self {
            target_revision,
            earliest_requested_at: requested_at,
            latest_requested_at: requested_at,
            cause_ids: vec![cause_id],
            reason,
            coalesced_request_count: 0,
        }
    }

    pub fn coalesce(
        &mut self,
        target_revision: PresentationRevision,
        cause_id: CauseId,
        requested_at: PresentationTimestamp,
        reason: RenderReason,
    ) {
        self.target_revision = self.target_revision.max(target_revision);
        self.latest_requested_at = self.latest_requested_at.max(requested_at);
        if !self.cause_ids.contains(&cause_id) {
            self.cause_ids.push(cause_id);
        }
        self.reason = reason;
        self.coalesced_request_count = self.coalesced_request_count.saturating_add(1);
    }

    pub fn merge(&mut self, demand: Self) {
        self.target_revision = self.target_revision.max(demand.target_revision);
        self.latest_requested_at = self.latest_requested_at.max(demand.latest_requested_at);
        for cause_id in demand.cause_ids {
            if !self.cause_ids.contains(&cause_id) {
                self.cause_ids.push(cause_id);
            }
        }
        self.reason = demand.reason;
        self.coalesced_request_count = self
            .coalesced_request_count
            .saturating_add(demand.coalesced_request_count)
            .saturating_add(1);
    }

    pub(crate) fn startup(requested_at: PresentationTimestamp) -> Self {
        Self::new(
            PresentationRevision::default(),
            CauseId::new("runtime:cause:0"),
            requested_at,
            RenderReason::Startup,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresentationFrame {
    pub sequence: u64,
    pub revision: PresentationRevision,
    pub cause_ids: Vec<CauseId>,
    pub requested_at: PresentationTimestamp,
    pub render_started_at: PresentationTimestamp,
    pub render_ended_at: PresentationTimestamp,
    pub submitted_at: PresentationTimestamp,
    pub write_started_at: PresentationTimestamp,
    pub write_ended_at: PresentationTimestamp,
    pub acknowledged_at: PresentationTimestamp,
    pub frame_kind: FrameKind,
    pub byte_count: usize,
    pub byte_sha256: String,
    pub acknowledgement: FrameAckOutcome,
}
