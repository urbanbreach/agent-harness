#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "owner tests use fail-fast contract assertions"
)]

use std::fs;
use std::process::Command;

use serde_json::Value;

#[path = "support/tui_reference_authority_test.rs"]
mod authority_support;

use authority_support::{
    authority_defects, check_field, read_json, repo_root, ACTIVE_BINARY_SHA256, ACTIVE_REVISION,
    ACTIVE_VERSION, AUTHORITY_PATH,
};

#[test]
fn active_reference_authority_agrees_with_all_declared_sources() {
    // Given
    let root = repo_root();
    let authority = read_json(&root.join(AUTHORITY_PATH));

    // When
    let defects = authority_defects(&root, &authority);

    // Then
    assert!(defects.is_empty(), "{}", defects.join("\n"));
}

#[test]
fn active_reference_authority_rejects_copied_revision_mutation() {
    // Given
    let root = repo_root();
    let temporary = tempfile::tempdir().expect("temporary authority directory");
    let copied = temporary.path().join("authority.json");
    fs::copy(root.join(AUTHORITY_PATH), &copied).expect("copy authority fixture");
    let mut authority = read_json(&copied);
    authority["reference"]["source_revision"] = Value::String("0".repeat(40));
    fs::write(
        &copied,
        serde_json::to_vec_pretty(&authority).expect("serialize mutated authority"),
    )
    .expect("write mutated authority fixture");

    // When
    let mut defects = Vec::new();
    check_field(
        &read_json(&copied),
        "/reference/source_revision",
        ACTIVE_REVISION,
        &mut defects,
    );

    // Then
    assert!(
        defects
            .iter()
            .any(|defect| defect.starts_with("/reference/source_revision expected")),
        "revision mutation was not rejected: {defects:?}"
    );
}

#[test]
#[ignore = "manual canonical reference binary provenance check"]
fn canonical_reference_binary_matches_active_authority() {
    // Given
    let root = repo_root();
    let authority = read_json(&root.join(AUTHORITY_PATH));
    let binary = root.join(
        authority["reference"]["executable"]
            .as_str()
            .expect("reference executable path"),
    );

    // When
    let digest = command_text(Command::new("sha256sum").arg(&binary));
    let version = command_text(Command::new(&binary).arg("--version"));

    // Then
    assert_eq!(digest.split_whitespace().next(), Some(ACTIVE_BINARY_SHA256));
    assert_eq!(version.trim(), ACTIVE_VERSION);
}

fn command_text(command: &mut Command) -> String {
    let output = command.output().expect("execute provenance command");
    assert!(output.status.success(), "provenance command failed");
    String::from_utf8(output.stdout).expect("provenance output is UTF-8")
}
