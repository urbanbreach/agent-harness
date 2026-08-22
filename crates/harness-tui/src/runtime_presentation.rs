use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::presentation::{
    CauseId, InteractionId, PresentationCause, PresentationCauseKind, PresentationClock,
    PresentationRevision, PresentationTrace, RenderDemand, RenderReason,
};
use crate::terminal::FrameAck;

mod interaction_queue;

pub use interaction_queue::InteractionEventClass;
use interaction_queue::InteractionQueue;

pub const PRESENTATION_TRACE_ENV: &str = "HARNESS_TUI_PRESENTATION_TRACE";
pub const INTERACTION_QUEUE_ENV: &str = "HARNESS_TUI_INTERACTION_QUEUE";

#[derive(Debug)]
pub struct PresentationTelemetrySession {
    path: PathBuf,
    clock: PresentationClock,
    trace: PresentationTrace,
    next_cause_sequence: u64,
    revision: PresentationRevision,
    pending_demand: Option<RenderDemand>,
    interaction_queue: Option<InteractionQueue>,
}

impl PresentationTelemetrySession {
    pub fn from_env() -> io::Result<Option<Self>> {
        Self::from_paths(
            std::env::var_os(PRESENTATION_TRACE_ENV).map(PathBuf::from),
            std::env::var_os(INTERACTION_QUEUE_ENV).map(PathBuf::from),
        )
    }

    pub fn from_trace_path(path: Option<PathBuf>) -> io::Result<Option<Self>> {
        Self::from_paths(path, None)
    }

    pub fn from_paths(
        trace_path: Option<PathBuf>,
        interaction_queue_path: Option<PathBuf>,
    ) -> io::Result<Option<Self>> {
        trace_path
            .map(|path| Self::new(path, interaction_queue_path))
            .transpose()
    }

    fn new(path: PathBuf, interaction_queue_path: Option<PathBuf>) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let trace_id = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("harness-presentation")
            .to_owned();
        Ok(Self {
            path,
            clock: PresentationClock::new(),
            trace: PresentationTrace::new(trace_id),
            next_cause_sequence: 1,
            revision: PresentationRevision::default(),
            pending_demand: None,
            interaction_queue: interaction_queue_path.map(InteractionQueue::new),
        })
    }

    pub fn clock(&self) -> PresentationClock {
        self.clock.clone()
    }

    pub fn take_interaction_id(
        &mut self,
        event_class: InteractionEventClass,
    ) -> io::Result<Option<InteractionId>> {
        match self.interaction_queue.as_mut() {
            Some(queue) => queue.take(event_class),
            None => Ok(None),
        }
    }

    pub const fn trace(&self) -> &PresentationTrace {
        &self.trace
    }

    pub fn record_visible_cause(
        &mut self,
        kind: PresentationCauseKind,
        reason: RenderReason,
        interaction_id: Option<InteractionId>,
    ) -> CauseId {
        let cause_id = CauseId::new(format!(
            "{}:cause:{}",
            self.trace.trace_id, self.next_cause_sequence
        ));
        self.next_cause_sequence = self.next_cause_sequence.saturating_add(1);
        let requested_at = self.clock.now();
        let cause = PresentationCause::new(cause_id.clone(), kind, requested_at, interaction_id);
        let _ = self.trace.record_cause(cause);
        self.revision = self.revision.next();
        let demand = RenderDemand::new(self.revision, cause_id.clone(), requested_at, reason);
        match self.pending_demand.as_mut() {
            Some(pending) => pending.merge(demand),
            None => self.pending_demand = Some(demand),
        }
        cause_id
    }

    pub fn record_no_visible_cause(
        &mut self,
        kind: PresentationCauseKind,
        interaction_id: Option<InteractionId>,
    ) -> io::Result<CauseId> {
        let cause_id = CauseId::new(format!(
            "{}:cause:{}",
            self.trace.trace_id, self.next_cause_sequence
        ));
        self.next_cause_sequence = self.next_cause_sequence.saturating_add(1);
        let received_at = self.clock.now();
        self.trace
            .record_cause(PresentationCause::new(
                cause_id.clone(),
                kind,
                received_at,
                interaction_id,
            ))
            .map_err(io::Error::other)?;
        self.trace
            .record_no_visible_change(cause_id.clone(), self.clock.now())
            .map_err(io::Error::other)?;
        Ok(cause_id)
    }

    pub fn take_render_demand(&mut self) -> Option<RenderDemand> {
        let demand = self.pending_demand.take()?;
        self.trace.record_demand(demand.clone());
        Some(demand)
    }

    pub fn record_no_visible_change(&mut self, demand: &RenderDemand) -> io::Result<()> {
        let closed_at = self.clock.now();
        for cause_id in &demand.cause_ids {
            self.trace
                .record_no_visible_change(cause_id.clone(), closed_at)
                .map_err(io::Error::other)?;
        }
        Ok(())
    }

    pub fn record_acknowledgements(&mut self, acknowledgements: Vec<FrameAck>) {
        for acknowledgement in acknowledgements {
            self.trace.record_acknowledgement(acknowledgement);
        }
    }

    pub fn record_resync(&mut self, rejected: &RenderDemand) -> io::Result<()> {
        let replacement_revision = self.revision.next();
        self.trace
            .record_resync_required(
                rejected.target_revision,
                replacement_revision,
                self.clock.now(),
            )
            .map_err(io::Error::other)?;
        self.revision = replacement_revision;
        let Some(first) = rejected.cause_ids.first() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "resync demand has no causes",
            ));
        };
        let requested_at = self.clock.now();
        let mut replacement = RenderDemand::new(
            replacement_revision,
            first.clone(),
            requested_at,
            RenderReason::Resync,
        );
        for cause_id in rejected.cause_ids.iter().skip(1) {
            replacement.coalesce(
                replacement_revision,
                cause_id.clone(),
                requested_at,
                RenderReason::Resync,
            );
        }
        self.pending_demand = Some(replacement);
        Ok(())
    }

    pub fn finish(self) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(&self.trace).map_err(io::Error::other)?;
        let temporary_path = temporary_path(&self.path);
        fs::write(&temporary_path, bytes)?;
        fs::rename(temporary_path, self.path)
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_owned();
    extension.push_str(".tmp");
    path.with_extension(extension)
}
