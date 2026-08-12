use std::path::Path;
use std::process::{Command, Stdio};

use crate::tui_fidelity_runner::{ArtifactDigest, DualRuntimeReceipt, PresentationEvidence};

use super::super::error::ComparatorError;
use super::runtime_pair;

pub fn compare(capture: &DualRuntimeReceipt) -> Result<(), ComparatorError> {
    let (reference, candidate) = runtime_pair(capture)?;
    super::super::self_compare::reject_self_comparison(
        &reference.binary.sha256,
        &candidate.binary.sha256,
    )?;
    if reference.binary.path == candidate.binary.path {
        return Err(ComparatorError::SelfComparison {
            sha256: reference.binary.sha256.clone(),
        });
    }
    for runtime in [reference, candidate] {
        if runtime.binary.source_revision.is_empty() || runtime.binary.sha256.len() != 64 {
            return Err(ComparatorError::Invalid {
                detail: format!(
                    "{} binary provenance is incomplete",
                    runtime.adapter.as_str()
                ),
            });
        }
        for checkpoint in &runtime.checkpoints {
            for artifact in &checkpoint.artifacts {
                verify(artifact)?;
            }
        }
        let external = match &runtime.presentation {
            PresentationEvidence::ExternalOnly { external }
            | PresentationEvidence::HarnessNative { external, .. } => external,
        };
        for artifact in [&external.raw_ansi, &external.observations_artifact] {
            verify(artifact)?;
        }
        if let PresentationEvidence::HarnessNative {
            native_trace_artifact,
            ..
        } = &runtime.presentation
        {
            verify(native_trace_artifact)?;
        }
    }
    Ok(())
}

fn verify(artifact: &ArtifactDigest) -> Result<(), ComparatorError> {
    let observed = sha256_file(Path::new(&artifact.path))?;
    if observed == artifact.sha256 {
        Ok(())
    } else {
        Err(ComparatorError::Hashing {
            stale: vec![super::super::hashing::StaleArtifact {
                kind: artifact.path.clone(),
                expected: artifact.sha256.clone(),
                observed,
            }],
            stale_len: 1,
        })
    }
}

fn sha256_file(path: &Path) -> Result<String, ComparatorError> {
    let output = Command::new("sha256sum")
        .arg("--")
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| ComparatorError::Io {
            path: path.to_path_buf(),
            detail: error.to_string(),
        })?;
    if !output.status.success() {
        return Err(ComparatorError::Io {
            path: path.to_path_buf(),
            detail: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .map(str::to_owned)
        .ok_or_else(|| ComparatorError::Invalid {
            detail: format!("sha256sum returned no digest for {}", path.display()),
        })
}
