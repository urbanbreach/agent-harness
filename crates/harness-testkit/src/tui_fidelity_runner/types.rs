use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::error::RunnerError;
use super::util::sha256_file;
use crate::tui_fidelity::{AdapterKind, CheckpointName, Viewport};
use crate::tui_fidelity_compare::ComparisonReceipt;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeBinary {
    pub path: PathBuf,
    pub source_revision: String,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateBinding {
    pub candidate_sha: String,
    pub candidate_binary_sha256: String,
    pub runner_sha256: String,
    pub target_dir: PathBuf,
    pub freshness_relation: String,
}

impl RuntimeBinary {
    pub fn from_path(path: &Path, source_revision: &str) -> Result<Self, RunnerError> {
        Ok(Self {
            path: path.to_path_buf(),
            source_revision: source_revision.to_owned(),
            sha256: sha256_file(path)?,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceGuardConfig {
    pub program: PathBuf,
    pub reference_root: PathBuf,
    pub revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RendererConfig {
    pub node_program: PathBuf,
    pub script: PathBuf,
    pub browser_program: PathBuf,
    pub font_family: String,
    pub node_modules: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunnerTiming {
    pub tick: Duration,
    pub scenario_timeout: Duration,
    pub normal_exit_timeout: Duration,
    pub cleanup_timeout: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunnerConfig {
    pub repo_root: PathBuf,
    pub evidence_dir: PathBuf,
    pub reference: RuntimeBinary,
    pub harness: RuntimeBinary,
    pub candidate_binding: CandidateBinding,
    pub source_guard: SourceGuardConfig,
    pub renderer: RendererConfig,
    pub timing: RunnerTiming,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactDigest {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct BrowserCapabilities {
    pub unicode_version: String,
    pub device_pixel_ratio: f64,
    pub browser: String,
    pub font_loaded: bool,
    pub color: String,
    pub graphics: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CheckpointReceipt {
    pub name: CheckpointName,
    pub viewport: Viewport,
    pub captured_at_millis: u128,
    pub capabilities: BrowserCapabilities,
    pub artifacts: Vec<ArtifactDigest>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdapterReceipt {
    pub adapter: AdapterKind,
    pub binary: RuntimeBinary,
    pub normal_exit_code: i32,
    pub input_timestamps_millis: Vec<u128>,
    pub checkpoints: Vec<CheckpointReceipt>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DualRuntimeReceipt {
    pub schema_version: String,
    pub scenario_id: String,
    pub terminal_type: String,
    pub runtimes: Vec<AdapterReceipt>,
    pub candidate_binding: CandidateBinding,
    pub source_guard_before: ArtifactDigest,
    pub source_guard_after: ArtifactDigest,
    #[serde(default)]
    pub comparison: Option<ComparisonReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupReceipt {
    pub schema_version: String,
    pub status: String,
    pub forced_termination_observed: bool,
    /// Unexpected child PIDs found alive at the cleanup boundary.
    pub detected_child_pids: Vec<u32>,
    /// Child PIDs still alive after termination and the bounded reap wait.
    pub surviving_pids: Vec<u32>,
    pub temporary_paths_removed: Vec<String>,
    pub cleanup_errors: Vec<String>,
    pub primary_error: Option<String>,
}
