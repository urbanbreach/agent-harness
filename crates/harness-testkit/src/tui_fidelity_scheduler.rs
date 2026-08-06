use std::collections::VecDeque;
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::tui_fidelity_obligation::VerificationKey;
use crate::tui_fidelity_staging::{JobIsolation, KeyState, StagingArea, StagingError};

const HARD_MAX_WORKERS: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SchedulerReport {
    pub passed: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub skipped: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchedulerError {
    Invalid(String),
    Staging(String),
    Worker(String),
}

impl fmt::Display for SchedulerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(detail) => write!(formatter, "scheduler config: {detail}"),
            Self::Staging(detail) => write!(formatter, "scheduler staging: {detail}"),
            Self::Worker(detail) => write!(formatter, "scheduler worker: {detail}"),
        }
    }
}

impl std::error::Error for SchedulerError {}

impl From<StagingError> for SchedulerError {
    fn from(error: StagingError) -> Self {
        Self::Staging(error.to_string())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundedScheduler {
    workers: usize,
}

impl BoundedScheduler {
    pub fn with_default_workers() -> Self {
        let logical = logical_cpus();
        let formula = (logical / 2).clamp(1, 8);
        Self {
            workers: formula.min(reserved_cpu_cap(logical)),
        }
    }

    pub fn new(workers: usize) -> Result<Self, SchedulerError> {
        if workers == 0 || workers > HARD_MAX_WORKERS {
            return Err(SchedulerError::Invalid(format!(
                "workers must be between 1 and {HARD_MAX_WORKERS}"
            )));
        }
        Ok(Self {
            workers: workers.min(reserved_cpu_cap(logical_cpus())),
        })
    }

    pub const fn workers(self) -> usize {
        self.workers
    }

    pub fn run<F>(
        self,
        keys: &[VerificationKey],
        staging: &StagingArea,
        execute: F,
    ) -> Result<SchedulerReport, SchedulerError>
    where
        F: Fn(&VerificationKey, &JobIsolation) -> Result<PathBuf, String> + Sync,
    {
        let mut pending = VecDeque::new();
        let mut skipped = 0;
        for key in keys {
            match staging.state(key)? {
                KeyState::Pending => pending.push_back(key.clone()),
                KeyState::Passed => skipped += 1,
                KeyState::Running | KeyState::Failed | KeyState::Cancelled => {}
            }
        }
        let queue = Arc::new(Mutex::new(pending));
        let failed = Arc::new(AtomicBool::new(false));
        let results = std::thread::scope(|scope| {
            let mut handles = Vec::with_capacity(self.workers);
            for _ in 0..self.workers {
                let queue = Arc::clone(&queue);
                let failed = Arc::clone(&failed);
                let execute = &execute;
                handles.push(scope.spawn(move || {
                    let mut worker_results = Vec::new();
                    loop {
                        if failed.load(Ordering::SeqCst) {
                            break;
                        }
                        let key = queue
                            .lock()
                            .map_err(|error| SchedulerError::Worker(error.to_string()))?
                            .pop_front();
                        let Some(key) = key else { break };
                        staging.mark_running(&key)?;
                        let isolation = staging.isolation(&key)?;
                        match execute(&key, &isolation) {
                            Ok(artifact) => {
                                staging.mark_passed(&key, &artifact)?;
                                worker_results.push(true);
                            }
                            Err(detail) => {
                                staging.mark_failed(&key, &detail)?;
                                failed.store(true, Ordering::SeqCst);
                                worker_results.push(false);
                            }
                        }
                    }
                    Ok::<Vec<bool>, SchedulerError>(worker_results)
                }));
            }
            handles
                .into_iter()
                .map(|handle| {
                    handle
                        .join()
                        .map_err(|_| SchedulerError::Worker("worker panicked".to_owned()))?
                })
                .collect::<Result<Vec<_>, _>>()
        })?;
        let mut passed = 0;
        let mut failures = 0;
        for result in results.into_iter().flatten() {
            if result {
                passed += 1;
            } else {
                failures += 1;
            }
        }
        let mut cancelled = 0;
        if failed.load(Ordering::SeqCst) {
            for key in keys {
                if staging.state(key)? == KeyState::Pending {
                    staging.mark_cancelled(key)?;
                    cancelled += 1;
                }
            }
        }
        Ok(SchedulerReport {
            passed,
            failed: failures,
            cancelled,
            skipped,
        })
    }
}

fn logical_cpus() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
}

fn reserved_cpu_cap(logical: usize) -> usize {
    logical.saturating_sub(1).clamp(1, HARD_MAX_WORKERS)
}
