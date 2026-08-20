use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde::Deserialize;
use sha2::{Digest as _, Sha256};

use crate::tui_fidelity_runner::{
    ArtifactDigest, CandidateBinding, CandidateReceiptKind, CandidateRepositoryBinding,
    DualRuntimeReceipt, PresentationEvidence,
};

use super::super::error::ComparatorError;
use super::runtime_pair;

const CANDIDATE_BINDING_SCHEMA: &str = "harness.tui-fidelity.candidate-binding.v2";
const SOURCE_GUARD_SCHEMA: &str = "harness.tui-fidelity.source-guard.v2";

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
    validate_candidate_binding(&capture.candidate_binding, capture)?;
    for runtime in [reference, candidate] {
        if runtime.binary.source_revision.is_empty() || runtime.binary.sha256.len() != 64 {
            return invalid(format!(
                "{} binary provenance is incomplete",
                runtime.adapter.as_str()
            ));
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

fn validate_candidate_binding(
    binding: &CandidateBinding,
    capture: &DualRuntimeReceipt,
) -> Result<(), ComparatorError> {
    if binding.schema_version != CANDIDATE_BINDING_SCHEMA
        || !binding.parity_acceptance_eligible
        || !candidate_release_class_is_coherent(binding)
    {
        return invalid(
            "candidate binding is not parity-eligible or has an incoherent release class",
        );
    }
    let repository = fs::canonicalize(&binding.repository.canonical_path).map_err(|error| {
        ComparatorError::Io {
            path: binding.repository.canonical_path.clone(),
            detail: error.to_string(),
        }
    })?;
    if repository != binding.repository.canonical_path {
        return invalid("candidate repository path is not canonical");
    }
    let current = current_repository_binding(&repository)?;
    if current != binding.repository {
        return invalid("candidate repository bytes or Git identity changed after binding");
    }
    let (_, candidate) = runtime_pair(capture)?;
    let target = candidate
        .binary
        .path
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| ComparatorError::Invalid {
            detail: "candidate binary is not under target/<profile>".to_owned(),
        })?;
    let target = fs::canonicalize(target).map_err(|error| ComparatorError::Io {
        path: target.to_path_buf(),
        detail: error.to_string(),
    })?;
    if target != binding.target_dir
        || candidate.binary.source_revision != binding.repository.head
        || candidate.binary.sha256 != binding.binaries.harness_sha256
    {
        return invalid("candidate target, revision, or harness digest does not match binding");
    }
    verify_path_digest(
        &binding.authority.path,
        &binding.authority.sha256,
        "authority",
    )?;
    verify_path_digest(
        &binding.reference_receipt.path,
        &binding.reference_receipt.sha256,
        "reference receipt",
    )?;
    if reference_source_revision(capture)? != binding.authority.revision {
        return invalid("authority revision does not match captured reference revision");
    }
    validate_authority(binding, &repository)?;
    let runner = std::env::current_exe().map_err(|error| ComparatorError::Io {
        path: PathBuf::from("<current-executable>"),
        detail: error.to_string(),
    })?;
    verify_path_digest(&runner, &binding.binaries.runner_sha256, "runner")?;
    verify_path_digest(
        &binding.target_dir.join("debug/tui_fidelity_aggregate"),
        &binding.binaries.aggregate_sha256,
        "aggregate",
    )?;
    validate_source_guards(binding, capture)
}

fn candidate_release_class_is_coherent(binding: &CandidateBinding) -> bool {
    match binding.receipt_kind {
        CandidateReceiptKind::Release => {
            binding.release_eligible && binding.clean_release && binding.repository.clean
        }
        CandidateReceiptKind::DiagnosticNonRelease => {
            !binding.release_eligible && !binding.clean_release && !binding.repository.clean
        }
        CandidateReceiptKind::Fixture => false,
    }
}

fn validate_authority(
    binding: &CandidateBinding,
    repository: &Path,
) -> Result<(), ComparatorError> {
    let expected_authority = repository.join("configs/tui-fidelity-reference-authority.json");
    if binding.authority.path != expected_authority {
        return invalid("candidate binding does not name the canonical reference authority");
    }
    let bytes = fs::read(&binding.authority.path).map_err(|error| ComparatorError::Io {
        path: binding.authority.path.clone(),
        detail: error.to_string(),
    })?;
    let authority: ReferenceAuthority =
        serde_json::from_slice(&bytes).map_err(|error| ComparatorError::Invalid {
            detail: format!("reference authority is invalid: {error}"),
        })?;
    let expected_receipt = repository.join(authority.reference.receipt_path);
    if authority.schema_version != "harness.tui-fidelity.reference-authority.v1"
        || authority.status != "active"
        || authority.reference.source_revision != binding.authority.revision
        || expected_receipt != binding.reference_receipt.path
    {
        return invalid("reference authority fields do not match candidate binding");
    }
    Ok(())
}

fn current_repository_binding(
    repository: &Path,
) -> Result<CandidateRepositoryBinding, ComparatorError> {
    let head = git_text(repository, &["rev-parse", "HEAD"])?;
    let tree = git_text(repository, &["rev-parse", "HEAD^{tree}"])?;
    let status = git_output(
        repository,
        &["status", "--porcelain=v1", "--untracked-files=all", "-z"],
    )?;
    let dirty_diff = git_output(repository, &["diff", "--binary", "HEAD", "--"])?;
    Ok(CandidateRepositoryBinding {
        canonical_path: repository.to_path_buf(),
        head,
        tree,
        clean: status.stdout.is_empty(),
        tracked_source_sha256: source_manifest_sha256(repository, false)?,
        dirty_diff_sha256: sha256_bytes(&dirty_diff.stdout),
        untracked_manifest_sha256: source_manifest_sha256(repository, true)?,
        cargo_lock_sha256: sha256_path(&repository.join("Cargo.lock"))?,
        toolchain_sha256: sha256_path(&repository.join("rust-toolchain.toml"))?,
        cargo_config_sha256: optional_sha256(&repository.join(".cargo/config.toml"))?,
    })
}

fn source_manifest_sha256(repository: &Path, untracked: bool) -> Result<String, ComparatorError> {
    let args = if untracked {
        ["ls-files", "--others", "--exclude-standard", "-z"].as_slice()
    } else {
        ["ls-files", "-z"].as_slice()
    };
    let listed = git_output(repository, args)?;
    let mut records = Vec::new();
    for raw_path in listed
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        if !untracked && !approved_source_path(raw_path) {
            continue;
        }
        let path = repository.join(OsStr::from_bytes(raw_path));
        let mut record = if path.exists() {
            sha256_path(&path)?.into_bytes()
        } else if untracked {
            return invalid("listed untracked source path is missing");
        } else {
            b"deleted".to_vec()
        };
        record.extend_from_slice(b"  ");
        record.extend_from_slice(raw_path);
        record.push(0);
        records.push(record);
    }
    records.sort_unstable();
    Ok(sha256_bytes(&records.concat()))
}

