//! Fail-closed schema validation tests for live-provider and dogfood journey
//! artifact receipts (Todo 12).
//!
//! These tests prove the validator accepts a complete, independently
//! constructed fixture and rejects every required-field omission, stale-root,
//! and secret-bearing value. The fixture values are hand-authored and do NOT
//! derive from validator output.
// allow: SIZE_OK — one complete artifact receipt mutation matrix shares one fixture.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "schema tests use fail-fast asserts"
)]

use harness_testkit::parity::artifact_schema::{
    ArtifactReceipt, AuthMode, CandidateIdentity, EpochBindings, EvidenceIdentity, JourneyReceipt,
    ProviderMode, ReferenceIdentity, RunnerIdentity, SecretScanResult, TeardownReceipt,
    ValidationOutcome, WorkspaceState,
};
use harness_testkit::parity::status::{
    applicable_dimensions_for, applicable_layers_for, check_dimension_completeness,
    check_layer_completeness, validate_pass_status, validate_pass_status_with_dimensions,
    ProofDimension,
};
use harness_testkit::parity::{validate_no_self_comparison, CaptureSource};

// ---------------------------------------------------------------------------
// Independently constructed fixture values (NOT derived from validator output)
// ---------------------------------------------------------------------------

const FIXTURE_BINARY_DIGEST: &str =
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const FIXTURE_SOURCE_REVISION: &str =
    "5398297de4f3210c39655d4c620096267cccfb8c4e79c582232e10f42e6e1af5";
const FIXTURE_COMMAND: &str =
    "/tmp/agent-harness-wt-6/harness --config harness.jsonc prompt --mock fixture";
const FIXTURE_ISOLATION_ROOT: &str = "/tmp/agent-harness-wt-6";
const FIXTURE_RUNNER_PATH: &str = "/tmp/agent-harness-wt-6/harness";
const FIXTURE_EVIDENCE_ROOT: &str = "/tmp/harness-evidence/attempt-2/task-4";
const FIXTURE_ARTIFACT_PATH: &str = "/tmp/harness-evidence/attempt-2/task-4/receipt.json";
const FIXTURE_WORKSPACE_BEFORE_DIGEST: &str =
    "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2";
const FIXTURE_WORKSPACE_AFTER_DIGEST: &str =
    "f1e2d3c4b5a6f7e8d9c0b1a2f3e4d5c6b7a8f9e0d1c2b3a4f5e6d7c8b9a0f1e2";
const FIXTURE_TEARDOWN_EXIT_CODE: i32 = 0;
const FIXTURE_TEARDOWN_REMOVED_PATHS: &[&str] = &["/tmp/agent-harness-wt-6/target/tmp-journey-1"];
const FIXTURE_SECRET_SCAN_PATTERNS: &[&str] =
    &["api_key", "bearer", "authorization", "password", "secret"];
const FIXTURE_REFERENCE_PATH: &str = "/tmp/grok-build/target/debug/xai-grok-pager";
const FIXTURE_REFERENCE_SHA256: &str =
    "883e3dea2a57773f3a9b229746ff7a99b9761836401e0f022599914b3bb9a9a5";
const FIXTURE_PRODUCT_EPOCH: &str =
    "b4c9a289323b21a01c3e940f150eb9b8c542587f1abfd8f0e1cc1ffc5e475514";
const FIXTURE_REFERENCE_EPOCH: &str =
    "d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592";

fn fixture_secret_scan_clean() -> SecretScanResult {
    SecretScanResult {
        clean: true,
        patterns_checked: FIXTURE_SECRET_SCAN_PATTERNS
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
        findings: Vec::new(),
    }
}

fn fixture_teardown() -> TeardownReceipt {
    TeardownReceipt {
        exit_code: FIXTURE_TEARDOWN_EXIT_CODE,
        removed_paths: FIXTURE_TEARDOWN_REMOVED_PATHS
            .iter()
            .map(|p| (*p).to_owned())
            .collect(),
        workspace_restored: true,
    }
}

fn fixture_workspace_before() -> WorkspaceState {
    WorkspaceState {
        digest: FIXTURE_WORKSPACE_BEFORE_DIGEST.to_owned(),
        file_count: 42,
    }
}

fn fixture_workspace_after() -> WorkspaceState {
    WorkspaceState {
        digest: FIXTURE_WORKSPACE_AFTER_DIGEST.to_owned(),
        file_count: 43,
    }
}

