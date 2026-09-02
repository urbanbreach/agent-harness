//! Rejection tests for the P1-04 artifact-root guard, mirroring the
//! P1-03 security fix: an environment-supplied root must never reach
//! `remove_dir_all` unvalidated.

use crate::support::artifacts;
use harness_tui::UnwrapOrAbort;
use std::fs;
use std::path::PathBuf;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn temporary_home(name: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("harness-p1-04-guard-{}-{name}", std::process::id()));
    fs::create_dir_all(&directory).unwrap_or_abort();
    directory
}

#[test]
fn p1_04_artifact_root_rejects_unowned_directories() {
    // arrange
    let foreign = temporary_home("foreign");
    fs::write(foreign.join("operator-data.txt"), b"keep me").unwrap_or_abort();

    // act
    let result = std::panic::catch_unwind(|| artifacts::reset_artifact_root(&foreign));

    // assert: the unowned directory is refused before any deletion.
    assert!(result.is_err(), "unowned directory must be refused");
    assert!(foreign.join("operator-data.txt").is_file());

    // act: an owned directory carries the marker and resets cleanly.
    fs::write(foreign.join(".harness-p1-04-artifact-root"), b"").unwrap_or_abort();
    artifacts::reset_artifact_root(&foreign);
    assert!(!foreign.join("operator-data.txt").exists());
    let _ = fs::remove_dir_all(foreign);
}

#[test]
fn p1_04_artifact_root_rejects_unsanctioned_canonical_roots() {
    // arrange: candidate roots outside target/ and tempdir.
    let repository = repository_root();
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_abort());
    let source_tree = repository.join("crates");
    let workspace_docs = repository.join("docs");

    // act: every candidate is passed through validation.
    for candidate in [&repository, &home, &source_tree, &workspace_docs] {
        let result = std::panic::catch_unwind(|| artifacts::validated_artifact_root(candidate));

        // assert: unsanctioned roots are refused.
        assert!(
            result.is_err(),
            "unsanctioned root must be refused: {}",
            candidate.display()
        );
    }
}

#[test]
fn p1_04_artifact_root_accepts_sanctioned_roots() {
    // arrange: roots inside repository target/ and the platform tempdir.
    let inside_target = repository_root().join("target/p1-04-guard-accept");
    let inside_temporary =
        std::env::temp_dir().join(format!("harness-p1-04-guard-accept-{}", std::process::id()));
    fs::create_dir_all(&inside_target).unwrap_or_abort();
    fs::create_dir_all(&inside_temporary).unwrap_or_abort();

    // act: validation returns the canonical root without panicking, including
    // a non-existent descendant validated through its existing ancestors.
    let not_yet_created = inside_target.join("first-run/does-not-exist");
    artifacts::validated_artifact_root(&inside_target);
    artifacts::validated_artifact_root(&inside_temporary);
    artifacts::validated_artifact_root(&not_yet_created);

    // assert: sanctioned roots survive validation.
    assert!(inside_target.is_dir());
    assert!(inside_temporary.is_dir());
    let _ = fs::remove_dir_all(&inside_target);
    let _ = fs::remove_dir_all(&inside_temporary);
}
