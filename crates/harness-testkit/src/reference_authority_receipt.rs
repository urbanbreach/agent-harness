use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const REFERENCE_RECEIPT_SCHEMA: &str = "harness.tui-fidelity.reference-binary-receipt.v1";

#[derive(Debug, thiserror::Error)]
pub enum ReferenceReceiptError {
    #[error("read receipt {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("parse receipt {path}: {source}")]
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("reference receipt mismatch: {0}")]
    Mismatch(String),
    #[error("inspect reference binary {path}: {source}")]
    Binary {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("reference binary --version output is not UTF-8")]
    VersionEncoding,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceAuthorityReceipt {
    schema_version: String,
    observed_at: String,
    source: Source,
    toolchain: Toolchain,
    binary: Binary,
    provenance: Provenance,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    canonical_checkout: PathBuf,
    revision: String,
    tree: String,
    clean: bool,
    cargo_lock_sha256: String,
    rust_toolchain_sha256: String,
    cargo_config_sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Toolchain {
    rustc_version: String,
    rustc_sha256: String,
    cargo_version: String,
    cargo_sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binary {
    path: PathBuf,
    sha256: String,
    version: String,
    mtime: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Provenance {
    packet: String,
    #[serde(rename = "prior_evidence")]
    _prior_evidence: PathBuf,
    #[serde(rename = "rebuild_performed_during_reconciliation")]
    _rebuild_performed_during_reconciliation: bool,
}

impl ReferenceAuthorityReceipt {
    pub fn read(path: &Path) -> Result<Self, ReferenceReceiptError> {
        let bytes = std::fs::read(path).map_err(|source| ReferenceReceiptError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        serde_json::from_slice(&bytes).map_err(|source| ReferenceReceiptError::Parse {
            path: path.to_path_buf(),
            source,
        })
    }

    pub fn verify(
        &self,
        repo_root: &Path,
        binary_path: &Path,
        expected_revision: &str,
    ) -> Result<(), ReferenceReceiptError> {
        self.verify_shape(expected_revision)?;
        let expected_path =
            std::fs::canonicalize(repo_root.join(&self.binary.path)).map_err(|source| {
                ReferenceReceiptError::Binary {
                    path: self.binary.path.clone(),
                    source,
                }
            })?;
        let actual_path =
            std::fs::canonicalize(binary_path).map_err(|source| ReferenceReceiptError::Binary {
                path: binary_path.to_path_buf(),
                source,
            })?;
        mismatch_if(
            expected_path != actual_path,
            "binary path is not canonical authority path",
        )?;
        let bytes =
            std::fs::read(&actual_path).map_err(|source| ReferenceReceiptError::Binary {
                path: actual_path.clone(),
                source,
            })?;
        mismatch_if(
            hex_digest(&bytes) != self.binary.sha256,
            "binary digest differs",
        )?;
        let output = Command::new(&actual_path)
            .arg("--version")
            .output()
            .map_err(|source| ReferenceReceiptError::Binary {
                path: actual_path,
                source,
            })?;
        mismatch_if(!output.status.success(), "binary --version failed")?;
        mismatch_if(
            !output.stderr.is_empty(),
            "binary --version wrote to stderr",
        )?;
        let version = std::str::from_utf8(&output.stdout)
            .map_err(|_| ReferenceReceiptError::VersionEncoding)?;
        mismatch_if(
            version.trim_end_matches(['\r', '\n']) != self.binary.version,
            "binary version differs",
        )
    }

    pub fn revision(&self) -> &str {
        &self.source.revision
    }

    pub fn sha256(&self) -> &str {
        &self.binary.sha256
    }

    fn verify_shape(&self, expected_revision: &str) -> Result<(), ReferenceReceiptError> {
        mismatch_if(
            self.schema_version != REFERENCE_RECEIPT_SCHEMA,
            "schema differs",
        )?;
        mismatch_if(!self.source.clean, "source is not clean")?;
        mismatch_if(
            self.source.revision != expected_revision,
            "revision differs",
        )?;
        require_hex(&self.source.revision, 40, "source revision")?;
        require_hex(&self.source.tree, 40, "source tree")?;
        require_hex(&self.source.cargo_lock_sha256, 64, "Cargo.lock digest")?;
        require_hex(
            &self.source.rust_toolchain_sha256,
            64,
            "rust-toolchain digest",
        )?;
        require_hex(&self.source.cargo_config_sha256, 64, "cargo config digest")?;
        require_hex(&self.toolchain.rustc_sha256, 64, "rustc digest")?;
        require_hex(&self.toolchain.cargo_sha256, 64, "cargo digest")?;
        require_hex(&self.binary.sha256, 64, "binary digest")?;
        for value in [
            &self.observed_at,
            &self.toolchain.rustc_version,
            &self.toolchain.cargo_version,
            &self.binary.version,
            &self.binary.mtime,
            &self.provenance.packet,
        ] {
            mismatch_if(value.trim().is_empty(), "required field is empty")?;
        }
        mismatch_if(
            self.source.canonical_checkout.as_os_str().is_empty(),
            "checkout path empty",
        )?;
        mismatch_if(self.binary.path.as_os_str().is_empty(), "binary path empty")?;
        Ok(())
    }
}

fn require_hex(value: &str, width: usize, field: &str) -> Result<(), ReferenceReceiptError> {
    mismatch_if(
        value.len() != width
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        &format!("{field} is not {width}-digit hexadecimal"),
    )
}

fn mismatch_if(condition: bool, detail: &str) -> Result<(), ReferenceReceiptError> {
    if condition {
        Err(ReferenceReceiptError::Mismatch(detail.to_owned()))
    } else {
        Ok(())
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            use std::fmt::Write as _;
            let _ = write!(output, "{byte:02x}");
            output
        })
}
