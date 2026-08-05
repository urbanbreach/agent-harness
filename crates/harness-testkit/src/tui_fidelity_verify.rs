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
    pub attempt_id: String,
    pub evidence_root: PathBuf,
    pub workers: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationReceipt {
    pub schema_version: String,
    pub profile: VerificationProfile,
    pub candidate_sha: String,
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
    if scheduler_report.failed > 0 || scheduler_report.cancelled > 0 {
        return Err(VerifyError::Invalid(format!(
            "{} failed and {} cancelled verification keys",
            scheduler_report.failed, scheduler_report.cancelled
        )));
    }
    if started.elapsed() > plan.profile.deadline() {
        return Err(VerifyError::Invalid(format!(
            "profile {:?} exceeded {} seconds",
            plan.profile,
            plan.profile.deadline().as_secs()
        )));
    }
    let key_count = plan.keys.len();
    let obligation_count = plan.obligation_count;
    let profile = plan.profile;
    let candidate_sha = config.candidate_sha.clone();
    let evidence_path = if profile == VerificationProfile::All {
        staging.sealed_path()?
    } else {
        staging.path().to_path_buf()
    };
    let receipt = VerificationReceipt {
        schema_version: "harness.tui-fidelity.verification.v1".to_owned(),
        profile,
        candidate_sha,
        obligation_count,
        key_count,
        duration_millis: started.elapsed().as_millis(),
        scheduler: scheduler_report,
        evidence_path: evidence_path.display().to_string(),
        sealed: profile == VerificationProfile::All,
    };
    let receipt_path = staging.path().join("verification-receipt.json");
    let bytes = serde_json::to_vec_pretty(&receipt)
        .map_err(|error| VerifyError::Json(error.to_string()))?;
    std::fs::write(&receipt_path, bytes).map_err(|error| VerifyError::Io {
        path: receipt_path,
        detail: error.to_string(),
    })?;
    if profile == VerificationProfile::All {
        let published = staging.seal()?;
        if published != evidence_path {
            return Err(VerifyError::Invalid(
                "sealed evidence path differs from receipt".to_owned(),
            ));
        }
    }
    Ok(receipt)
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
