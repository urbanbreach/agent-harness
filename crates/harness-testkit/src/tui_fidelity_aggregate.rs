use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::tui_fidelity_compare::AcceptanceProfile;

mod gates;
mod helpers;
mod input_visibility;
mod packet2_contract;
mod run_reader;
mod scheduling;
mod types;
use helpers::summarize;
use types::Run;

const RUN_COUNT: usize = 5;

#[derive(Debug, thiserror::Error)]
pub enum AggregateError {
    #[error("exactly five run roots are required, got {0}")]
    RunCount(usize),
    #[error("{path}: {detail}")]
    Evidence { path: PathBuf, detail: String },
    #[error("run authority differs at {0}")]
    MixedAuthority(PathBuf),
    #[error("aggregate threshold failed: {0}")]
    Threshold(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AggregateSummary {
    pub schema_version: &'static str,
    pub run_count: usize,
    pub authority: Authority,
    pub reference_external_p95_micros: u64,
    pub candidate_external_p95_micros: u64,
    pub candidate_native_receive_to_flush_p95_micros: u64,
    pub candidate_interval_p95_micros: u64,
    pub candidate_interval_max_micros: u64,
    pub coalesced_requests: u64,
    pub queue_saturation: u64,
    pub resyncs: u64,
    pub full_repaints: u64,
    pub idle_redraws: u64,
    pub artifact_sha256: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Authority {
    pub(super) scenario_id: String,
    pub(super) receipt_schema: String,
    pub(super) comparison_schema: String,
    pub(super) reference_sha256: String,
    pub(super) candidate_sha256: String,
    pub(super) action_schedule_sha256: String,
    pub(super) motion_contract_sha256: String,
    pub(super) observer_version: String,
    pub(super) terminal_identity: String,
}

pub fn aggregate(run_roots: &[PathBuf]) -> Result<AggregateSummary, AggregateError> {
    aggregate_with_profile(run_roots, AcceptanceProfile::FullParity)
}

pub fn aggregate_with_profile(
    run_roots: &[PathBuf],
    profile: AcceptanceProfile,
) -> Result<AggregateSummary, AggregateError> {
    if run_roots.len() != RUN_COUNT {
        return Err(AggregateError::RunCount(run_roots.len()));
    }
    let mut runs = Vec::with_capacity(RUN_COUNT);
    for root in run_roots {
        runs.push(run_reader::read(root, profile)?);
    }
    let authority = runs[0].authority.clone();
    let input_order = runs[0].input_order.clone();
    for run in &runs {
        if run.authority != authority || run.input_order != input_order {
            return Err(AggregateError::MixedAuthority(run.root.clone()));
        }
    }
    summarize(authority, &runs, profile)
}
