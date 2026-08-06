mod state;

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::tui_fidelity_obligation::{ObligationError, VerificationKey};

use state::{
    atomic_json, digest_file, io_error, key_digest, read_record, validate_artifact,
    validate_component, KeyRecord, SealManifest,
};

const STATE_SCHEMA: &str = "harness.tui-fidelity.staging-key.v1";
const SEAL_SCHEMA: &str = "harness.tui-fidelity.sealed-evidence.v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttemptPolicy {
    Development,
    FinalAll,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyState {
    Pending,
    Running,
    Passed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobIsolation {
    pub evidence_dir: PathBuf,
    pub pty_dir: PathBuf,
    pub browser_profile: PathBuf,
    pub temp_dir: PathBuf,
}

#[derive(Debug)]
pub struct StagingArea {
    path: PathBuf,
    candidate: String,
    attempt: String,
    keys: Vec<VerificationKey>,
    policy: AttemptPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StagingError {
    Invalid(String),
    Io { path: PathBuf, detail: String },
    Json(String),
}

impl fmt::Display for StagingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(detail) => write!(formatter, "staging evidence: {detail}"),
            Self::Io { path, detail } => {
                write!(formatter, "staging I/O {}: {detail}", path.display())
            }
            Self::Json(detail) => write!(formatter, "staging JSON: {detail}"),
        }
    }
}

impl std::error::Error for StagingError {}

impl From<ObligationError> for StagingError {
    fn from(error: ObligationError) -> Self {
        Self::Invalid(error.to_string())
    }
}

