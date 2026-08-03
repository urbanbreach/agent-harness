use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::error::RunnerError;
use super::types::CleanupReceipt;
use super::util::write_json_new;

static ATTEMPT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Default)]
pub(super) struct CleanupTracker {
    forced_termination: bool,
    surviving_pids: BTreeSet<u32>,
    removed_paths: Vec<String>,
    errors: Vec<String>,
}

impl CleanupTracker {
    pub fn record_process(&mut self, cleanup: ProcessCleanup) {
        self.forced_termination |= cleanup.forced_termination;
        self.surviving_pids.extend(cleanup.surviving_pids);
    }

    pub fn record_removed(&mut self, path: &Path) {
        self.removed_paths.push(path.display().to_string());
    }

    pub fn record_error(&mut self, detail: String) {
        self.errors.push(detail);
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    pub fn receipt(&self, primary: Option<&RunnerError>) -> CleanupReceipt {
        CleanupReceipt {
            schema_version: "harness.tui-fidelity.cleanup.v2".to_owned(),
            status: if self.has_errors() {
                "cleanup_error"
            } else if primary.is_some() {
                "error"
            } else {
                "clean"
            }
            .to_owned(),
            forced_termination_observed: self.forced_termination,
            surviving_pids: self.surviving_pids.iter().copied().collect(),
            temporary_paths_removed: self.removed_paths.clone(),
            cleanup_errors: self.errors.clone(),
            primary_error: primary.map(ToString::to_string),
        }
    }

    pub fn error_detail(&self) -> String {
        self.errors.join("; ")
    }
}

#[derive(Default)]
pub(super) struct ProcessCleanup {
    pub forced_termination: bool,
    pub surviving_pids: Vec<u32>,
}

pub(super) struct EvidenceSession {
    directory: PathBuf,
    cleanup_path: PathBuf,
    stale: bool,
}

impl EvidenceSession {
    pub fn initialize(directory: &Path) -> Result<Self, RunnerError> {
        let stale = directory.exists()
            && fs::read_dir(directory)
                .map_err(|error| io_error(directory, error))?
                .next()
                .is_some();
        fs::create_dir_all(directory).map_err(|error| io_error(directory, error))?;
        let cleanup_path = if stale {
            directory.join(format!(
                "cleanup-attempt-{}-{}.json",
                std::process::id(),
                ATTEMPT_ID.fetch_add(1, Ordering::Relaxed)
            ))
        } else {
            directory.join("cleanup.json")
        };
        Ok(Self {
            directory: directory.to_path_buf(),
            cleanup_path,
            stale,
        })
    }

    pub const fn is_stale(&self) -> bool {
        self.stale
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn write(&self, receipt: &CleanupReceipt) -> Result<(), RunnerError> {
        write_json_new(&self.cleanup_path, receipt)
    }
}

pub fn record_preflight_failure(
    evidence_dir: &Path,
    primary: &RunnerError,
) -> Result<(), RunnerError> {
    let evidence = EvidenceSession::initialize(evidence_dir)?;
    evidence.write(&CleanupTracker::default().receipt(Some(primary)))
}

fn io_error(path: &Path, error: impl std::fmt::Display) -> RunnerError {
    RunnerError::Io {
        path: path.to_path_buf(),
        detail: error.to_string(),
    }
}
