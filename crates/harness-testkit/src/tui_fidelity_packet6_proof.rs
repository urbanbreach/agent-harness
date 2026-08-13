use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use crate::tui_fidelity_packet6::Packet6CapabilityError;

pub(super) fn require_digest(value: &str) -> Result<(), Packet6CapabilityError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(Packet6CapabilityError::Input(
            "digest is not lowercase SHA-256".to_owned(),
        ))
    }
}

pub(super) fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            let _ = write!(output, "{byte:02x}");
            output
        })
}

pub(super) fn canonical_evidence_path(
    root: &Path,
    path: &Path,
) -> Result<PathBuf, Packet6CapabilityError> {
    let canonical_root =
        std::fs::canonicalize(root).map_err(|error| Packet6CapabilityError::Proof {
            path: root.to_path_buf(),
            detail: error.to_string(),
        })?;
    let canonical_path =
        std::fs::canonicalize(root.join(path)).map_err(|error| Packet6CapabilityError::Proof {
            path: path.to_path_buf(),
            detail: error.to_string(),
        })?;
    if canonical_path.starts_with(canonical_root) {
        Ok(canonical_path)
    } else {
        Err(Packet6CapabilityError::Proof {
            path: path.to_path_buf(),
            detail: "canonical path escapes evidence root".to_owned(),
        })
    }
}
