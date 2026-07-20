//! Shared helpers for A-JOURNEYS offline scaffolding tests.

use harness::UnwrapOrAbort;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::common::repo_root;

pub(crate) const WORKTREE_PTY_OWNER_REL: &str = "crates/harness/tests/pty_happy_path_recorded.rs";
pub(crate) const WORKTREE_PTY_OWNER_FN: &str =
    "dual_binary_cli_pty_worktree_ctrl_w_creates_git_worktree";
pub(crate) const JOURNEY_TEST_REL: &str = "crates/harness/tests/journey_signoff_test.rs";
pub(crate) const JOURNEY_LANE_RECEIPT_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/receipts/loop15-journey-lane-v1.md";
pub(crate) const JOURNEY_EVIDENCE_EXPAND_RECEIPT_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/receipts/loop15-journey-evidence-expand-v1.md";
pub(crate) const JOURNEY_ROWS_EXPAND_RECEIPT_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/receipts/loop15-journey-rows-expand-v1.md";
pub(crate) const WAIT_ANY_ALL_L2_REL: &str =
    "crates/harness-core/src/coord/background_notifications.rs";
pub(crate) const WAIT_ANY_ALL_L5_REL: &str =
    "crates/harness-tools/tests/native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/10_background_output_wait_any_all_test.rs";
pub(crate) const WAIT_ANY_ALL_L6_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/receipts/loop15-journey-surface-evidence-v1.md";
pub(crate) const FOLDER_TRUST_L2_REL: &str = "crates/harness-core/src/folder_trust.rs";
pub(crate) const FOLDER_TRUST_L5_REL: &str = "crates/harness-tools/src/shell_safety.rs";
pub(crate) const FOLDER_TRUST_L6_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/receipts/loop15-journey-surface-evidence-v1.md";
pub(crate) const MEMORY_CLI_L2_REL: &str = "crates/harness-core/src/memory.rs";
pub(crate) const MEMORY_CLI_L5_REL: &str = "crates/harness/src/memory_cmd.rs";
pub(crate) const MEMORY_CLI_L6_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/receipts/loop15-journey-surface-evidence-v1.md";
pub(crate) const STABLE_L3_WAIT_ANY_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/actual/journey-wait-any-all-v1";
pub(crate) const STABLE_L3_MEMORY_CLI_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/actual/journey-memory-cli-v1";
pub(crate) const STABLE_L3_FOLDER_TRUST_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/actual/journey-folder-trust-deny-v1";
pub(crate) const ALWAYS_APPROVE_L2_REL: &str =
    "crates/harness-tui/src/app/tests/permission_modal_tests.rs";
pub(crate) const ALWAYS_APPROVE_L5_REL: &str = "crates/harness-tui/src/keybindings/tests.rs";
pub(crate) const ALWAYS_APPROVE_L6_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/receipts/loop15-journey-always-settings-l3-v1.md";
pub(crate) const STABLE_L3_ALWAYS_APPROVE_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/actual/journey-always-approve-mode-v1";
pub(crate) const SETTINGS_EDITOR_L2_REL: &str =
    "crates/harness-tui/src/app/tests/settings_editor_tests.rs";
pub(crate) const SETTINGS_EDITOR_L5_REL: &str =
    "crates/harness-tui/src/app/tests/settings_editor_tests.rs";
pub(crate) const SETTINGS_EDITOR_L6_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/receipts/loop15-journey-always-settings-l3-v1.md";
pub(crate) const STABLE_L3_SETTINGS_EDITOR_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/actual/journey-settings-editor-v1";
pub(crate) const CONFIG_SHOW_RECEIPT_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/receipts/loop15-config-show-effective-v1.md";
pub(crate) const CONFIG_SOURCES_RECEIPT_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/receipts/loop15-config-sources-explain-v1.md";
pub(crate) const WORKTREE_PTY_RECEIPT_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/receipts/loop15-worktree-pty-journey-v1.md";
pub(crate) const WORKTREE_FUNCTIONAL_RECEIPT_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/receipts/loop15-worktree-functional-v1.md";
pub(crate) const CONFIG_STATE_TEST_REL: &str = "crates/harness/src/lib.rs";
pub(crate) const WORKTREE_STATE_TEST_REL: &str =
    "crates/harness-tui/src/app/tests/lifecycle_shell_tests.rs";
