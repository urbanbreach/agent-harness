//! Fail-closed A-JOURNEYS scaffolding for journey-template rows.
//!
//! Offline deterministic owners:
//! - `JOURNEY-CONFIG-SHOW-EFFECTIVE`
//! - `JOURNEY-CONFIG-SOURCES-EXPLAIN` (sources + explain)
//!
//! Env-gated owner (documented here; executed only with `HARNESS_TUI_PTY_SIGNOFF=1`):
//! - `JOURNEY-WORKTREE-CTRL-W` → `pty_happy_path_recorded::dual_binary_cli_pty_worktree_ctrl_w_creates_git_worktree`
//!
//! Real offline surface evidence (L3 capture dirs; nonvisual journey passes):
//! - `JOURNEY-WAIT-ANY-ALL` — runs harness-tools wait-any integration owner
//! - `JOURNEY-MEMORY-CLI` — binary `harness memory put/get/list`
//! - `JOURNEY-FOLDER-TRUST-DENY` — deny-before-spawn gate receipt
//!
//! AppState-only evidence (manifest rows promoted to pass, Wave 6 2026-07-30:
//! AppState surface captures + canonical L1/L4 pairing; full PTY shell
//! evidence is still env-gated and not claimed):
//! - `JOURNEY-ALWAYS-APPROVE-MODE` — AppState AlwaysConfirm + badge render dump
//! - `JOURNEY-SETTINGS-EDITOR` — AppState /settings overlay + registry rows dump

use harness::UnwrapOrAbort;
use serde_json::Value;
use std::fs;

mod common;
#[path = "support/journey_signoff.rs"]
mod journey_signoff;
#[path = "support/journey_surface_evidence.rs"]
mod journey_surface_evidence;

use common::repo_root;
use journey_signoff::{
    assert_all_journey_manifest_evidence, assert_cli_success, example_config_path,
    journey_artifact_root, parse_stdout_json, run_harness_cli, stable_l3_artifact_root,
    write_cli_artifact_pair, STABLE_L3_CONFIG_SHOW_REL, STABLE_L3_CONFIG_SOURCES_REL,
    STABLE_L3_WORKTREE_REL, WORKTREE_PTY_OWNER_FN, WORKTREE_PTY_OWNER_REL,
};
use journey_surface_evidence::{
    execute_always_approve_mode_surface_evidence, execute_folder_trust_deny_surface_evidence,
    execute_memory_cli_surface_evidence, execute_settings_editor_surface_evidence,
    execute_wait_any_surface_evidence,
};

#[test]
fn journey_config_show_effective_cli_writes_artifact() {
    // arrange
    // act
    // assert
    let artifacts = journey_artifact_root("config-show-effective");
    let config = example_config_path();

    let output = run_harness_cli(&[
        "--config",
        config.to_str().unwrap_or_abort(),
        "config",
        "show",
        "--effective",
    ]);
    write_cli_artifact_pair(
        &artifacts,
        STABLE_L3_CONFIG_SHOW_REL,
        "config-show-effective",
        &output,
    );

    assert_cli_success("config show --effective", &output);
    let json = parse_stdout_json(&output);
    assert_eq!(
        json["schema_version"].as_str(),
        Some("harness-config-effective-v1")
    );
    assert_eq!(json["redacted"].as_bool(), Some(true));
    assert!(
        json["layers"]
            .as_array()
            .is_some_and(|layers| !layers.is_empty()),
        "expected non-empty layers: {json}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("sk-proj-") && !stdout.contains("sk-ant-"),
        "secret-looking token leaked in journey artifact stdout: {stdout}"
    );
    assert!(artifacts.join("config-show-effective.stdout.txt").is_file());
    assert!(stable_l3_artifact_root(STABLE_L3_CONFIG_SHOW_REL)
        .join("config-show-effective.stdout.txt")
        .is_file());
}

#[test]
fn journey_config_sources_cli_writes_artifact() {
    // arrange
    // act
    // assert
    let artifacts = journey_artifact_root("config-sources");
    let config = example_config_path();

    let output = run_harness_cli(&[
        "--config",
        config.to_str().unwrap_or_abort(),
        "config",
        "sources",
    ]);
    write_cli_artifact_pair(
        &artifacts,
        STABLE_L3_CONFIG_SOURCES_REL,
        "config-sources",
        &output,
    );

    assert_cli_success("config sources", &output);
    let json = parse_stdout_json(&output);
    assert_eq!(
        json["schema_version"].as_str(),
        Some("harness-config-sources-v1")
    );
    assert!(
        json["layers"]
            .as_array()
            .is_some_and(|layers| !layers.is_empty()),
        "expected discovered layers: {json}"
    );
    assert!(artifacts.join("config-sources.stdout.txt").is_file());
    assert!(stable_l3_artifact_root(STABLE_L3_CONFIG_SOURCES_REL)
        .join("config-sources.stdout.txt")
        .is_file());
}

#[test]
fn journey_config_explain_cli_writes_artifact() {
    // arrange
    // act
    // assert
    let artifacts = journey_artifact_root("config-explain");
    let config = example_config_path();

    let output = run_harness_cli(&[
        "--config",
        config.to_str().unwrap_or_abort(),
        "config",
        "explain",
        "model",
    ]);
    write_cli_artifact_pair(
        &artifacts,
        STABLE_L3_CONFIG_SOURCES_REL,
        "config-explain",
        &output,
    );

    assert_cli_success("config explain model", &output);
    let json = parse_stdout_json(&output);
    assert_eq!(
        json["schema_version"].as_str(),
        Some("harness-config-explain-v1")
    );
    assert_eq!(json["path"].as_str(), Some("model"));
    assert_eq!(json["found"].as_bool(), Some(true));
    assert_eq!(json["redacted"].as_bool(), Some(true));
    assert!(
        json["source_path"].as_str().is_some_and(|p| !p.is_empty()),
        "expected winning source_path: {json}"
    );
    assert!(artifacts.join("config-explain.stdout.txt").is_file());
    assert!(stable_l3_artifact_root(STABLE_L3_CONFIG_SOURCES_REL)
        .join("config-explain.stdout.txt")
        .is_file());
}