fn approved_source_path(path: &[u8]) -> bool {
    path.ends_with(b".rs")
        || path.ends_with(b".sh")
        || path.ends_with(b".py")
        || path.ends_with(b".json")
        || path.ends_with(b".jsonc")
        || path.ends_with(b".toml")
        || path.ends_with(b".yaml")
        || path.ends_with(b".yml")
        || path == b"Cargo.lock"
        || path == b"rust-toolchain"
}

fn validate_source_guards(
    binding: &CandidateBinding,
    capture: &DualRuntimeReceipt,
) -> Result<(), ComparatorError> {
    verify(&capture.source_guard_before)?;
    verify(&capture.source_guard_after)?;
    let before =
        fs::read(&capture.source_guard_before.path).map_err(|error| ComparatorError::Io {
            path: PathBuf::from(&capture.source_guard_before.path),
            detail: error.to_string(),
        })?;
    let after =
        fs::read(&capture.source_guard_after.path).map_err(|error| ComparatorError::Io {
            path: PathBuf::from(&capture.source_guard_after.path),
            detail: error.to_string(),
        })?;
    if before != after
        || capture.source_guard_before.sha256 != binding.source_guard_receipt_sha256
        || capture.source_guard_after.sha256 != binding.source_guard_receipt_sha256
    {
        return invalid("source-guard before/final receipts do not equal the bound receipt");
    }
    let guard: SourceGuardReceipt =
        serde_json::from_slice(&after).map_err(|error| ComparatorError::Invalid {
            detail: format!("source-guard receipt is invalid: {error}"),
        })?;
    if guard.schema != SOURCE_GUARD_SCHEMA
        || !guard.reference.clean_pre
        || !guard.reference.clean_post
        || guard.harness.clean_pre != binding.repository.clean
        || guard.harness.clean_post != binding.repository.clean
        || guard.reference.revision != binding.authority.revision
        || guard.harness.revision != binding.repository.head
        || guard.harness.tree != binding.repository.tree
        || guard.harness.source_sha256 != binding.repository.tracked_source_sha256
        || guard.harness.dirty_diff_sha256 != binding.repository.dirty_diff_sha256
        || guard.harness.untracked_manifest_sha256 != binding.repository.untracked_manifest_sha256
        || guard.harness.cargo_lock_sha256 != binding.repository.cargo_lock_sha256
        || guard.harness.toolchain_sha256 != binding.repository.toolchain_sha256
        || guard.harness.cargo_config_sha256 != binding.repository.cargo_config_sha256
        || guard.harness.path != binding.repository.canonical_path
    {
        return invalid("source-guard receipt does not cover the bound clean source state");
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceGuardReceipt {
    schema: String,
    reference: GuardSource,
    harness: GuardSource,
    #[serde(rename = "tools")]
    _tools: serde_json::Value,
}

#[derive(Deserialize)]
struct ReferenceAuthority {
    schema_version: String,
    status: String,
    reference: ReferenceAuthorityIdentity,
}

#[derive(Deserialize)]
struct ReferenceAuthorityIdentity {
    source_revision: String,
    receipt_path: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GuardSource {
    path: PathBuf,
    revision: String,
    tree: String,
    #[serde(rename = "status_sha256")]
    _status_sha256: String,
    source_sha256: String,
    dirty_diff_sha256: String,
    untracked_manifest_sha256: String,
    cargo_lock_sha256: String,
    toolchain_sha256: String,
    cargo_config_sha256: Option<String>,
    clean_pre: bool,
    clean_post: bool,
}

fn reference_source_revision(capture: &DualRuntimeReceipt) -> Result<&str, ComparatorError> {
    runtime_pair(capture).map(|(reference, _)| reference.binary.source_revision.as_str())
}

fn verify_path_digest(path: &Path, expected: &str, kind: &str) -> Result<(), ComparatorError> {
    let observed = sha256_path(path)?;
    if observed == expected {
        Ok(())
    } else {
        invalid(format!("{kind} digest changed after candidate binding"))
    }
}

fn verify(artifact: &ArtifactDigest) -> Result<(), ComparatorError> {
    let observed = sha256_path(Path::new(&artifact.path))?;
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

fn optional_sha256(path: &Path) -> Result<Option<String>, ComparatorError> {
    if path.exists() {
        sha256_path(path).map(Some)
    } else {
        Ok(None)
    }
}

fn sha256_path(path: &Path) -> Result<String, ComparatorError> {
    let bytes = fs::read(path).map_err(|error| ComparatorError::Io {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    Ok(sha256_bytes(&bytes))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn git_text(repository: &Path, args: &[&str]) -> Result<String, ComparatorError> {
    let output = git_output(repository, args)?;
    String::from_utf8(output.stdout)
        .map(|text| text.trim().to_owned())
        .map_err(|error| ComparatorError::Invalid {
            detail: format!("Git returned non-UTF-8 identity: {error}"),
        })
}

fn git_output(repository: &Path, args: &[&str]) -> Result<Output, ComparatorError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repository)
        .env("GIT_MASTER", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| ComparatorError::Io {
            path: repository.to_path_buf(),
            detail: error.to_string(),
        })?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(ComparatorError::Invalid {
            detail: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        })
    }
}

fn invalid<T>(detail: impl Into<String>) -> Result<T, ComparatorError> {
    Err(ComparatorError::Invalid {
        detail: detail.into(),
    })
}