pub(crate) const WORKTREE_CORE_TEST_REL: &str = "crates/harness-core/src/worktree.rs";
pub(crate) const STABLE_L3_CONFIG_SHOW_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/actual/journey-config-show-effective-v1";
pub(crate) const STABLE_L3_CONFIG_SOURCES_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/actual/journey-config-sources-explain-v1";
pub(crate) const STABLE_L3_WORKTREE_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/actual/journey-worktree-owner-v1";
pub(crate) const EXAMPLE_CONFIG_REL: &str = "configs/harness.example.jsonc";

pub(crate) fn require_harness_binary() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_BIN_EXE_harness"));
    assert!(
        path.is_file(),
        "missing harness binary at {}; journey signoff is fail-closed (no skip)",
        path.display()
    );
    path
}

pub(crate) fn example_config_path() -> PathBuf {
    let config = repo_root().join(EXAMPLE_CONFIG_REL);
    assert!(
        config.is_file(),
        "missing example config {}; journey lane is fail-closed",
        config.display()
    );
    config
}

pub(crate) fn journey_artifact_root(slug: &str) -> PathBuf {
    let root = std::env::var_os("HARNESS_JOURNEY_ARTIFACT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("target").join("journey-signoff-artifacts"));
    let dir = root.join(slug);
    fs::create_dir_all(&dir).unwrap_or_abort();
    dir
}

#[allow(
    clippy::panic,
    reason = "fail-closed test helper with custom error message"
)]
pub(crate) fn stable_l3_artifact_root(rel: &str) -> PathBuf {
    let dir = repo_root().join(rel);
    fs::create_dir_all(&dir).unwrap_or_else(|err| {
        panic!(
            "failed to create stable L3 evidence dir {}: {err} (fail-closed)",
            dir.display()
        )
    });
    dir
}

#[allow(
    clippy::panic,
    reason = "fail-closed test helper with custom error message"
)]
pub(crate) fn run_harness_cli(args: &[&str]) -> std::process::Output {
    let harness_bin = require_harness_binary();
    Command::new(&harness_bin)
        .args(args)
        .current_dir(repo_root())
        .output()
        .unwrap_or_else(|err| {
            panic!(
                "failed to spawn harness binary at {}: {err} (fail-closed)",
                harness_bin.display()
            )
        })
}

pub(crate) fn write_cli_artifact(dir: &Path, name: &str, output: &std::process::Output) {
    fs::write(dir.join(format!("{name}.stdout.txt")), &output.stdout).unwrap_or_abort();
    fs::write(dir.join(format!("{name}.stderr.txt")), &output.stderr).unwrap_or_abort();
    fs::write(
        dir.join(format!("{name}.status.txt")),
        format!(
            "success={}\ncode={}\n",
            output.status.success(),
            output.status.code().unwrap_or(-1)
        ),
    )
    .unwrap_or_abort();
}

pub(crate) fn write_cli_artifact_pair(
    lane_dir: &Path,
    stable_rel: &str,
    name: &str,
    output: &std::process::Output,
) {
    write_cli_artifact(lane_dir, name, output);
    let stable = stable_l3_artifact_root(stable_rel);
    write_cli_artifact(&stable, name, output);
    assert!(
        stable.join(format!("{name}.stdout.txt")).is_file(),
        "stable L3 stdout missing under {} (fail-closed)",
        stable.display()
    );
}

pub(crate) fn assert_cli_success(label: &str, output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{label} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[allow(
    clippy::panic,
    reason = "fail-closed test helper with custom error message"
)]
pub(crate) fn parse_stdout_json(output: &std::process::Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim()).unwrap_or_else(|err| {
        panic!("stdout is not JSON ({err}): {stdout}");
    })
}

#[allow(
    clippy::panic,
    reason = "fail-closed test helper with custom error message"
)]
pub(crate) fn find_journey<'a>(rows: &'a [Value], journey_id: &str) -> &'a Value {
    rows.iter()
        .find(|row| row["behavior_id"].as_str() == Some(journey_id))
        .unwrap_or_else(|| panic!("missing journey row {journey_id}"))
}

