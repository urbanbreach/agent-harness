use harness_testkit::UnwrapOrAbort;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[path = "support/repo_root.rs"]
mod repo_root;

use repo_root::repo_root;

const PINNED_REVISION: &str = "be713136d2a69080743a3f6b3c72077057e5948f";

#[test]
fn source_guard_accepts_clean_pinned_reference() {
    // Given
    let reference = canonical_reference();

    // When
    let output = verify(&reference, PINNED_REVISION, &[]);

    // Then
    assert_success(&output);
}

#[test]
fn source_guard_accepts_relative_canonical_reference() {
    // Given
    let reference = PathBuf::from("inspirations/grok-build");

    // When
    let output = verify(&reference, PINNED_REVISION, &[]);

    // Then
    assert_success(&output);
}

#[test]
fn source_guard_accepts_current_manifest_input() {
    // Given
    let reference = canonical_reference();

    // When
    let output = verify(
        &reference,
        PINNED_REVISION,
        &["--input-root".into(), PathBuf::from("Cargo.toml").into()],
    );

    // Then
    assert_success(&output);
}

#[test]
fn source_guard_accepts_current_code_input() {
    // Given
    let reference = canonical_reference();
    let source = repo_root().join("crates/harness-testkit/tests/source_guard_test.rs");

    // When
    let output = verify(
        &reference,
        PINNED_REVISION,
        &["--input-root".into(), source.into()],
    );

    // Then
    assert_success(&output);
}

#[test]
fn source_guard_accepts_fresh_runtime_output() {
    // Given
    let reference = canonical_reference();
    let evidence_root = repo_root().join(".omo/evidence/task-1-grok-build-tui-experiential-parity");
    std::fs::create_dir_all(&evidence_root).unwrap_or_abort();
    let output_root = tempfile::tempdir_in(&evidence_root).unwrap_or_abort();

    // When
    let output = verify(
        &reference,
        PINNED_REVISION,
        &[
            "--input-root".into(),
            output_root.path().as_os_str().to_owned(),
        ],
    );

    // Then
    assert_success(&output);
}

#[test]
fn source_guard_rejects_wrong_revision() {
    // Given
    let reference = canonical_reference();

    // When
    let output = verify(&reference, "0000000000000000000000000000000000000000", &[]);

    // Then
    assert_failure(&output, "revision");
}

#[test]
fn source_guard_rejects_unapproved_input_root() {
    // Given
    let reference = canonical_reference();
    let unapproved = tempfile::tempdir().unwrap_or_abort();

    // When
    let output = verify(
        &reference,
        PINNED_REVISION,
        &[
            "--input-root".into(),
            unapproved.path().as_os_str().to_owned(),
        ],
    );

    // Then
    assert_failure(&output, "input root");
}

#[test]
fn source_guard_rejects_excluded_target_input() {
    // Given
    let reference = canonical_reference();
    let target = repo_root().join("target");
    std::fs::create_dir_all(&target).unwrap_or_abort();

    // When
    let output = verify(
        &reference,
        PINNED_REVISION,
        &["--input-root".into(), target.into()],
    );

    // Then
    assert_failure(&output, "excluded input root");
}

#[test]
fn source_guard_rejects_nested_target_directory_input() {
    // Given
    let (_temporary, target) = temporary_input("crates/harness-testkit", "target");
    std::fs::create_dir_all(&target).unwrap_or_abort();

    // When / Then
    assert_rejected_input(target);
}

#[test]
fn source_guard_rejects_nested_target_generated_file_input() {
    // Given
    let (_temporary, generated) =
        temporary_input("crates/harness-testkit", "target/debug/generated.rs");
    std::fs::write(&generated, b"generated\n").unwrap_or_abort();

    // When / Then
    assert_rejected_input(generated);
}

#[test]
fn source_guard_rejects_nested_node_modules_input() {
    // Given
    let (_temporary, node_modules) = temporary_input("scripts", "tui-parity/node_modules");
    std::fs::create_dir_all(&node_modules).unwrap_or_abort();

    // When / Then
    assert_rejected_input(node_modules);
}

fn temporary_input(root: &str, relative: &str) -> (tempfile::TempDir, PathBuf) {
    let temporary = tempfile::tempdir_in(repo_root().join(root)).unwrap_or_abort();
    let input = temporary.path().join(relative);
    std::fs::create_dir_all(input.parent().unwrap_or_abort()).unwrap_or_abort();
    (temporary, input)
}

fn assert_rejected_input(input: PathBuf) {
    let output = verify(
        &canonical_reference(),
        PINNED_REVISION,
        &["--input-root".into(), input.into()],
    );
    assert_failure(&output, "excluded input root");
}

#[test]
fn source_guard_rejects_reference_metadata_input() {
    // Given
    let reference = canonical_reference();
    let metadata = reference.join(".git");

    // When
    let output = verify(
        &reference,
        PINNED_REVISION,
        &["--input-root".into(), metadata.into()],
    );

    // Then
    assert_failure(&output, "unapproved input root");
}

