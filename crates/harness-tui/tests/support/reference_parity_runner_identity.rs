//! Fail-closed runner-identity validator for exact-binary parity captures.
//!
//! Locks Contract §5.1 (Todo 5 of grok-build-clean-room-parity.md):
//! - Reference captures execute ONLY the frozen absolute reference binary.
//! - Candidate captures execute ONLY the explicit absolute `HARNESS_BIN`.
//! - Capture metadata records absolute binary path, SHA-256, version, and
//!   permissions for both reference and candidate.
//! - Helper-binary substitution, copied reference-digest seeding, silent
//!   dry-run skips, cargo run / current-process self-rendering, and
//!   identical reference/candidate process ids or paths are all rejected.
//!
//! The validator is self-contained (std + serde only) so the
//! `reference_parity_runner_identity_test` integration test can drive it
//! with in-memory fixtures without spinning a real binary.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Canonical absolute path of the frozen reference binary (Todo 5 plan).
pub const REFERENCE_BINARY_ABSOLUTE_PATH: &str =
    "/home/urbanbreach/Projects/agent-harness/inspirations/grok-build/target/debug/xai-grok-pager";

/// Pinned SHA-256 of the frozen reference binary. Mirrors
/// `support::REFERENCE_BINARY_SHA256` to keep this leaf independent.
pub const REFERENCE_BINARY_SHA256: &str =
    "883e3dea2a57773f3a9b229746ff7a99b9761836401e0f022599914b3bb9a9a5";

/// Reported version string of the frozen reference binary.
pub const REFERENCE_BINARY_VERSION: &str = "grok 0.1.220-alpha.4 (c1b5909) [stable]";

/// File-name fragments that mark helper / native-visual helper binaries.
/// A capture whose `binary_path` basename contains one of these markers is
/// rejected: helpers produce scenario output but are NOT the candidate under
/// signoff, so they cannot substitute for the explicit `HARNESS_BIN`.
pub const HELPER_BINARY_NAME_MARKERS: &[&str] = &[
    "reference_parity_pty_test",
    "pty_helper",
    "native_visual_helper",
    "simulation_evidence",
];

/// Prefixes that mark cargo-run / current-process self-rendering. A capture
/// whose `binary_path` or `generating_command` starts with one of these is
/// rejected: signoff must execute a built absolute binary, not `cargo run`.
pub const CARGO_RUN_MARKERS: &[&str] = &[
    "cargo run",
    "cargo +",
    "target/debug/harness ",
    "cargo-test-",
];

/// Which side of the comparison a runner identity belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunnerKind {
    Reference,
    Candidate,
}

/// Recorded runner identity for a captured artifact. Mirrors the fields the
/// capture scripts (`web-terminal-visual-qa.mjs` and family) must write into
/// `metadata.json` under `runner_identity`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunnerIdentity {
    pub runner_kind: RunnerKind,
    /// Absolute filesystem path of the binary that produced the capture.
    pub binary_path: String,
    /// SHA-256 (lowercase hex) of the binary at `binary_path`.
    pub binary_sha256: String,
    /// `--version` output (single line) of the binary.
    pub binary_version: String,
    /// POSIX permission octal (e.g. "755"). Required for both runners.
    pub permissions: String,
    /// OS process id recorded at capture time. Used to detect the current
    /// process being used as both reference and candidate.
    #[serde(default)]
    pub process_id: Option<u32>,
}

/// Capture metadata shape validated by this leaf. Tests build it inline; the
/// capture scripts write the same shape into `metadata.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CaptureMetadata {
    /// Recorded runner identity. `None` means the capture omitted the
    /// `runner_identity` block entirely (the validator rejects this).
    #[serde(default)]
    pub runner_identity: Option<RunnerIdentity>,
    /// Dry-run marker. A capture that records `dry_run=true` is wiring-only
    /// evidence and may never satisfy freshness for a claimed row.
    #[serde(default)]
    pub dry_run: bool,
    /// Extra generating-command / source-label fields kept verbatim. The
    /// validator inspects these for cargo-run markers.
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
}