pub(crate) fn assert_evidence_path_exists(journey_id: &str, layer: &str, rel: &str) {
    if rel.is_empty() {
        return;
    }
    let file_rel = rel.split("::").next().unwrap_or(rel);
    let path = repo_root().join(file_rel);
    assert!(
        path.exists(),
        "{journey_id} evidence_paths.{layer} missing on disk (fail-closed): {rel} -> {}",
        path.display()
    );
}

fn assert_empty_layer(journey_id: &str, row: &Value, layer: &str) {
    assert!(
        row["evidence_paths"][layer]
            .as_str()
            .unwrap_or("x")
            .is_empty(),
        "{journey_id} evidence_paths.{layer} must stay empty"
    );
}

fn assert_static_evidence_layers(journey_id: &str, row: &Value) {
    for layer in ["L2", "L5", "L6"] {
        assert_evidence_path_exists(
            journey_id,
            layer,
            row["evidence_paths"][layer].as_str().unwrap_or(""),
        );
    }
}

fn assert_owner_names_journey_signoff(journey_id: &str, row: &Value) {
    let owner = row["owners"]["state_interaction_test"]
        .as_str()
        .unwrap_or("");
    assert!(
        owner.contains("journey_signoff_test"),
        "{journey_id} state_interaction_test owner must name journey_signoff_test, got {owner}"
    );
}

fn assert_config_journey_evidence(row: &Value, journey_id: &str, l3: &str, l6: &str) {
    assert_eq!(row["status"].as_str(), Some("pass"));
    assert_empty_layer(journey_id, row, "L1");
    assert_eq!(
        row["evidence_paths"]["L2"].as_str(),
        Some(CONFIG_STATE_TEST_REL)
    );
    assert_eq!(row["evidence_paths"]["L3"].as_str(), Some(l3));
    assert_empty_layer(journey_id, row, "L4");
    assert_eq!(row["evidence_paths"]["L5"].as_str(), Some(JOURNEY_TEST_REL));
    assert_eq!(row["evidence_paths"]["L6"].as_str(), Some(l6));
    assert_static_evidence_layers(journey_id, row);
    assert_owner_names_journey_signoff(journey_id, row);
}

