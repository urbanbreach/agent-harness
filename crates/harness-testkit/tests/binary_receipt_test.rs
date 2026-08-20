use harness_testkit::binary_receipt::{
    BinaryIdentity, BinaryReceipt, BinaryReceiptError, BuildFingerprint, MutationProbeReceipt,
    ReceiptExpectations, RepeatBuildReceipt,
};
use harness_testkit::UnwrapOrAbort;
use std::path::Path;

#[path = "support/harness_bin.rs"]
mod harness_bin;

const REFERENCE_REVISION: &str = "eb267feff13129e568df38fb6fdf0ceb65f735d6";
const HARNESS_REVISION: &str = "harness-test-revision";

#[test]
fn binary_receipt_accepts_matching_repeat_identity() {
    // arrange
    // Given
    let receipt = fixture_receipt();

    // When
    // act
    let result = receipt.verify(&fixture_expectations());

    // Then
    // assert
    assert!(result.is_ok(), "matching receipt rejected: {result:?}");
}

#[test]
fn binary_receipt_rejects_wrong_reference_revision() {
    // arrange
    // Given
    let mut receipt = fixture_receipt();
    receipt.reference.source_revision = "wrong-revision".to_owned();

    // When
    // act
    let result = receipt.verify(&fixture_expectations());

    // Then
    // assert
    assert!(matches!(
        result,
        Err(BinaryReceiptError::Mismatch { field, .. }) if field == "reference.source_revision"
    ));
}

#[test]
fn binary_receipt_rejects_repeat_digest_drift() {
    // arrange
    // Given
    let mut receipt = fixture_receipt();
    receipt.reference_repeat.second.binary_sha256 = "f".repeat(64);

    // When
    // act
    let result = receipt.verify(&fixture_expectations());

    // Then
    // assert
    assert!(matches!(
        result,
        Err(BinaryReceiptError::InvalidField { field, .. }) if field == "reference_repeat.matching"
    ));
}

#[test]
fn binary_receipt_rejects_mutated_binary_digest() {
    // arrange
    // Given
    let temporary = tempfile::tempdir().unwrap_or_abort();
    let binary = temporary.path().join("harness");
    std::fs::write(&binary, b"immutable binary").unwrap_or_abort();
    let mut receipt = fixture_receipt();
    receipt.reference.binary_path = binary.display().to_string();
    receipt.reference.sha256 = "0".repeat(64);

    // When
    // act
    let result = receipt.verify_binary_digests();

    // Then
    // assert
    assert!(matches!(
        result,
        Err(BinaryReceiptError::DigestMismatch { field, .. }) if field == "reference.sha256"
    ));
}

#[test]
fn binary_receipt_rejects_mutated_repeat_binary_digest() {
    // arrange
    // Given
    let temporary = tempfile::tempdir().unwrap_or_abort();
    let reference_first = temporary.path().join("reference-first");
    let reference_second = temporary.path().join("reference-second");
    let harness_first = temporary.path().join("harness-first");
    let harness_second = temporary.path().join("harness-second");
    for path in [
        &reference_first,
        &reference_second,
        &harness_first,
        &harness_second,
    ] {
        std::fs::write(path, b"immutable binary").unwrap_or_abort();
    }
    let mut receipt = fixture_receipt();
    receipt.reference.binary_path = reference_first.display().to_string();
    receipt.reference.sha256 = sha256sum(&reference_first);
    receipt.reference_repeat.first_binary_path = reference_first.display().to_string();
    receipt.reference_repeat.second_binary_path = reference_second.display().to_string();
    receipt.reference_repeat.first.binary_sha256 = sha256sum(&reference_first);
    receipt.reference_repeat.second.binary_sha256 = sha256sum(&reference_second);
    receipt.harness.binary_path = harness_first.display().to_string();
    receipt.harness.sha256 = sha256sum(&harness_first);
    receipt.harness_repeat.first_binary_path = harness_first.display().to_string();
    receipt.harness_repeat.second_binary_path = harness_second.display().to_string();
    receipt.harness_repeat.first.binary_sha256 = sha256sum(&harness_first);
    receipt.harness_repeat.second.binary_sha256 = sha256sum(&harness_second);
    std::fs::write(&reference_second, b"mutated repeat binary").unwrap_or_abort();

    // When
    // act
    let result = receipt.verify_binary_digests();

    // Then
    // assert
    assert!(matches!(
        result,
        Err(BinaryReceiptError::DigestMismatch { field, .. })
            if field == "reference_repeat.second.binary_sha256"
    ));
}