fn fixture_receipt() -> ArtifactReceipt {
    ArtifactReceipt {
        binary_digest: FIXTURE_BINARY_DIGEST.to_owned(),
        source_revision: FIXTURE_SOURCE_REVISION.to_owned(),
        command: FIXTURE_COMMAND.to_owned(),
        provider_mode: ProviderMode::Offline,
        auth_mode: AuthMode::None,
        workspace_before: fixture_workspace_before(),
        workspace_after: fixture_workspace_after(),
        teardown: fixture_teardown(),
        isolation_root: FIXTURE_ISOLATION_ROOT.to_owned(),
        secret_scan: fixture_secret_scan_clean(),
        owner: "task-4".to_owned(),
        candidate: CandidateIdentity {
            source_revision: FIXTURE_SOURCE_REVISION.to_owned(),
            binary_digest: FIXTURE_BINARY_DIGEST.to_owned(),
        },
        runner: RunnerIdentity {
            path: FIXTURE_RUNNER_PATH.to_owned(),
            sha256: FIXTURE_BINARY_DIGEST.to_owned(),
            version: "harness 0.1.0".to_owned(),
            permissions: "755".to_owned(),
        },
        evidence: EvidenceIdentity {
            attempt_id: "attempt-2".to_owned(),
            task_id: 4,
            root: FIXTURE_EVIDENCE_ROOT.to_owned(),
            artifact_path: FIXTURE_ARTIFACT_PATH.to_owned(),
            artifact_sha256: FIXTURE_WORKSPACE_AFTER_DIGEST.to_owned(),
            fresh_root: true,
        },
        reference: ReferenceIdentity {
            path: FIXTURE_REFERENCE_PATH.to_owned(),
            sha256: FIXTURE_REFERENCE_SHA256.to_owned(),
        },
        epoch: EpochBindings {
            product_epoch: FIXTURE_PRODUCT_EPOCH.to_owned(),
            reference_epoch: FIXTURE_REFERENCE_EPOCH.to_owned(),
        },
        proof_dimensions: ProofDimension::all().into(),
    }
}

// ---------------------------------------------------------------------------
// GREEN: happy path — complete fixture passes
// ---------------------------------------------------------------------------

#[test]
fn happy_complete_fixture_passes_validation() {
    // arrange
    let receipt = fixture_receipt();
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Pass);
    assert!(result.required_fields_missing.is_empty());
    assert!(result.rejected_fields.is_empty());
    assert!(result.secret_scan_clean);
}

#[test]
fn happy_validator_produces_machine_readable_json() {
    // arrange
    let receipt = fixture_receipt();
    let result = receipt.validate();
    let json = result.to_json_string().expect("to_json_string");

    // The JSON must be parseable and contain the expected outcome.
    // act
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    // assert
    assert_eq!(parsed["outcome"], "pass");
    assert!(parsed["required_fields_missing"]
        .as_array()
        .expect("array")
        .is_empty());
    assert_eq!(parsed["secret_scan_clean"], true);
}

#[test]
fn happy_journey_receipt_wraps_artifact_receipt() {
    // arrange
    let artifact = fixture_receipt();
    let journey = JourneyReceipt {
        journey_id: "journey-001".to_owned(),
        provider_mode: ProviderMode::Offline,
        auth_mode: AuthMode::None,
        artifact: artifact.clone(),
    };
    // act
    let result = journey.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Pass);
    assert!(result.secret_scan_clean);
}

#[test]
fn receipt_deserialization_rejects_missing_immutable_provenance() {
    // arrange
    let receipt = fixture_receipt();
    let mut value = serde_json::to_value(receipt).expect("receipt serializes");
    let object = value.as_object_mut().expect("receipt is an object");
    object.remove("candidate");
    object.remove("evidence");
    object.remove("owner");
    object.remove("runner");

    // act
    let parsed = serde_json::from_value::<ArtifactReceipt>(value);

    // assert
    assert!(
        parsed.is_err(),
        "a receipt without candidate, evidence, owner, and runner identity must be rejected"
    );
}

#[test]
fn journey_receipt_rejects_contradictory_provider_mode() {
    // arrange
    let journey = JourneyReceipt {
        journey_id: "journey-contradiction".to_owned(),
        provider_mode: ProviderMode::Live,
        auth_mode: AuthMode::ApiKey,
        artifact: fixture_receipt(),
    };

    // act
    let result = journey.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(
        result
            .rejected_fields
            .iter()
            .any(|field| field == "provider_mode"),
        "provider mode mismatch must be rejected: {:?}",
        result.rejected_fields
    );
}

#[test]
fn pass_status_rejects_clean_secret_scan_with_findings() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt
        .secret_scan
        .findings
        .push(harness_testkit::parity::artifact_schema::SecretFinding {
            field: "command".to_owned(),
            pattern: "token".to_owned(),
            snippet: "[redacted]".to_owned(),
        });
    let layers = applicable_layers_for("visual");
    let completeness = check_layer_completeness(&layers, &layers);

    // act
    let validation = validate_pass_status(&completeness, &receipt);

    // assert
    assert!(
        validation.is_err(),
        "pass status must reject a contradictory clean secret scan"
    );
}

#[test]
fn receipt_rejects_copied_artifact_outside_its_task_root() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.evidence.artifact_path =
        "/tmp/harness-evidence/attempt-2/task-5/copied.json".to_owned();

    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(result
        .rejected_fields
        .iter()
        .any(|field| field == "evidence"));
}

#[test]
fn receipt_rejects_changed_runner_identity() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.runner.sha256 = "changed-runner-digest".to_owned();

    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(result.rejected_fields.iter().any(|field| field == "runner"));
}

