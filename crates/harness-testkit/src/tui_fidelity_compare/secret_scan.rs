use std::path::{Path, PathBuf};

pub use crate::secret_scanner::SecretFinding;

use super::error::ComparatorError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactRoots {
    pub reference: PathBuf,
    pub candidate: PathBuf,
}

impl ArtifactRoots {
    pub fn new(reference: impl Into<PathBuf>, candidate: impl Into<PathBuf>) -> Self {
        Self {
            reference: reference.into(),
            candidate: candidate.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretScanResult {
    pub findings: Vec<SecretFinding>,
}

impl SecretScanResult {
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }
}

pub fn scan_artifacts(roots: &ArtifactRoots) -> Result<SecretScanResult, ComparatorError> {
    let patterns = crate::secret_scanner::default_forbidden_patterns().map_err(|error| {
        ComparatorError::Invalid {
            detail: format!("secret pattern: {error}"),
        }
    })?;
    let findings = crate::secret_scanner::scan_directories_for_secrets(
        [&roots.reference, &roots.candidate],
        &patterns,
    )
    .map_err(|error| ComparatorError::Io {
        path: roots.candidate.clone(),
        detail: error.to_string(),
    })?;
    if findings.is_empty() {
        Ok(SecretScanResult { findings })
    } else {
        let findings_len = findings.len();
        Err(ComparatorError::Secrets {
            findings,
            findings_len,
        })
    }
}

pub fn scan_directory(path: &Path) -> Result<SecretScanResult, ComparatorError> {
    let patterns = crate::secret_scanner::default_forbidden_patterns().map_err(|error| {
        ComparatorError::Invalid {
            detail: format!("secret pattern: {error}"),
        }
    })?;
    let findings = crate::secret_scanner::scan_directory_tree_for_secrets(path, &patterns)
        .map_err(|error| ComparatorError::Io {
            path: path.to_path_buf(),
            detail: error.to_string(),
        })?;
    if findings.is_empty() {
        Ok(SecretScanResult { findings })
    } else {
        let findings_len = findings.len();
        Err(ComparatorError::Secrets {
            findings,
            findings_len,
        })
    }
}
