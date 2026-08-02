//! Fail-closed runner-identity tests for exact-binary parity captures.
//!
//! Contract: `grok-build-clean-room-parity.md` Todo 5 (lines 204-210).
//! Coverage:
//!   - Happy: a seeded manifest with truthful reference and candidate runner
//!     identities passes validation.
//!   - Failure: missing HARNESS_BIN, mismatched candidate SHA, helper binary
//!     path, copied reference digest, identical reference/candidate process
//!     ids or paths, dry-run-only evidence, and cargo-run self-rendering are
//!     each rejected with a specific failure reason.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration runner-identity tests use fail-fast asserts"
)]

#[path = "support/reference_parity_runner_identity.rs"]
mod support;

use support::{
    reject_cargo_run_markers, validate_runner_pair, CaptureMetadata, RunnerIdentity,
    RunnerIdentityError, RunnerKind, REFERENCE_BINARY_ABSOLUTE_PATH, REFERENCE_BINARY_SHA256,
    REFERENCE_BINARY_VERSION,
};

const CANDIDATE_BIN: &str = "/home/urbanbreach/Projects/agent-harness/target/debug/harness";
const CANDIDATE_SHA: &str = "86a079821e9f35a880e8ab326d99e78afece590a643fd7a64598d19883ab087b";
const CANDIDATE_VERSION: &str = "harness 0.1.0";

fn truthful_reference() -> RunnerIdentity {
    RunnerIdentity {
        runner_kind: RunnerKind::Reference,
        binary_path: REFERENCE_BINARY_ABSOLUTE_PATH.to_owned(),
        binary_sha256: REFERENCE_BINARY_SHA256.to_owned(),
        binary_version: REFERENCE_BINARY_VERSION.to_owned(),
        permissions: "755".to_owned(),
        process_id: Some(1010),
    }
}

fn truthful_candidate() -> RunnerIdentity {
    RunnerIdentity {
        runner_kind: RunnerKind::Candidate,
        binary_path: CANDIDATE_BIN.to_owned(),
        binary_sha256: CANDIDATE_SHA.to_owned(),
        binary_version: CANDIDATE_VERSION.to_owned(),
        permissions: "755".to_owned(),
        process_id: Some(2020),
    }
}

fn metadata_for(id: RunnerIdentity) -> CaptureMetadata {
    CaptureMetadata {
        runner_identity: Some(id),
        dry_run: false,
        fields: std::collections::BTreeMap::new(),
    }
}

fn assert_has_error(result: Result<(), Vec<RunnerIdentityError>>, expected: RunnerIdentityError) {
    match result {
        Ok(()) => panic!("expected runner-identity error {expected:?}, got Ok"),
        Err(errors) => assert!(
            errors
                .iter()
                .any(|err| std::mem::discriminant(err) == std::mem::discriminant(&expected)),
            "expected error variant {:?}, got {errors:?}",
            expected
        ),
    }
}

#[test]
fn happy_truthful_manifest_passes_validation() {
    // arrange
    let reference = metadata_for(truthful_reference());
    let candidate = metadata_for(truthful_candidate());

    // act
    let result = validate_runner_pair(&reference, &candidate, Some(CANDIDATE_SHA));

    // assert
    result.unwrap_or_else(|errors| panic!("truthful pair must pass: {errors:?}"));
}

#[test]
fn missing_harness_bin_candidate_is_rejected() {
    // arrange — candidate metadata omits the runner_identity block entirely
    let reference = metadata_for(truthful_reference());
    let candidate = CaptureMetadata {
        runner_identity: None,
        dry_run: false,
        fields: std::collections::BTreeMap::new(),
    };

    // act
    let result = validate_runner_pair(&reference, &candidate, Some(CANDIDATE_SHA));

    // assert
    assert_has_error(result, RunnerIdentityError::MissingCandidateHarnessBin);
}

#[test]
fn missing_reference_identity_is_rejected() {
    // arrange
    let reference = CaptureMetadata {
        runner_identity: None,
        dry_run: false,
        fields: std::collections::BTreeMap::new(),
    };
    let candidate = metadata_for(truthful_candidate());

    // act
    let result = validate_runner_pair(&reference, &candidate, Some(CANDIDATE_SHA));

    // assert
    assert_has_error(
        result,
        RunnerIdentityError::MissingRunnerIdentity {
            kind: RunnerKind::Reference,
        },
    );
}

#[test]
fn mismatched_candidate_sha_is_rejected() {
    // arrange — captured candidate SHA differs from the expected HARNESS_BIN SHA
    let mut candidate = truthful_candidate();
    candidate.binary_sha256 = "0".repeat(64);

    let reference = metadata_for(truthful_reference());
    let candidate_metadata = metadata_for(candidate);

    // act
    let result = validate_runner_pair(&reference, &candidate_metadata, Some(CANDIDATE_SHA));

    // assert
    assert_has_error(
        result,
        RunnerIdentityError::MismatchedCandidateSha {
            expected: CANDIDATE_SHA.to_owned(),
            actual: "0".repeat(64),
        },
    );
}

