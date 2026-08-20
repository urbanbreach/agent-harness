use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::tui_fidelity_matrix::{CoverageManifest, RequirementInventory};
use crate::tui_fidelity_obligation::{
    deduplicate_obligations, CaptureKey, ObligationType, VerificationKey,
};
use crate::tui_fidelity_scheduler::{BoundedScheduler, SchedulerError, SchedulerReport};
use crate::tui_fidelity_staging::{AttemptPolicy, JobIsolation, StagingArea, StagingError};

mod completion;
pub use completion::{validate_active_completion, ActiveCompletion, CompletionBindings};

pub const ALL_DEADLINE: Duration = Duration::from_secs(120);
pub const CHANGED_DEADLINE: Duration = Duration::from_secs(90);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationProfile {
    Changed,
    All,
    Motion,
}

impl VerificationProfile {
    pub const fn deadline(self) -> Duration {
        match self {
            Self::Changed => CHANGED_DEADLINE,
            Self::All | Self::Motion => ALL_DEADLINE,
        }
    }

    pub const fn policy(self) -> AttemptPolicy {
        match self {
            Self::All => AttemptPolicy::FinalAll,
            Self::Changed | Self::Motion => AttemptPolicy::Development,
        }
    }
}

impl fmt::Display for VerificationProfile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Changed => "changed",
            Self::All => "all",
            Self::Motion => "motion",
        })
    }
}

