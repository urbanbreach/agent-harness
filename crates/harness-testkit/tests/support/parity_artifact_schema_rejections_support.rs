use super::*;

// ---------------------------------------------------------------------------
// Stale-root rejection
// ---------------------------------------------------------------------------

#[test]
fn reject_stale_root_path_in_receipt() {
    // arrange
    let mut receipt = fixture_receipt();
    // A stale root is one that does not match the expected isolation root
    // prefix pattern. The validator rejects isolation roots that are not
    // absolute paths under /tmp or a configured workspace root.
    receipt.isolation_root = "relative/path/without/abs/root".to_owned();
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(
        result.rejected_fields.iter().any(|f| f == "isolation_root"),
        "expected isolation_root in rejected fields: {:?}",
        result.rejected_fields
    );
}

// ---------------------------------------------------------------------------
// Secret-bearing value rejection
// ---------------------------------------------------------------------------

#[test]
fn reject_secret_in_binary_digest() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.binary_digest = "api_key=sk-secret123".to_owned();
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(
        result.rejected_fields.iter().any(|f| f == "binary_digest"),
        "expected binary_digest in rejected fields: {:?}",
        result.rejected_fields
    );
}

#[test]
fn reject_secret_in_command() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.command = "OPENAI_API_KEY=sk-test run harness".to_owned();
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(
        result.rejected_fields.iter().any(|f| f == "command"),
        "expected command in rejected fields: {:?}",
        result.rejected_fields
    );
}

#[test]
fn reject_secret_in_source_revision() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.source_revision = "bearer=token123".to_owned();
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(
        result
            .rejected_fields
            .iter()
            .any(|f| f == "source_revision"),
        "expected source_revision in rejected fields: {:?}",
        result.rejected_fields
    );
}

#[test]
fn reject_secret_in_isolation_root() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.isolation_root = "/tmp/password=hunter2".to_owned();
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(
        result.rejected_fields.iter().any(|f| f == "isolation_root"),
        "expected isolation_root in rejected fields: {:?}",
        result.rejected_fields
    );
}

#[test]
fn reject_secret_in_workspace_before_digest() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.workspace_before.digest = "secret=abc123".to_owned();
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(
        result
            .rejected_fields
            .iter()
            .any(|f| f == "workspace_before"),
        "expected workspace_before in rejected fields: {:?}",
        result.rejected_fields
    );
}

#[test]
fn reject_secret_in_workspace_after_digest() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.workspace_after.digest = "authorization=bearer".to_owned();
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(
        result
            .rejected_fields
            .iter()
            .any(|f| f == "workspace_after"),
        "expected workspace_after in rejected fields: {:?}",
        result.rejected_fields
    );
}

// ---------------------------------------------------------------------------
// Teardown failure rejection
// ---------------------------------------------------------------------------

#[test]
fn reject_teardown_nonzero_exit() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.teardown.exit_code = 1;
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(
        result.rejected_fields.iter().any(|f| f == "teardown"),
        "expected teardown in rejected fields: {:?}",
        result.rejected_fields
    );
}

// ---------------------------------------------------------------------------
// Multiple simultaneous failures
// ---------------------------------------------------------------------------

#[test]
fn reject_multiple_missing_fields_reports_all() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.binary_digest.clear();
    receipt.source_revision.clear();
    receipt.command.clear();
    receipt.isolation_root.clear();
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(result
        .required_fields_missing
        .iter()
        .any(|f| f == "binary_digest"));
    assert!(result
        .required_fields_missing
        .iter()
        .any(|f| f == "source_revision"));
    assert!(result
        .required_fields_missing
        .iter()
        .any(|f| f == "command"));
    assert!(result
        .required_fields_missing
        .iter()
        .any(|f| f == "isolation_root"));
    assert_eq!(result.required_fields_missing.len(), 4);
}

// ---------------------------------------------------------------------------
// Epoch binding rejection
// ---------------------------------------------------------------------------

#[test]
fn reject_missing_product_epoch() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.epoch.product_epoch.clear();
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(result
        .required_fields_missing
        .iter()
        .any(|f| f == "epoch.product_epoch"));
}

#[test]
fn reject_missing_reference_epoch() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.epoch.reference_epoch.clear();
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(result
        .required_fields_missing
        .iter()
        .any(|f| f == "epoch.reference_epoch"));
}

// ---------------------------------------------------------------------------
// Reference identity rejection
// ---------------------------------------------------------------------------

#[test]
fn reject_missing_reference_identity() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.reference.path.clear();
    receipt.reference.sha256.clear();
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(result
        .required_fields_missing
        .iter()
        .any(|f| f == "reference.path"));
    assert!(result
        .required_fields_missing
        .iter()
        .any(|f| f == "reference.sha256"));
}

#[test]
fn reject_relative_reference_path() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.reference.path = "relative/path/binary".to_owned();
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(result.rejected_fields.iter().any(|f| f == "reference"));
}

#[test]
fn reject_secret_in_reference_path() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.reference.path = "/tmp/api_key=leak".to_owned();
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(result.rejected_fields.iter().any(|f| f == "reference"));
}

// ---------------------------------------------------------------------------
// Proof dimension rejection
// ---------------------------------------------------------------------------

#[test]
fn reject_empty_proof_dimensions() {
    // arrange
    let mut receipt = fixture_receipt();
    receipt.proof_dimensions.clear();
    // act
    let result = receipt.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(result
        .required_fields_missing
        .iter()
        .any(|f| f == "proof_dimensions"));
}

#[test]
fn pass_status_with_dimensions_rejects_incomplete_dimensions() {
    // arrange
    let receipt = fixture_receipt();
    let applicable = applicable_dimensions_for("visual");
    let mut present = applicable.clone();
    present.remove(&ProofDimension::P6);
    let completeness = check_dimension_completeness(&applicable, &present);

    // act
    let result = validate_pass_status_with_dimensions(&completeness, &receipt);
    // assert
    assert!(
        result.is_err(),
        "pass status must reject incomplete proof dimensions"
    );
}

#[test]
fn pass_status_with_dimensions_accepts_complete_dimensions() {
    // arrange
    let receipt = fixture_receipt();
    let applicable = applicable_dimensions_for("visual");
    let present = applicable.clone();
    let completeness = check_dimension_completeness(&applicable, &present);

    // act
    let result = validate_pass_status_with_dimensions(&completeness, &receipt);
    // assert
    assert!(
        result.is_ok(),
        "pass status must accept complete proof dimensions: {result:?}"
    );
}

#[test]
fn receipt_v2_json_migrates_with_defaults_but_fails_validation() {
    // arrange
    let receipt = fixture_receipt();
    let mut value = serde_json::to_value(&receipt).expect("serialize");
    let obj = value.as_object_mut().expect("object");
    obj.remove("reference");
    obj.remove("epoch");
    obj.remove("proof_dimensions");

    let migrated: ArtifactReceipt =
        serde_json::from_value(value).expect("v2 JSON deserializes with defaults");
    // act
    let result = migrated.validate();

    // assert
    assert_eq!(result.outcome, ValidationOutcome::Fail);
    assert!(result
        .required_fields_missing
        .iter()
        .any(|f| f == "epoch.product_epoch"));
    assert!(result
        .required_fields_missing
        .iter()
        .any(|f| f == "reference.path"));
    assert!(result
        .required_fields_missing
        .iter()
        .any(|f| f == "proof_dimensions"));
}