#[test]
fn helper_binary_substitution_is_rejected() {
    // arrange — point HARNESS_BIN at the reference_parity_pty_test helper
    let mut candidate = truthful_candidate();
    candidate.binary_path =
        "/home/urbanbreach/Projects/agent-harness/target/debug/deps/reference_parity_pty_test-3c4657bf9589c841"
            .to_owned();

    let reference = metadata_for(truthful_reference());
    let candidate_metadata = metadata_for(candidate);

    // act
    let result = validate_runner_pair(&reference, &candidate_metadata, Some(CANDIDATE_SHA));

    // assert
    assert_has_error(
        result,
        RunnerIdentityError::HelperBinarySubstituted {
            kind: RunnerKind::Candidate,
            path: String::new(),
            marker: "reference_parity_pty_test".to_owned(),
        },
    );
}

#[test]
fn native_visual_helper_substitution_is_rejected() {
    // arrange — point HARNESS_BIN at the native_visual_helper helper binary
    let mut candidate = truthful_candidate();
    candidate.binary_path =
        "/home/urbanbreach/Projects/agent-harness/target/debug/native_visual_helper".to_owned();

    let reference = metadata_for(truthful_reference());
    let candidate_metadata = metadata_for(candidate);

    // act
    let result = validate_runner_pair(&reference, &candidate_metadata, Some(CANDIDATE_SHA));

    // assert
    assert_has_error(
        result,
        RunnerIdentityError::HelperBinarySubstituted {
            kind: RunnerKind::Candidate,
            path: String::new(),
            marker: "native_visual_helper".to_owned(),
        },
    );
}

#[test]
fn copied_reference_digest_is_rejected() {
    // arrange — candidate carries the reference binary's SHA while keeping a
    // distinct path; this is the "copied reference-digest seeding" attack.
    let mut candidate = truthful_candidate();
    candidate.binary_sha256 = REFERENCE_BINARY_SHA256.to_owned();

    let reference = metadata_for(truthful_reference());
    let candidate_metadata = metadata_for(candidate);

    // act
    let result = validate_runner_pair(&reference, &candidate_metadata, Some(CANDIDATE_SHA));

    // assert
    assert_has_error(result, RunnerIdentityError::CopiedReferenceDigest);
}

#[test]
fn identical_runner_paths_are_rejected() {
    // arrange — candidate and reference share the exact same path (a signoff
    // against the current process / cargo run path).
    let mut candidate = truthful_candidate();
    candidate.binary_path = REFERENCE_BINARY_ABSOLUTE_PATH.to_owned();
    candidate.binary_sha256 = REFERENCE_BINARY_SHA256.to_owned();
    candidate.binary_version = REFERENCE_BINARY_VERSION.to_owned();

    let reference = metadata_for(truthful_reference());
    let candidate_metadata = metadata_for(candidate);

    // act
    let result = validate_runner_pair(&reference, &candidate_metadata, Some(CANDIDATE_SHA));

    // assert
    assert_has_error(
        result,
        RunnerIdentityError::IdenticalRunnerPaths {
            path: REFERENCE_BINARY_ABSOLUTE_PATH.to_owned(),
        },
    );
}

#[test]
fn identical_runner_process_ids_are_rejected() {
    // arrange — distinct paths and SHAs but the same OS pid (current process
    // rendered both sides).
    let mut candidate = truthful_candidate();
    candidate.process_id = Some(1010);

    let reference = metadata_for(truthful_reference());
    let candidate_metadata = metadata_for(candidate);

    // act
    let result = validate_runner_pair(&reference, &candidate_metadata, Some(CANDIDATE_SHA));

    // assert
    assert_has_error(
        result,
        RunnerIdentityError::IdenticalRunnerProcessId { pid: 1010 },
    );
}

#[test]
fn dry_run_only_evidence_is_rejected() {
    // arrange — a candidate metadata records dry_run=true; signoff must not
    // accept wiring-only output as freshness evidence.
    let reference = metadata_for(truthful_reference());
    let mut candidate = metadata_for(truthful_candidate());
    candidate.dry_run = true;

    // act
    let result = validate_runner_pair(&reference, &candidate, Some(CANDIDATE_SHA));

    // assert
    assert_has_error(result, RunnerIdentityError::DryRunOnlyEvidence);
}

#[test]
fn reference_dry_run_only_evidence_is_rejected() {
    // arrange — reference side is dry-run
    let mut reference = metadata_for(truthful_reference());
    reference.dry_run = true;
    let candidate = metadata_for(truthful_candidate());

    // act
    let result = validate_runner_pair(&reference, &candidate, Some(CANDIDATE_SHA));

    // assert
    assert_has_error(result, RunnerIdentityError::DryRunOnlyEvidence);
}