fn fixture_expectations() -> ReceiptExpectations {
    ReceiptExpectations {
        reference_revision: REFERENCE_REVISION.to_owned(),
        harness_revision: HARNESS_REVISION.to_owned(),
        reference_clean_pre: true,
        reference_clean_post: true,
        harness_clean_pre: true,
        harness_clean_post: true,
        reference_package: "xai-grok-pager-bin".to_owned(),
        reference_executable: "xai-grok-pager".to_owned(),
        harness_package: "harness".to_owned(),
        harness_executable: "harness".to_owned(),
    }
}

fn fixture_receipt() -> BinaryReceipt {
    let reference = fixture_identity(
        REFERENCE_REVISION,
        "xai-grok-pager-bin",
        "xai-grok-pager",
        "/tmp/reference-target/debug/xai-grok-pager",
        "a",
    );
    let harness = fixture_identity(
        HARNESS_REVISION,
        "harness",
        "harness",
        "/tmp/harness-target/debug/harness",
        "b",
    );

    BinaryReceipt {
        schema_version: "harness.tui-fidelity.binary-build.v1".to_owned(),
        reference,
        harness,
        reference_repeat: fixture_repeat(
            REFERENCE_REVISION,
            "/tmp/reference-target/debug/xai-grok-pager",
            "a",
        ),
        harness_repeat: fixture_repeat(HARNESS_REVISION, "/tmp/harness-target/debug/harness", "b"),
        mutation_probe: MutationProbeReceipt {
            wrong_revision_rejected: true,
            mutated_digest_rejected: true,
        },
    }
}

fn fixture_identity(
    source_revision: &str,
    package: &str,
    executable: &str,
    binary_path: &str,
    digest_byte: &str,
) -> BinaryIdentity {
    BinaryIdentity {
        source_revision: source_revision.to_owned(),
        clean_pre: true,
        clean_post: true,
        source_sha256: format_digest('c'),
        package: package.to_owned(),
        executable: executable.to_owned(),
        target_dir: binary_path
            .rsplit_once('/')
            .map(|(path, _)| path.to_owned())
            .unwrap_or_else(|| "/tmp/target".to_owned()),
        binary_path: binary_path.to_owned(),
        version: "binary 0.1.0".to_owned(),
        sha256: format_digest(digest_byte.chars().next().unwrap_or('0')),
        cargo_lock_sha256: format_digest('d'),
        toolchain_sha256: format_digest('e'),
        rustc_version: "rustc test".to_owned(),
        rustc_sha256: format_digest('f'),
        cargo_version: "cargo test".to_owned(),
        cargo_sha256: format_digest('1'),
    }
}

fn fixture_repeat(
    source_revision: &str,
    binary_path: &str,
    digest_byte: &str,
) -> RepeatBuildReceipt {
    let fingerprint = BuildFingerprint {
        source_revision: source_revision.to_owned(),
        source_sha256: format_digest('c'),
        cargo_lock_sha256: format_digest('d'),
        toolchain_sha256: format_digest('e'),
        rustc_sha256: format_digest('f'),
        cargo_sha256: format_digest('1'),
        binary_sha256: format_digest(digest_byte.chars().next().unwrap_or('0')),
        version: "binary 0.1.0".to_owned(),
    };
    RepeatBuildReceipt {
        first: fingerprint.clone(),
        second: fingerprint,
        first_target_dir: binary_path
            .rsplit_once('/')
            .map(|(path, _)| path.to_owned())
            .unwrap_or_else(|| "/tmp/target".to_owned()),
        second_target_dir: "/tmp/repeat-target/debug".to_owned(),
        first_binary_path: binary_path.to_owned(),
        second_binary_path: "/tmp/repeat-target/debug/binary".to_owned(),
        matching: true,
    }
}

fn format_digest(byte: char) -> String {
    byte.to_string().repeat(64)
}

fn sha256sum(path: &Path) -> String {
    let output = harness_bin::command("sha256sum")
        .arg(path)
        .output()
        .unwrap_or_abort();
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .map(str::to_owned)
        .unwrap_or_abort()
}
