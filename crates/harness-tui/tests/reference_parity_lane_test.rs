//! Contract tests for the strict fail-closed `signoff-parity` lane.
//!
//! Locks dry-run wiring and missing-manifest fail-closed behavior without
//! requiring dual-binary capture infrastructure from later waves.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration lane contract tests use fail-fast asserts for script/process outcomes"
)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root must resolve")
}

fn test_lanes_script() -> PathBuf {
    repo_root().join("scripts/test-lanes.sh")
}

fn reference_parity_manifest() -> PathBuf {
    repo_root().join("docs/reference/tui-reference-parity-manifest.v1.json")
}

#[test]
fn signoff_parity_dry_run_records_fail_closed_stages() {
    // arrange
    let artifact_root = tempfile::tempdir().expect("temp artifact root");
    let script = test_lanes_script();

    // act
    let output = Command::new("bash")
        .arg(&script)
        .args([
            "signoff-parity",
            "--dry-run",
            "--artifact-dir",
            artifact_root.path().to_str().expect("utf-8 path"),
        ])
        .current_dir(repo_root())
        .output()
        .expect("scripts/test-lanes.sh must be runnable");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let summary = fs::read_to_string(artifact_root.path().join("summary.txt"))
        .expect("dry-run must write summary.txt");
    let verdict_path = artifact_root
        .path()
        .join("signoff-parity/stages/parity_evidence/artifacts/parity-lane-verdict.txt");
    let verdict =
        fs::read_to_string(&verdict_path).expect("dry-run must write parity-lane-verdict.txt");

    // assert
    assert!(
        output.status.success(),
        "signoff-parity --dry-run must exit 0\nstdout={stdout}\nstderr={stderr}"
    );
    assert!(
        summary.contains("signoff-parity reference_parity_manifest_present DRY-RUN"),
        "dry-run must record the independent manifest path stage: {summary}"
    );
    assert!(
        summary.contains("signoff-parity reference_parity_manifest_test DRY-RUN")
            || summary.contains("signoff-parity p0_parity_contract_test DRY-RUN"),
        "dry-run must record at least one owner nextest stage: {summary}"
    );
    assert!(
        !summary
            .lines()
            .any(|line| line.starts_with("signoff-parity ") && line.contains(" SKIP")),
        "signoff-parity dry-run must not silent-skip required stages: {summary}"
    );
    assert!(
        verdict.contains("verdict=DRY-RUN")
            && verdict.contains("owns=dual_binary_cells_and_pixels")
            && verdict.contains("parity_complete=false"),
        "verdict must mark dry-run ownership and incomplete manifest parity: {verdict}"
    );
}

#[test]
fn signoff_parity_fails_closed_when_independent_manifest_missing() {
    // arrange
    let manifest = reference_parity_manifest();
    if manifest.is_file() {
        // Given: independent manifest already present (T02 landed).
        // When: this contract still requires fail-closed wiring in the script.
        // Then: static gate strings remain present so missing-path cannot soft-pass.
        let script = fs::read_to_string(test_lanes_script())
            .expect("scripts/test-lanes.sh must be readable");
        assert!(
            script.contains("docs/reference/tui-reference-parity-manifest.v1.json")
                && script.contains("parity_prerequisites")
                && script.contains("record_gate_failure"),
            "signoff-parity must keep a fail-closed missing-manifest gate even after the manifest lands"
        );
        return;
    }

    let artifact_root = tempfile::tempdir().expect("temp artifact root");

    // act
    let output = Command::new("bash")
        .arg(test_lanes_script())
        .args([
            "signoff-parity",
            "--artifact-dir",
            artifact_root.path().to_str().expect("utf-8 path"),
        ])
        .current_dir(repo_root())
        .output()
        .expect("scripts/test-lanes.sh must be runnable");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let summary = fs::read_to_string(artifact_root.path().join("summary.txt"))
        .expect("lane must write summary.txt");
    let gate_stderr = fs::read_to_string(
        artifact_root
            .path()
            .join("signoff-parity/stages/parity_prerequisites/stderr.txt"),
    )
    .unwrap_or_default();
    let verdict = fs::read_to_string(
        artifact_root
            .path()
            .join("signoff-parity/stages/parity_evidence/artifacts/parity-lane-verdict.txt"),
    )
    .unwrap_or_default();

    // assert
    assert!(
        !output.status.success(),
        "signoff-parity must fail closed when the independent manifest is missing\nstdout={stdout}\nstderr={stderr}"
    );
    assert!(
        summary.contains("signoff-parity parity_prerequisites FAIL") || summary.contains(" FAIL "),
        "missing manifest must record FAIL, not SKIP/PASS: {summary}"
    );
    assert!(
        gate_stderr.contains("tui-reference-parity-manifest.v1.json")
            || stderr.contains("tui-reference-parity-manifest.v1.json"),
        "failure must name the independent manifest path\nstderr={stderr}\ngate={gate_stderr}"
    );
    assert!(
        verdict.contains("verdict=FAIL"),
        "aggregate verdict must be FAIL when prerequisites are missing: {verdict}"
    );
}