#[test]
fn source_guard_rejects_unresolved_symlink() {
    // Given
    let reference = canonical_reference();
    let temporary = tempfile::tempdir().unwrap_or_abort();
    let unresolved = temporary.path().join("missing");
    std::os::unix::fs::symlink("absent", &unresolved).unwrap_or_abort();

    // When
    let output = verify(
        &reference,
        PINNED_REVISION,
        &["--input-root".into(), unresolved.as_os_str().to_owned()],
    );

    // Then
    assert_failure(&output, "resolve");
}

#[test]
fn source_guard_rejects_dirty_reference_worktree() {
    // Given
    let reference = canonical_reference();
    let temporary = tempfile::tempdir().unwrap_or_abort();
    let checkout = temporary.path().join("reference");
    add_reference_worktree(&reference, &checkout);
    std::fs::write(checkout.join("unapproved-input.txt"), b"mutation\n").unwrap_or_abort();

    // When
    let output = verify(&checkout, PINNED_REVISION, &[]);

    // Then
    std::fs::remove_file(checkout.join("unapproved-input.txt")).unwrap_or_abort();
    remove_reference_worktree(&reference, &checkout);
    assert_failure(&output, "dirty");
}

#[test]
fn source_guard_rejects_reference_source_mutation() {
    // Given
    let reference = canonical_reference();
    let temporary = tempfile::tempdir().unwrap_or_abort();
    let checkout = temporary.path().join("reference");
    add_reference_worktree(&reference, &checkout);
    let manifest = checkout.join("Cargo.toml");
    let original = std::fs::read(&manifest).unwrap_or_abort();
    let mut mutated = original.clone();
    mutated.extend_from_slice(b"\nsource guard mutation proof\n");
    std::fs::write(&manifest, mutated).unwrap_or_abort();

    // When
    let output = verify(&checkout, PINNED_REVISION, &[]);

    // Then
    std::fs::write(&manifest, original).unwrap_or_abort();
    remove_reference_worktree(&reference, &checkout);
    assert_failure(&output, "source mutation");
}

#[test]
fn source_guard_rejects_stale_receipt() {
    // Given
    let reference = canonical_reference();
    let temporary = tempfile::tempdir().unwrap_or_abort();
    let receipt = temporary.path().join("receipt.json");
    let first = verify(
        &reference,
        PINNED_REVISION,
        &["--receipt".into(), receipt.as_os_str().to_owned()],
    );
    assert_success(&first);
    let stale = std::fs::read_to_string(&receipt)
        .unwrap_or_abort()
        .replace(PINNED_REVISION, "0000000000000000000000000000000000000000");
    std::fs::write(&receipt, stale).unwrap_or_abort();

    // When
    let output = verify(
        &reference,
        PINNED_REVISION,
        &["--receipt".into(), receipt.as_os_str().to_owned()],
    );

    // Then
    assert_failure(&output, "receipt");
}

fn canonical_reference() -> PathBuf {
    let output = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(repo_root())
        .env("GIT_MASTER", "1")
        .output()
        .unwrap_or_abort();
    assert_success(&output);

    let common_dir = String::from_utf8(output.stdout).unwrap_or_abort();
    Path::new(common_dir.trim())
        .parent()
        .unwrap_or_abort()
        .join("inspirations/grok-build")
}

fn verify(reference: &Path, revision: &str, extra: &[std::ffi::OsString]) -> Output {
    let receipt_dir = tempfile::tempdir().unwrap_or_abort();
    let uses_explicit_receipt = extra.iter().any(|value| value == "--receipt");
    let mut command = Command::new(repo_root().join("scripts/tui-fidelity/source-guard.sh"));
    command
        .arg("verify")
        .arg("--reference")
        .arg(reference)
        .arg("--revision")
        .arg(revision)
        .current_dir(repo_root())
        .args(extra);
    if !uses_explicit_receipt {
        command
            .arg("--receipt")
            .arg(receipt_dir.path().join("receipt.json"));
    }
    command.output().unwrap_or_abort()
}

fn add_reference_worktree(reference: &Path, checkout: &Path) {
    let output = Command::new("git")
        .args(["worktree", "add", "--detach"])
        .arg(checkout)
        .arg(PINNED_REVISION)
        .current_dir(reference)
        .env("GIT_MASTER", "1")
        .output()
        .unwrap_or_abort();
    assert_success(&output);
}

fn remove_reference_worktree(reference: &Path, checkout: &Path) {
    let output = Command::new("git")
        .args(["worktree", "remove"])
        .arg(checkout)
        .current_dir(reference)
        .env("GIT_MASTER", "1")
        .output()
        .unwrap_or_abort();
    assert_success(&output);
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_failure(output: &Output, expected: &str) {
    assert!(
        !output.status.success() && String::from_utf8_lossy(&output.stderr).contains(expected),
        "expected failure containing {expected:?}, got {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}