/// Typed runner-identity failures. Each variant names a distinct rejected
/// class so tests can assert on the exact control.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunnerIdentityError {
    /// Candidate capture missing `HARNESS_BIN`-derived identity entirely.
    MissingCandidateHarnessBin,
    /// Capture missing the `runner_identity` block.
    MissingRunnerIdentity { kind: RunnerKind },
    /// A required identity field is empty or absent.
    MissingIdentityField {
        kind: RunnerKind,
        field: &'static str,
    },
    /// Path is not absolute (signoff must pin an exact built binary).
    RelativeBinaryPath { kind: RunnerKind, path: String },
    /// Helper binary path was recorded in place of the real runner.
    HelperBinarySubstituted {
        kind: RunnerKind,
        path: String,
        marker: String,
    },
    /// `cargo run` / cargo-test self-rendering detected.
    CargoRunSelfRendering { kind: RunnerKind, marker: String },
    /// Reference path does not match the frozen absolute reference binary.
    ReferenceBinaryPathMismatch {
        expected: &'static str,
        actual: String,
    },
    /// Reference SHA-256 does not match the pinned reference binary digest.
    ReferenceBinaryShaMismatch {
        expected: &'static str,
        actual: String,
    },
    /// Reference version does not match the pinned reference binary version.
    ReferenceBinaryVersionMismatch {
        expected: &'static str,
        actual: String,
    },
    /// Candidate SHA-256 differs from the expected digest derived from
    /// `HARNESS_BIN` at capture time.
    MismatchedCandidateSha { expected: String, actual: String },
    /// Candidate digest equals the reference digest (copied reference-digest
    /// seeding is forbidden).
    CopiedReferenceDigest,
    /// Reference and candidate recorded the same binary path (self-oracle).
    IdenticalRunnerPaths { path: String },
    /// Reference and candidate recorded the same binary SHA-256 (self-oracle).
    IdenticalRunnerSha { sha: String },
    /// Reference and candidate recorded the same OS process id (current
    /// process used as both runners).
    IdenticalRunnerProcessId { pid: u32 },
    /// Capture metadata marks the run as dry-only (cannot satisfy freshness).
    DryRunOnlyEvidence,
}

/// Validate a pair of captures (reference, candidate) for runner-identity
/// freshness (Contract §5.1). Returns `Err` listing every typed violation.
///
/// `expected_candidate_sha` is the SHA-256 of the absolute `HARNESS_BIN` at
/// capture time, captured BEFORE invoking the binary. Pass `None` to skip the
/// cross-check (e.g. unit tests that only exercise the structural guards).
pub fn validate_runner_pair(
    reference: &CaptureMetadata,
    candidate: &CaptureMetadata,
    expected_candidate_sha: Option<&str>,
) -> Result<(), Vec<RunnerIdentityError>> {
    let mut errors = Vec::new();
    if reference.dry_run || candidate.dry_run {
        errors.push(RunnerIdentityError::DryRunOnlyEvidence);
    }
    let Some(reference_id) = &reference.runner_identity else {
        errors.push(RunnerIdentityError::MissingRunnerIdentity {
            kind: RunnerKind::Reference,
        });
        return finalize(errors);
    };
    let Some(candidate_id) = &candidate.runner_identity else {
        errors.push(RunnerIdentityError::MissingCandidateHarnessBin);
        return finalize(errors);
    };
    validate_single(reference_id, &mut errors);
    validate_single(candidate_id, &mut errors);
    validate_reference_pins(reference_id, &mut errors);
    validate_candidate_sha(candidate_id, expected_candidate_sha, &mut errors);
    validate_distinct(reference_id, candidate_id, &mut errors);
    finalize(errors)
}

fn validate_candidate_sha(
    candidate: &RunnerIdentity,
    expected: Option<&str>,
    errors: &mut Vec<RunnerIdentityError>,
) {
    let Some(expected_sha) = expected else {
        return;
    };
    if !expected_sha.is_empty() && candidate.binary_sha256 != expected_sha {
        errors.push(RunnerIdentityError::MismatchedCandidateSha {
            expected: expected_sha.to_owned(),
            actual: candidate.binary_sha256.clone(),
        });
    }
}

