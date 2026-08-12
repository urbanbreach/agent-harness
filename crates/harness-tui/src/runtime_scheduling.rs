use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Instant;

use serde::Serialize;

use crate::presentation::{CauseId, InteractionId};

pub const SCHEDULING_TRACE_ENV: &str = "TUI_FIDELITY_SCHEDULING_TRACE";
pub const SCHEDULING_READINESS_ENV: &str = "TUI_FIDELITY_SCHEDULING_READINESS";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SchedulingLiveReadiness {
    pub queued_depth: usize,
    pub deferred_ready: bool,
    pub stream_active: bool,
}

impl SchedulingLiveReadiness {
    pub const fn ready_depth(self) -> usize {
        self.queued_depth
            .saturating_add(self.deferred_ready as usize)
    }
}

#[derive(Debug)]
pub struct SchedulingReadinessSignal {
    path: PathBuf,
    epoch: Instant,
    sample_sequence: u64,
    last: Option<SchedulingLiveReadiness>,
}

#[derive(Serialize)]
struct SchedulingReadinessSnapshot {
    schema_version: &'static str,
    sample_sequence: u64,
    sampled_at_micros: u128,
    ready_depth: usize,
    queued_depth: usize,
    deferred_ready: bool,
    stream_active: bool,
}

impl SchedulingReadinessSignal {
    pub fn from_env() -> io::Result<Option<Self>> {
        std::env::var_os(SCHEDULING_READINESS_ENV)
            .map(PathBuf::from)
            .map(Self::new)
            .transpose()
    }

    pub fn new(path: PathBuf) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(Self {
            path,
            epoch: Instant::now(),
            sample_sequence: 0,
            last: None,
        })
    }

    pub fn publish_if_changed(&mut self, live: SchedulingLiveReadiness) -> io::Result<bool> {
        if self.last == Some(live) {
            return Ok(false);
        }
        self.sample_sequence = self.sample_sequence.saturating_add(1);
        let snapshot = SchedulingReadinessSnapshot {
            schema_version: "harness.packet2-scheduling-readiness.v1",
            sample_sequence: self.sample_sequence,
            sampled_at_micros: self.epoch.elapsed().as_micros(),
            ready_depth: live.ready_depth(),
            queued_depth: live.queued_depth,
            deferred_ready: live.deferred_ready,
            stream_active: live.stream_active,
        };
        let bytes = serde_json::to_vec(&snapshot).map_err(io::Error::other)?;
        let temporary = self.path.with_extension("tmp");
        fs::write(&temporary, bytes)?;
        fs::rename(temporary, &self.path)?;
        self.last = Some(live);
        Ok(true)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SchedulingInputSend {
    pub interaction_id: String,
    pub action_ordinal: usize,
    pub terminal_sequence: u64,
    pub source_decision: &'static str,
    pub live_ready_depth: usize,
    pub queued_live_depth: usize,
    pub deferred_live_ready: bool,
    pub stream_active: bool,
    pub preempted_live: bool,
    pub fairness_yield: bool,
    pub deadline_millis: Option<u64>,
    pub cause_id: String,
}

#[derive(Debug, Serialize)]
struct SchedulingTrace {
    schema_version: &'static str,
    actual_input_sends: Vec<SchedulingInputSend>,
    maximum_backlog_depth: usize,
}

#[derive(Debug)]
pub struct SchedulingTelemetrySession {
    path: PathBuf,
    actual_input_sends: Vec<SchedulingInputSend>,
    observed_actions: BTreeSet<String>,
    maximum_backlog_depth: usize,
    next_terminal_sequence: u64,
}

impl SchedulingTelemetrySession {
    pub fn from_env() -> io::Result<Option<Self>> {
        std::env::var_os(SCHEDULING_TRACE_ENV)
            .map(PathBuf::from)
            .map(Self::new)
            .transpose()
    }

    pub fn new(path: PathBuf) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(Self {
            path,
            actual_input_sends: Vec::new(),
            observed_actions: BTreeSet::new(),
            maximum_backlog_depth: 0,
            next_terminal_sequence: 1,
        })
    }

    pub fn record_terminal_ready(
        &mut self,
        interaction_id: Option<&InteractionId>,
        cause_id: &CauseId,
        live: SchedulingLiveReadiness,
        fairness_yield: bool,
        deadline_millis: Option<u64>,
    ) {
        let live_ready_depth = live.ready_depth();
        let Some(interaction_id) = interaction_id else {
            return;
        };
        if !self
            .observed_actions
            .insert(interaction_id.as_str().to_owned())
        {
            return;
        }
        let Some(action_ordinal) = action_ordinal(interaction_id.as_str()) else {
            return;
        };
        self.maximum_backlog_depth = self.maximum_backlog_depth.max(live_ready_depth);
        let terminal_sequence = self.next_terminal_sequence;
        self.next_terminal_sequence = self.next_terminal_sequence.saturating_add(1);
        self.actual_input_sends.push(SchedulingInputSend {
            interaction_id: interaction_id.as_str().to_owned(),
            action_ordinal,
            terminal_sequence,
            source_decision: "terminal_input",
            live_ready_depth,
            queued_live_depth: live.queued_depth,
            deferred_live_ready: live.deferred_ready,
            stream_active: live.stream_active,
            preempted_live: live_ready_depth > 0,
            fairness_yield,
            deadline_millis,
            cause_id: cause_id.as_str().to_owned(),
        });
    }

    pub fn finish(self) -> io::Result<()> {
        let trace = SchedulingTrace {
            schema_version: "harness.packet2-scheduling.v1",
            actual_input_sends: self.actual_input_sends,
            maximum_backlog_depth: self.maximum_backlog_depth,
        };
        let bytes = serde_json::to_vec_pretty(&trace).map_err(io::Error::other)?;
        fs::write(self.path, bytes)
    }
}

fn action_ordinal(interaction_id: &str) -> Option<usize> {
    interaction_id.rsplit(':').next()?.parse().ok()
}