#[test]
fn journey_worktree_ctrl_w_owner_is_env_gated_dual_binary_pty() {
    // arrange
    // act
    // assert
    let owner_path = repo_root().join(WORKTREE_PTY_OWNER_REL);
    let source = fs::read_to_string(&owner_path).unwrap_or_else(|err| {
        panic!(
            "missing worktree PTY journey owner at {}: {err}",
            owner_path.display()
        )
    });

    assert!(
        source.contains(WORKTREE_PTY_OWNER_FN),
        "worktree journey owner fn `{WORKTREE_PTY_OWNER_FN}` missing from {}",
        owner_path.display()
    );
    assert!(
        source.contains("HARNESS_TUI_PTY_SIGNOFF"),
        "worktree journey owner must be env-gated on HARNESS_TUI_PTY_SIGNOFF"
    );
    assert!(
        source.contains("#[ignore")
            && source.contains("signoff dual-binary CLI PTY worktree journey"),
        "worktree journey owner must remain ignored by default"
    );

    let artifacts = journey_artifact_root("worktree-owner");
    let receipt = serde_json::json!({
        "schema_version": "harness-journey-worktree-owner-v1",
        "journey_id": "JOURNEY-WORKTREE-CTRL-W",
        "status": "incomplete",
        "owner_test": format!("{WORKTREE_PTY_OWNER_REL}::{WORKTREE_PTY_OWNER_FN}"),
        "gate": "HARNESS_TUI_PTY_SIGNOFF=1",
        "run_command": [
            "env",
            "RUST_TEST_THREADS=1",
            "HARNESS_TUI_PTY_SIGNOFF=1",
            "cargo",
            "nextest",
            "run",
            "-p",
            "harness",
            "--test",
            "pty_happy_path_recorded",
            "--test-threads",
            "1",
            "--",
            "--ignored",
            "--exact",
            WORKTREE_PTY_OWNER_FN
        ],
        "notes": "Offline journey lane documents owner only; full L1 freeze/PTY evidence remains open. L3 text artifact is this owner receipt, not a PNG freeze."
    });
    let body = serde_json::to_vec_pretty(&receipt).unwrap_or_abort();
    fs::write(artifacts.join("worktree-owner.json"), &body).unwrap_or_abort();
    let stable = stable_l3_artifact_root(STABLE_L3_WORKTREE_REL);
    fs::write(stable.join("worktree-owner.json"), &body).unwrap_or_abort();
    assert!(artifacts.join("worktree-owner.json").is_file());
    assert!(stable.join("worktree-owner.json").is_file());
}

#[test]
fn journey_wait_any_all_runs_owner_tests_and_writes_artifacts() {
    // arrange
    // act
    // assert
    execute_wait_any_surface_evidence();
}

#[test]
fn journey_memory_cli_put_get_list_writes_artifacts() {
    // arrange
    // act
    // assert
    execute_memory_cli_surface_evidence();
}

#[test]
fn journey_folder_trust_deny_documents_deny_path() {
    // arrange
    // act
    // assert
    execute_folder_trust_deny_surface_evidence();
}

#[test]
fn journey_always_approve_mode_appstate_render_writes_artifacts() {
    // arrange
    // act
    // assert
    execute_always_approve_mode_surface_evidence();
}

#[test]
fn journey_settings_editor_appstate_render_writes_artifacts() {
    // arrange
    // act
    // assert
    execute_settings_editor_surface_evidence();
}

#[test]
fn journey_manifest_rows_point_at_signoff_scaffolding() {
    // arrange
    // act
    // assert
    let manifest_path = repo_root().join("docs/reference/tui-reference-parity-manifest.v1.json");
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap_or_abort())
            .unwrap_or_abort();
    let rows = manifest["rows"].as_array().unwrap_or_abort();
    assert_all_journey_manifest_evidence(rows);
}

#[test]
fn ordinary_mode_skips_missing_gitignored_journey_evidence() {
    // arrange
    let path = "target/test-lanes/latest/signoff-parity/evidence/receipts/missing-receipt.md";

    // act
    let result = std::panic::catch_unwind(|| {
        journey_signoff::assert_evidence_path_exists("JOURNEY-TEST-SKIP", "L6", path, false);
    });

    // assert
    assert!(
        result.is_ok(),
        "ordinary mode must not fail when a gitignored receipt is missing"
    );
}

#[test]
fn strict_mode_rejects_missing_gitignored_journey_evidence() {
    // arrange
    let path = "target/test-lanes/latest/signoff-parity/evidence/receipts/missing-receipt.md";

    // act
    let result = std::panic::catch_unwind(|| {
        journey_signoff::assert_evidence_path_exists("JOURNEY-TEST-STRICT", "L6", path, true);
    });

    // assert
    assert!(
        result.is_err(),
        "strict mode must fail when a referenced gitignored receipt is missing"
    );
}

#[test]
fn ordinary_mode_still_rejects_missing_committed_source_evidence() {
    // arrange
    let path = "crates/harness/src/this_file_does_not_exist.rs";

    // act
    let result = std::panic::catch_unwind(|| {
        journey_signoff::assert_evidence_path_exists("JOURNEY-TEST-SOURCE", "L2", path, false);
    });

    // assert
    assert!(
        result.is_err(),
        "ordinary mode must still fail on missing committed source evidence"
    );
}