fn validate_single(id: &RunnerIdentity, errors: &mut Vec<RunnerIdentityError>) {
    let kind = id.runner_kind;
    for (field, value) in [
        ("binary_path", &id.binary_path),
        ("binary_sha256", &id.binary_sha256),
        ("binary_version", &id.binary_version),
        ("permissions", &id.permissions),
    ] {
        if value.trim().is_empty() {
            errors.push(RunnerIdentityError::MissingIdentityField { kind, field });
        }
    }
    if !id.binary_path.trim().is_empty() {
        validate_path_shape(id, errors);
    }
    if !id.binary_sha256.trim().is_empty() && !is_sha256_hex(&id.binary_sha256) {
        errors.push(RunnerIdentityError::MissingIdentityField {
            kind,
            field: "binary_sha256_format",
        });
    }
}

fn validate_path_shape(id: &RunnerIdentity, errors: &mut Vec<RunnerIdentityError>) {
    let kind = id.runner_kind;
    let path = &id.binary_path;
    if !path.starts_with('/') {
        errors.push(RunnerIdentityError::RelativeBinaryPath {
            kind,
            path: path.clone(),
        });
        return;
    }
    if let Some(marker) = HELPER_BINARY_NAME_MARKERS
        .iter()
        .copied()
        .find(|marker| path.contains(marker))
    {
        errors.push(RunnerIdentityError::HelperBinarySubstituted {
            kind,
            path: path.clone(),
            marker: (*marker).to_owned(),
        });
    }
}

fn validate_reference_pins(id: &RunnerIdentity, errors: &mut Vec<RunnerIdentityError>) {
    if id.runner_kind != RunnerKind::Reference {
        return;
    }
    if id.binary_path != REFERENCE_BINARY_ABSOLUTE_PATH {
        errors.push(RunnerIdentityError::ReferenceBinaryPathMismatch {
            expected: REFERENCE_BINARY_ABSOLUTE_PATH,
            actual: id.binary_path.clone(),
        });
    }
    if id.binary_sha256 != REFERENCE_BINARY_SHA256 {
        errors.push(RunnerIdentityError::ReferenceBinaryShaMismatch {
            expected: REFERENCE_BINARY_SHA256,
            actual: id.binary_sha256.clone(),
        });
    }
    if id.binary_version != REFERENCE_BINARY_VERSION {
        errors.push(RunnerIdentityError::ReferenceBinaryVersionMismatch {
            expected: REFERENCE_BINARY_VERSION,
            actual: id.binary_version.clone(),
        });
    }
}

fn validate_distinct(
    reference: &RunnerIdentity,
    candidate: &RunnerIdentity,
    errors: &mut Vec<RunnerIdentityError>,
) {
    if reference.binary_path == candidate.binary_path {
        errors.push(RunnerIdentityError::IdenticalRunnerPaths {
            path: reference.binary_path.clone(),
        });
    }
    if !reference.binary_sha256.is_empty() && reference.binary_sha256 == candidate.binary_sha256 {
        if reference.binary_path != candidate.binary_path {
            errors.push(RunnerIdentityError::CopiedReferenceDigest);
        } else {
            errors.push(RunnerIdentityError::IdenticalRunnerSha {
                sha: reference.binary_sha256.clone(),
            });
        }
    }
    if let (Some(reference_pid), Some(candidate_pid)) = (reference.process_id, candidate.process_id)
    {
        if reference_pid == candidate_pid {
            errors.push(RunnerIdentityError::IdenticalRunnerProcessId { pid: reference_pid });
        }
    }
}

fn finalize(errors: Vec<RunnerIdentityError>) -> Result<(), Vec<RunnerIdentityError>> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn is_sha256_hex(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Reject `cargo run` / cargo-test self-rendering markers in a capture's
/// generating command or source label. Capture scripts must invoke a built
/// absolute binary directly.
pub fn reject_cargo_run_markers(metadata: &CaptureMetadata) -> Result<(), RunnerIdentityError> {
    for value in metadata.fields.values() {
        for marker in CARGO_RUN_MARKERS {
            if value.contains(marker) {
                return Err(RunnerIdentityError::CargoRunSelfRendering {
                    kind: RunnerKind::Candidate,
                    marker: (*marker).to_owned(),
                });
            }
        }
    }
    Ok(())
}