#[test]
fn relative_candidate_path_is_rejected() {
    // arrange — candidate path is relative (cargo run / current-process)
    let mut candidate = truthful_candidate();
    candidate.binary_path = "target/debug/harness-parity-candidate".to_owned();

    let reference = metadata_for(truthful_reference());
    let candidate_metadata = metadata_for(candidate);

    // act
    let result = validate_runner_pair(&reference, &candidate_metadata, Some(CANDIDATE_SHA));

    // assert
    assert_has_error(
        result,
        RunnerIdentityError::RelativeBinaryPath {
            kind: RunnerKind::Candidate,
            path: "target/debug/harness-parity-candidate".to_owned(),
        },
    );
}

#[test]
fn reference_path_mismatch_is_rejected() {
    // arrange — reference identity points at a non-pinned path
    let mut reference = truthful_reference();
    reference.binary_path =
        "/home/urbanbreach/Projects/agent-harness/target/debug/xai-grok-pager".to_owned();

    let reference_metadata = metadata_for(reference);
    let candidate = metadata_for(truthful_candidate());

    // act
    let result = validate_runner_pair(&reference_metadata, &candidate, Some(CANDIDATE_SHA));

    // assert
    assert_has_error(
        result,
        RunnerIdentityError::ReferenceBinaryPathMismatch {
            expected: REFERENCE_BINARY_ABSOLUTE_PATH,
            actual: String::new(),
        },
    );
}

#[test]
fn reference_sha_mismatch_is_rejected() {
    // arrange — reference identity carries a wrong pinned sha
    let mut reference = truthful_reference();
    reference.binary_sha256 = "f".repeat(64);

    let reference_metadata = metadata_for(reference);
    let candidate = metadata_for(truthful_candidate());

    // act
    let result = validate_runner_pair(&reference_metadata, &candidate, Some(CANDIDATE_SHA));

    // assert
    assert_has_error(
        result,
        RunnerIdentityError::ReferenceBinaryShaMismatch {
            expected: REFERENCE_BINARY_SHA256,
            actual: String::new(),
        },
    );
}

#[test]
fn reference_version_mismatch_is_rejected() {
    // arrange — reference identity carries a wrong version string
    let mut reference = truthful_reference();
    reference.binary_version = "grok 0.1.219".to_owned();

    let reference_metadata = metadata_for(reference);
    let candidate = metadata_for(truthful_candidate());

    // act
    let result = validate_runner_pair(&reference_metadata, &candidate, Some(CANDIDATE_SHA));

    // assert
    assert_has_error(
        result,
        RunnerIdentityError::ReferenceBinaryVersionMismatch {
            expected: REFERENCE_BINARY_VERSION,
            actual: String::new(),
        },
    );
}

#[test]
fn missing_required_identity_field_is_rejected() {
    // arrange — candidate identity omits permissions
    let mut candidate = truthful_candidate();
    candidate.permissions = String::new();

    let reference = metadata_for(truthful_reference());
    let candidate_metadata = metadata_for(candidate);

    // act
    let result = validate_runner_pair(&reference, &candidate_metadata, Some(CANDIDATE_SHA));

    // assert
    assert_has_error(
        result,
        RunnerIdentityError::MissingIdentityField {
            kind: RunnerKind::Candidate,
            field: "permissions",
        },
    );
}

#[test]
fn malformed_candidate_sha_is_rejected() {
    // arrange — candidate identity carries a malformed sha
    let mut candidate = truthful_candidate();
    candidate.binary_sha256 = "not-a-sha".to_owned();

    let reference = metadata_for(truthful_reference());
    let candidate_metadata = metadata_for(candidate);

    // act
    let result = validate_runner_pair(&reference, &candidate_metadata, Some(CANDIDATE_SHA));

    // assert
    assert_has_error(
        result,
        RunnerIdentityError::MissingIdentityField {
            kind: RunnerKind::Candidate,
            field: "binary_sha256_format",
        },
    );
}

#[test]
fn cargo_run_marker_in_generating_command_is_rejected() {
    // arrange — capture metadata records `cargo run` as the generating command
    let mut fields = std::collections::BTreeMap::new();
    fields.insert(
        "generating_command".to_owned(),
        "cargo run -p harness -- prompt".to_owned(),
    );
    let metadata = CaptureMetadata {
        runner_identity: None,
        dry_run: false,
        fields,
    };

    // act
    let result = reject_cargo_run_markers(&metadata);

    // assert
    assert!(matches!(
        result,
        Err(RunnerIdentityError::CargoRunSelfRendering { .. })
    ));
}

#[test]
fn cargo_run_plus_marker_is_rejected() {
    // arrange — capture metadata records `cargo +nightly run`
    let mut fields = std::collections::BTreeMap::new();
    fields.insert(
        "generating_command".to_owned(),
        "cargo +nightly run -- prompt".to_owned(),
    );
    let metadata = CaptureMetadata {
        runner_identity: None,
        dry_run: false,
        fields,
    };

    // act
    let result = reject_cargo_run_markers(&metadata);

    // assert
    assert!(matches!(
        result,
        Err(RunnerIdentityError::CargoRunSelfRendering { .. })
    ));
}