impl StagingArea {
    pub fn open(
        evidence_root: &Path,
        candidate: &str,
        attempt: &str,
        keys: &[VerificationKey],
        policy: AttemptPolicy,
    ) -> Result<Self, StagingError> {
        validate_component(candidate, "candidate", true)?;
        validate_component(attempt, "attempt", false)?;
        let path = evidence_root.join(format!("staging-{candidate}-{attempt}"));
        fs::create_dir_all(path.join("keys")).map_err(|error| io_error(&path, error))?;
        let area = Self {
            path,
            candidate: candidate.to_owned(),
            attempt: attempt.to_owned(),
            keys: keys.to_vec(),
            policy,
        };
        for key in keys {
            area.initialize_or_resume(key)?;
        }
        Ok(area)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn sealed_path(&self) -> Result<PathBuf, StagingError> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| StagingError::Invalid("staging root has no parent".to_owned()))?;
        Ok(parent.join(format!("sealed-{}-{}", self.candidate, self.attempt)))
    }

    pub fn state(&self, key: &VerificationKey) -> Result<KeyState, StagingError> {
        Ok(self.read_record(key)?.state)
    }

    pub fn isolation(&self, key: &VerificationKey) -> Result<JobIsolation, StagingError> {
        let root = self.path.join("work").join(key_digest(key)?);
        let isolation = JobIsolation {
            evidence_dir: root.join("evidence"),
            pty_dir: root.join("pty"),
            browser_profile: root.join("browser"),
            temp_dir: root.join("tmp"),
        };
        for path in [
            &isolation.evidence_dir,
            &isolation.pty_dir,
            &isolation.browser_profile,
            &isolation.temp_dir,
        ] {
            fs::create_dir_all(path).map_err(|error| io_error(path, error))?;
        }
        Ok(isolation)
    }

    pub fn mark_running(&self, key: &VerificationKey) -> Result<(), StagingError> {
        self.transition(key, KeyState::Running, None, None)
    }

    pub fn mark_cancelled(&self, key: &VerificationKey) -> Result<(), StagingError> {
        self.transition(key, KeyState::Cancelled, None, None)
    }

    pub fn mark_failed(&self, key: &VerificationKey, detail: &str) -> Result<(), StagingError> {
        let mut record = self.read_record(key)?;
        if record.state != KeyState::Running {
            return Err(StagingError::Invalid(
                "only a running key can fail".to_owned(),
            ));
        }
        record.state = KeyState::Failed;
        record.failed_attempts = record.failed_attempts.saturating_add(1);
        record.detail = Some(detail.to_owned());
        self.write_record(key, &record)
    }

    pub fn mark_passed(&self, key: &VerificationKey, artifact: &Path) -> Result<(), StagingError> {
        let relative = artifact.strip_prefix(&self.path).map_err(|_| {
            StagingError::Invalid("pass artifact is outside staging evidence".to_owned())
        })?;
        let digest = digest_file(artifact)?;
        self.transition(
            key,
            KeyState::Passed,
            Some(relative.to_string_lossy().into_owned()),
            Some(digest),
        )
    }

    pub fn seal(self) -> Result<PathBuf, StagingError> {
        if self.policy != AttemptPolicy::FinalAll {
            return Err(StagingError::Invalid(
                "only final-all evidence can be sealed".to_owned(),
            ));
        }
        let mut records = Vec::with_capacity(self.keys.len());
        for key in &self.keys {
            let record = self.read_record(key)?;
            if record.state != KeyState::Passed {
                return Err(StagingError::Invalid(
                    "all required keys must pass before sealing".to_owned(),
                ));
            }
            validate_artifact(&self.path, &record)?;
            records.push(record);
        }
        let manifest = SealManifest {
            schema_version: SEAL_SCHEMA.to_owned(),
            candidate: self.candidate.clone(),
            attempt: self.attempt.clone(),
            records,
        };
        atomic_json(&self.path.join("seal-manifest.json"), &manifest)?;
        let sealed = self.sealed_path()?;
        if sealed.exists() {
            return Err(StagingError::Invalid(
                "sealed evidence already exists".to_owned(),
            ));
        }
        fs::rename(&self.path, &sealed).map_err(|error| io_error(&sealed, error))?;
        Ok(sealed)
    }

    fn initialize_or_resume(&self, key: &VerificationKey) -> Result<(), StagingError> {
        let path = self.record_path(key)?;
        if !path.exists() {
            return self.write_record(key, &KeyRecord::pending(key.stable_id()?));
        }
        let mut record = self.read_record(key)?;
        record.state = match record.state {
            KeyState::Running | KeyState::Cancelled => KeyState::Pending,
            KeyState::Failed if self.policy == AttemptPolicy::FinalAll => {
                return Err(StagingError::Invalid(
                    "final-all failure cannot be retried".to_owned(),
                ));
            }
            KeyState::Failed if record.failed_attempts < 2 => KeyState::Pending,
            KeyState::Pending | KeyState::Passed | KeyState::Failed => record.state,
        };
        if record.state == KeyState::Passed {
            validate_artifact(&self.path, &record)?;
        }
        self.write_record(key, &record)
    }

    fn transition(
        &self,
        key: &VerificationKey,
        state: KeyState,
        artifact_path: Option<String>,
        artifact_sha256: Option<String>,
    ) -> Result<(), StagingError> {
        let mut record = self.read_record(key)?;
        if state == KeyState::Running && record.state != KeyState::Pending {
            return Err(StagingError::Invalid(
                "only a pending key can start".to_owned(),
            ));
        }
        if state == KeyState::Passed && record.state != KeyState::Running {
            return Err(StagingError::Invalid(
                "only a running key can pass".to_owned(),
            ));
        }
        record.state = state;
        record.artifact_path = artifact_path;
        record.artifact_sha256 = artifact_sha256;
        self.write_record(key, &record)
    }

    fn read_record(&self, key: &VerificationKey) -> Result<KeyRecord, StagingError> {
        let path = self.record_path(key)?;
        let record = read_record(&path)?;
        if record.schema_version != STATE_SCHEMA || record.key_id != key.stable_id()? {
            return Err(StagingError::Invalid(
                "key state schema or identity differs".to_owned(),
            ));
        }
        Ok(record)
    }

    fn write_record(&self, key: &VerificationKey, record: &KeyRecord) -> Result<(), StagingError> {
        atomic_json(&self.record_path(key)?, record)
    }

    fn record_path(&self, key: &VerificationKey) -> Result<PathBuf, StagingError> {
        Ok(self
            .path
            .join("keys")
            .join(format!("{}.json", key_digest(key)?)))
    }
}