#[test]
fn receipt_rejects_wrong_candidate_revision() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.candidate.source_revision = "wrong-revision".to_owned();

    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(result
        .rejected_fields
        .iter()
        .any(|field| field == "candidate"));
}

#[test]
fn receipt_rejects_missing_task_evidence() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.evidence.artifact_path.clear();

    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(result
        .required_fields_missing
        .iter()
        .any(|field| field == "evidence.artifact_path"));
}

#[test]
fn receipt_rejects_post_hoc_provenance() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.evidence.fresh_root = false;

    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(result
        .required_fields_missing
        .iter()
        .any(|field| field == "evidence.fresh_root"));
}

#[test]
fn comparison_rejects_self_comparison() {
    // arrange
    let source = CaptureSource::Harness;

    // act
    let result = validate_no_self_comparison(source, source);

    // assert
    assert!(
        result.is_err(),
        "a candidate must not compare against itself"
    );
}

// ---------------------------------------------------------------------------
// RED: rejection tests — each required field omission is caught
// ---------------------------------------------------------------------------

#[test]
fn reject_missing_binary_digest() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.binary_digest.clear();
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(
        result
            .required_fields_missing
            .iter()
            .any(|f| f == "binary_digest"),
        "expected binary_digest in missing fields: {:?}",
        result.required_fields_missing
    );
}

#[test]
fn reject_missing_source_revision() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.source_revision.clear();
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(
        result
            .required_fields_missing
            .iter()
            .any(|f| f == "source_revision"),
        "expected source_revision in missing fields: {:?}",
        result.required_fields_missing
    );
}

#[test]
fn reject_missing_command_field_in_receipt() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.command.clear();
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(
        result
            .required_fields_missing
            .iter()
            .any(|f| f == "command"),
        "expected command in missing fields: {:?}",
        result.required_fields_missing
    );
}

#[test]
fn reject_missing_provider_mode_via_default_marker() {
    // arrange
    let mut receipt = fixture_receipt();
    // ProviderMode is an enum; simulate "missing" by setting it to an
    // explicit Unknown marker that the validator must reject.
    receipt.provider_mode = ProviderMode::Unknown;
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(
        result
            .required_fields_missing
            .iter()
            .any(|f| f == "provider_mode"),
        "expected provider_mode in missing fields: {:?}",
        result.required_fields_missing
    );
}

#[test]
fn reject_missing_auth_mode_via_default_marker() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.auth_mode = AuthMode::Unknown;
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(
        result
            .required_fields_missing
            .iter()
            .any(|f| f == "auth_mode"),
        "expected auth_mode in missing fields: {:?}",
        result.required_fields_missing
    );
}

#[test]
fn reject_missing_workspace_before_state() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.workspace_before.digest.clear();
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(
        result
            .required_fields_missing
            .iter()
            .any(|f| f == "workspace_before"),
        "expected workspace_before in missing fields: {:?}",
        result.required_fields_missing
    );
}

#[test]
fn reject_missing_workspace_after_state() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.workspace_after.digest.clear();
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(
        result
            .required_fields_missing
            .iter()
            .any(|f| f == "workspace_after"),
        "expected workspace_after in missing fields: {:?}",
        result.required_fields_missing
    );
}

#[test]
fn reject_missing_teardown_receipt() {
    // arrange
    let mut receipt = fixture_receipt();
    // Simulate missing teardown by clearing the removed_paths and marking
    // workspace_restored=false; the validator must reject an empty teardown.
    receipt.teardown.removed_paths.clear();
    receipt.teardown.workspace_restored = false;
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(
        result
            .required_fields_missing
            .iter()
            .any(|f| f == "teardown"),
        "expected teardown in missing fields: {:?}",
        result.required_fields_missing
    );
}

#[test]
fn reject_missing_isolation_root() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.isolation_root.clear();
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(
        result
            .required_fields_missing
            .iter()
            .any(|f| f == "isolation_root"),
        "expected isolation_root in missing fields: {:?}",
        result.required_fields_missing
    );
}

#[test]
fn reject_missing_secret_scan() {
    // arrange
    let mut receipt = fixture_receipt();
    // Simulate missing secret scan by clearing patterns_checked — the
    // validator must reject a scan that checked zero patterns.
    receipt.secret_scan.patterns_checked.clear();
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(
        result
            .required_fields_missing
            .iter()
            .any(|f| f == "secret_scan"),
        "expected secret_scan in missing fields: {:?}",
        result.required_fields_missing
    );
}

#[test]
fn reject_failed_secret_scan() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.secret_scan.clean = false;
    receipt
        .secret_scan
        .findings
        .push(harness_testkit::parity::artifact_schema::SecretFinding {
            field: "binary_digest".to_owned(),
            pattern: "api_key".to_owned(),
            snippet: "api_key=sk-test123".to_owned(),
        });
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(!result.secret_scan_clean);
}

#[path = "support/parity_artifact_schema_rejections_support.rs"]
mod artifact_schema_rejections;