pub(crate) fn assert_all_journey_manifest_evidence(rows: &[Value]) {
    assert_config_journey_evidence(
        find_journey(rows, "JOURNEY-CONFIG-SHOW-EFFECTIVE"),
        "JOURNEY-CONFIG-SHOW-EFFECTIVE",
        STABLE_L3_CONFIG_SHOW_REL,
        CONFIG_SHOW_RECEIPT_REL,
    );
    assert_config_journey_evidence(
        find_journey(rows, "JOURNEY-CONFIG-SOURCES-EXPLAIN"),
        "JOURNEY-CONFIG-SOURCES-EXPLAIN",
        STABLE_L3_CONFIG_SOURCES_REL,
        CONFIG_SOURCES_RECEIPT_REL,
    );

    let journey_id = "JOURNEY-WORKTREE-CTRL-W";
    let worktree = find_journey(rows, journey_id);
    assert_eq!(worktree["status"].as_str(), Some("pass"));
    assert_empty_layer(journey_id, worktree, "L1");
    let l2 = worktree["evidence_paths"]["L2"].as_str().unwrap_or("");
    assert!(
        l2 == WORKTREE_STATE_TEST_REL || l2.contains("lifecycle_shell_tests"),
        "worktree L2 must point at state/intent tests, got {l2}"
    );
    assert_eq!(
        worktree["evidence_paths"]["L3"].as_str(),
        Some(STABLE_L3_WORKTREE_REL)
    );
    assert_empty_layer(journey_id, worktree, "L4");
    assert_eq!(
        worktree["evidence_paths"]["L5"].as_str(),
        Some(JOURNEY_TEST_REL)
    );
    let l6 = worktree["evidence_paths"]["L6"].as_str().unwrap_or("");
    assert!(
        l6 == WORKTREE_PTY_RECEIPT_REL
            || l6 == WORKTREE_FUNCTIONAL_RECEIPT_REL
            || l6 == JOURNEY_LANE_RECEIPT_REL
            || l6 == JOURNEY_EVIDENCE_EXPAND_RECEIPT_REL,
        "worktree L6 must point at an existing worktree/journey receipt, got {l6}"
    );
    assert_static_evidence_layers(journey_id, worktree);
    assert_evidence_path_exists(journey_id, "core", WORKTREE_CORE_TEST_REL);
    let pty_owner = worktree["owners"]["pty_test"].as_str().unwrap_or("");
    assert!(
        pty_owner.contains(WORKTREE_PTY_OWNER_FN) || pty_owner.contains("pty_happy_path_recorded"),
        "worktree pty_test owner must point at dual-binary PTY worktree test, got {pty_owner}"
    );
    let state_owner = worktree["owners"]["state_interaction_test"]
        .as_str()
        .unwrap_or("");
    assert!(
        state_owner.contains("journey_signoff_test") || state_owner.contains("lifecycle_shell"),
        "worktree state_interaction_test must point at journey scaffolding or lifecycle intent tests, got {state_owner}"
    );

    assert_surface_journey_evidence(
        rows,
        "JOURNEY-WAIT-ANY-ALL",
        "orchestration.wait_any",
        WAIT_ANY_ALL_L2_REL,
        STABLE_L3_WAIT_ANY_REL,
        WAIT_ANY_ALL_L5_REL,
        WAIT_ANY_ALL_L6_REL,
        "pass",
    );
    assert_surface_journey_evidence(
        rows,
        "JOURNEY-FOLDER-TRUST-DENY",
        "workspace.folder_trust",
        FOLDER_TRUST_L2_REL,
        STABLE_L3_FOLDER_TRUST_REL,
        FOLDER_TRUST_L5_REL,
        FOLDER_TRUST_L6_REL,
        "pass",
    );
    assert_surface_journey_evidence(
        rows,
        "JOURNEY-MEMORY-CLI",
        "memory.durable_product_surface",
        MEMORY_CLI_L2_REL,
        STABLE_L3_MEMORY_CLI_REL,
        MEMORY_CLI_L5_REL,
        MEMORY_CLI_L6_REL,
        "pass",
    );
    assert_surface_journey_evidence(
        rows,
        "JOURNEY-ALWAYS-APPROVE-MODE",
        "permission.always_approve_mode",
        ALWAYS_APPROVE_L2_REL,
        STABLE_L3_ALWAYS_APPROVE_REL,
        ALWAYS_APPROVE_L5_REL,
        ALWAYS_APPROVE_L6_REL,
        "pass",
    );
    assert_surface_journey_evidence(
        rows,
        "JOURNEY-SETTINGS-EDITOR",
        "tui.settings_editor",
        SETTINGS_EDITOR_L2_REL,
        STABLE_L3_SETTINGS_EDITOR_REL,
        SETTINGS_EDITOR_L5_REL,
        SETTINGS_EDITOR_L6_REL,
        "pass",
    );
    assert_evidence_path_exists("JOURNEY-ROWS-EXPAND", "L6", JOURNEY_ROWS_EXPAND_RECEIPT_REL);
}

fn assert_surface_journey_evidence(
    rows: &[Value],
    journey_id: &str,
    capability_id: &str,
    l2: &str,
    l3: &str,
    l5: &str,
    l6: &str,
    expected_status: &str,
) {
    let row = find_journey(rows, journey_id);
    assert_eq!(row["status"].as_str(), Some(expected_status));
    assert_eq!(row["row_kind"].as_str(), Some("journey"));
    assert_eq!(row["capability_id"].as_str(), Some(capability_id));
    assert!(!row["backend_owner"].as_str().unwrap_or("").is_empty());
    assert_empty_layer(journey_id, row, "L1");
    assert_empty_layer(journey_id, row, "L4");
    assert_eq!(row["evidence_paths"]["L2"].as_str(), Some(l2));
    assert_eq!(row["evidence_paths"]["L3"].as_str(), Some(l3));
    assert_eq!(row["evidence_paths"]["L5"].as_str(), Some(l5));
    assert_eq!(row["evidence_paths"]["L6"].as_str(), Some(l6));
    assert_static_evidence_layers(journey_id, row);
    let state_owner = row["owners"]["state_interaction_test"]
        .as_str()
        .unwrap_or("");
    assert!(
        !state_owner.is_empty() && state_owner != "pending",
        "{journey_id} state_interaction_test must name a real owner, got {state_owner}"
    );
}
