use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const BINARY_RECEIPT_SCHEMA: &str = "harness.tui-fidelity.binary-build.v1";

mod digest;
mod validation;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BinaryReceipt {
    pub schema_version: String,
    pub reference: BinaryIdentity,
    pub harness: BinaryIdentity,
    pub reference_repeat: RepeatBuildReceipt,
    pub harness_repeat: RepeatBuildReceipt,
    pub mutation_probe: MutationProbeReceipt,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BinaryIdentity {
    pub source_revision: String,
    pub clean_pre: bool,
    pub clean_post: bool,
    pub source_sha256: String,
    pub package: String,
    pub executable: String,
    pub target_dir: String,
    pub binary_path: String,
    pub version: String,
    pub sha256: String,
    pub cargo_lock_sha256: String,
    pub toolchain_sha256: String,
    pub rustc_version: String,
    pub rustc_sha256: String,
    pub cargo_version: String,
    pub cargo_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BuildFingerprint {
    pub source_revision: String,
    pub source_sha256: String,
    pub cargo_lock_sha256: String,
    pub toolchain_sha256: String,
    pub rustc_sha256: String,
    pub cargo_sha256: String,
    pub binary_sha256: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RepeatBuildReceipt {
    pub first: BuildFingerprint,
    pub second: BuildFingerprint,
    pub first_target_dir: String,
    pub second_target_dir: String,
    pub first_binary_path: String,
    pub second_binary_path: String,
    pub matching: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MutationProbeReceipt {
    pub wrong_revision_rejected: bool,
    pub mutated_digest_rejected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptExpectations {
    pub reference_revision: String,
    pub harness_revision: String,
    pub reference_clean_pre: bool,
    pub reference_clean_post: bool,
    pub harness_clean_pre: bool,
    pub harness_clean_post: bool,
    pub reference_package: String,
    pub reference_executable: String,
    pub harness_package: String,
    pub harness_executable: String,
}

#[derive(Debug)]
pub enum BinaryReceiptError {
    InvalidField {
        field: String,
        reason: String,
    },
    Mismatch {
        field: String,
        expected: String,
        actual: String,
    },
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    DigestCommand {
        path: PathBuf,
        message: String,
    },
    DigestMismatch {
        field: String,
        path: PathBuf,
        expected: String,
        actual: String,
    },
}

impl fmt::Display for BinaryReceiptError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidField { field, reason } => {
                write!(formatter, "invalid receipt field {field}: {reason}")
            }
            Self::Mismatch {
                field,
                expected,
                actual,
            } => write!(
                formatter,
                "receipt field {field} mismatch: expected {expected}, got {actual}"
            ),
            Self::Io { path, source } => write!(formatter, "read {}: {source}", path.display()),
            Self::Json { path, source } => {
                write!(formatter, "parse receipt {}: {source}", path.display())
            }
            Self::DigestCommand { path, message } => {
                write!(formatter, "sha256sum {} failed: {message}", path.display())
            }
            Self::DigestMismatch {
                field,
                path,
                expected,
                actual,
            } => write!(
                formatter,
                "receipt field {field} mismatch for {}: expected {expected}, got {actual}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for BinaryReceiptError {}

pub fn read_receipt(path: &Path) -> Result<BinaryReceipt, BinaryReceiptError> {
    let bytes = fs::read(path).map_err(|source| BinaryReceiptError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| BinaryReceiptError::Json {
        path: path.to_path_buf(),
        source,
    })
}