impl std::str::FromStr for VerificationProfile {
    type Err = VerifyError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "changed" => Ok(Self::Changed),
            "all" => Ok(Self::All),
            "motion" => Ok(Self::Motion),
            other => Err(VerifyError::Invalid(format!(
                "unknown verification profile {other}"
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PlanSelection<'a> {
    pub profile: VerificationProfile,
    pub changed: Option<&'a BTreeSet<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerificationPlan {
    pub profile: VerificationProfile,
    pub obligation_count: usize,
    pub keys: Vec<VerificationKey>,
    pub requirements_by_key: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug)]
pub struct VerifyConfig {
    pub candidate_sha: String,
    pub authority_sha256: String,
    pub inventory_sha256: String,
    pub coverage_sha256: String,
    pub attempt_id: String,
    pub evidence_root: PathBuf,
    pub workers: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Passed,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationReceipt {
    pub schema_version: String,
    pub status: VerificationStatus,
    pub profile: VerificationProfile,
    pub candidate_sha: String,
    pub authority_sha256: String,
    pub inventory_sha256: String,
    pub coverage_sha256: String,
    pub obligation_count: usize,
    pub key_count: usize,
    pub duration_millis: u128,
    pub scheduler: SchedulerReport,
    pub evidence_path: String,
    pub sealed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VerifyError {
    Invalid(String),
    Obligation(String),
    Scheduler(String),
    Staging(String),
    Io { path: PathBuf, detail: String },
    Json(String),
}

impl fmt::Display for VerifyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(detail) => write!(formatter, "verification: {detail}"),
            Self::Obligation(detail) => write!(formatter, "verification obligation: {detail}"),
            Self::Scheduler(detail) => write!(formatter, "verification scheduler: {detail}"),
            Self::Staging(detail) => write!(formatter, "verification staging: {detail}"),
            Self::Io { path, detail } => {
                write!(formatter, "verification I/O {}: {detail}", path.display())
            }
            Self::Json(detail) => write!(formatter, "verification JSON: {detail}"),
        }
    }
}

impl std::error::Error for VerifyError {}

impl From<SchedulerError> for VerifyError {
    fn from(error: SchedulerError) -> Self {
        Self::Scheduler(error.to_string())
    }
}

impl From<StagingError> for VerifyError {
    fn from(error: StagingError) -> Self {
        Self::Staging(error.to_string())
    }
}

pub fn build_plan(
    selection: PlanSelection<'_>,
    inventory: &RequirementInventory,
    manifest: &CoverageManifest,
) -> Result<VerificationPlan, VerifyError> {
    let selected = match selection.profile {
        VerificationProfile::All => inventory
            .requirements
            .iter()
            .map(|requirement| requirement.id.clone())
            .collect(),
        VerificationProfile::Changed => selection.changed.cloned().ok_or_else(|| {
            VerifyError::Invalid("changed profile requires a dependency-cone selection".to_owned())
        })?,
        VerificationProfile::Motion => motion_requirements(inventory, manifest),
    };
    let deduplicated = deduplicate_obligations(inventory, manifest, &selected)
        .map_err(|error| VerifyError::Obligation(error.to_string()))?;
    Ok(VerificationPlan {
        profile: selection.profile,
        obligation_count: deduplicated.obligation_count,
        keys: deduplicated.keys,
        requirements_by_key: deduplicated.requirements_by_key,
    })
}

pub fn execute_plan<F>(
    config: &VerifyConfig,
    plan: &VerificationPlan,
    execute: F,
) -> Result<VerificationReceipt, VerifyError>
where
    F: Fn(&VerificationKey, &JobIsolation) -> Result<PathBuf, String> + Sync,
{
    let started = Instant::now();
    let staging = StagingArea::open(
        &config.evidence_root,
        &config.candidate_sha,
        &config.attempt_id,
        &plan.keys,
        plan.profile.policy(),
    )?;
    let scheduler = config.workers.map_or_else(
        || Ok(BoundedScheduler::with_default_workers()),
        BoundedScheduler::new,
    )?;
    let scheduler_report = scheduler.run(&plan.keys, &staging, execute)?;
    let key_count = plan.keys.len();
    let obligation_count = plan.obligation_count;
    let profile = plan.profile;
    let candidate_sha = config.candidate_sha.clone();
    let receipt_path = staging.path().join("verification-receipt.json");
    let mut receipt = VerificationReceipt {
        schema_version: "harness.tui-fidelity.verification.v1".to_owned(),
        status: VerificationStatus::Passed,
        profile,
        candidate_sha,
        authority_sha256: config.authority_sha256.clone(),
        inventory_sha256: config.inventory_sha256.clone(),
        coverage_sha256: config.coverage_sha256.clone(),
        obligation_count,
        key_count,
        duration_millis: started.elapsed().as_millis(),
        scheduler: scheduler_report,
        evidence_path: staging.path().display().to_string(),
        sealed: false,
    };
    if receipt.scheduler.failed > 0 || receipt.scheduler.cancelled > 0 {
        receipt.status = VerificationStatus::Failed;
        write_receipt(&receipt_path, &receipt)?;
        return Err(VerifyError::Invalid(format!(
            "{} failed and {} cancelled verification keys",
            receipt.scheduler.failed, receipt.scheduler.cancelled
        )));
    }
    if started.elapsed() > profile.deadline() {
        receipt.status = VerificationStatus::Failed;
        write_receipt(&receipt_path, &receipt)?;
        return Err(VerifyError::Invalid(format!(
            "profile {:?} exceeded {} seconds",
            profile,
            profile.deadline().as_secs()
        )));
    }
    if profile == VerificationProfile::All {
        let published = staging.sealed_path()?;
        receipt.evidence_path = published.display().to_string();
        receipt.sealed = true;
        write_receipt(&receipt_path, &receipt)?;
        let actual = staging.seal()?;
        if actual != published {
            return Err(VerifyError::Invalid(
                "sealed evidence path differs from receipt".to_owned(),
            ));
        }
    } else {
        write_receipt(&receipt_path, &receipt)?;
    }
    Ok(receipt)
}

fn write_receipt(path: &Path, receipt: &VerificationReceipt) -> Result<(), VerifyError> {
    let bytes =
        serde_json::to_vec_pretty(receipt).map_err(|error| VerifyError::Json(error.to_string()))?;
    std::fs::write(path, bytes).map_err(|error| VerifyError::Io {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })
}

fn motion_requirements(
    inventory: &RequirementInventory,
    manifest: &CoverageManifest,
) -> BTreeSet<String> {
    let rows = manifest
        .rows
        .iter()
        .map(|row| (row.requirement_id.as_str(), row))
        .collect::<BTreeMap<_, _>>();
    inventory
        .requirements
        .iter()
        .filter(|requirement| {
            requirement.obligation.obligation_type() == ObligationType::DualCapture
                && rows
                    .get(requirement.id.as_str())
                    .is_some_and(|row| CaptureKey::from_row(row).is_motion())
        })
        .map(|requirement| requirement.id.clone())
        .collect()
}
