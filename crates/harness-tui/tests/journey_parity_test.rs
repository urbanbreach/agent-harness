//! Journey parity tests for Todo 27.
//!
//! Proves every manifest row whose surface is a nonvisual TUI journey,
//! using compiled CLI/TUI/coordinator operations and external postconditions.
//!
//! No synthetic destination AppState or direct event injection is used.
//! Each journey drives a real compiled operation (harness binary, coordinator
//! API, or TUI slash/key interaction) and verifies external postconditions.
//!
//! Manifest: docs/tui-reference-parity-manifest.v1.json
//! Journey rows: 8 (config x2, memory, orchestration, workspace, permissions, tui, worktree)

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration journey tests use fail-fast asserts"
)]

use std::fs;

use harness_tui::UnwrapOrAbort;
use serde_json::Value;

#[path = "support/journey_parity.rs"]
mod support;

use support::{
    collect_all_postconditions, harness_binary, journey_rows, load_manifest, repo_root,
    verify_all_postconditions, verify_mutated_postcondition_rejected, write_json_artifact,
    JOURNEY_IDS, MANIFEST_REL,
};

const MANIFEST_SRC: &str =
    include_str!("../../../docs/reference/tui-reference-parity-manifest.v1.json");

fn checked_in_manifest() -> Value {
    serde_json::from_str(MANIFEST_SRC).unwrap_or_abort()
}

// ---------------------------------------------------------------------------
// Happy path: each journey produces real action/event/postcondition evidence
// ---------------------------------------------------------------------------

#[test]
fn journey_manifest_has_exactly_8_journey_rows() {
    let manifest = checked_in_manifest();
    let rows = journey_rows(&manifest);
    assert_eq!(
        rows.len(),
        8,
        "expected exactly 8 journey rows, got {}",
        rows.len()
    );
}

#[test]
fn journey_manifest_rows_match_expected_ids() {
    let manifest = checked_in_manifest();
    let rows = journey_rows(&manifest);
    let ids: Vec<&str> = rows
        .iter()
        .map(|r| r["behavior_id"].as_str().unwrap_or(""))
        .collect();
    for expected in JOURNEY_IDS {
        assert!(
            ids.contains(&expected),
            "journey row {expected} missing from manifest"
        );
    }
}

#[test]
fn journey_config_show_effective_produces_real_evidence() {
    let manifest = checked_in_manifest();
    let rows = journey_rows(&manifest);
    let row = rows
        .iter()
        .find(|r| r["behavior_id"].as_str() == Some("JOURNEY-CONFIG-SHOW-EFFECTIVE"))
        .unwrap_or_abort();

    assert_eq!(row["row_kind"].as_str(), Some("journey"));
    assert_eq!(row["capability_id"].as_str(), Some("config.show.effective"));

    // Run the compiled harness binary
    let config = repo_root().join("configs/harness.example.jsonc");
    let output = support::run_harness(&[
        "--config",
        config.to_str().unwrap_or_abort(),
        "config",
        "show",
        "--effective",
    ]);
    support::assert_success("config show --effective", &output);
    support::write_cli_artifact("config-show-effective", "config-show-effective", &output);

    let json = support::parse_json(&output);
    assert_eq!(
        json["schema_version"].as_str(),
        Some("harness-config-effective-v1")
    );
    assert_eq!(json["redacted"].as_bool(), Some(true));
    assert!(
        json["layers"].as_array().is_some_and(|l| !l.is_empty()),
        "expected non-empty layers: {json}"
    );

    // External postcondition: no secret-looking tokens
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("sk-proj-") && !stdout.contains("sk-ant-"),
        "secret-looking token leaked in config show output"
    );
}

#[test]
fn journey_config_sources_explain_produces_real_evidence() {
    let config = repo_root().join("configs/harness.example.jsonc");

    // config sources
    let sources = support::run_harness(&[
        "--config",
        config.to_str().unwrap_or_abort(),
        "config",
        "sources",
    ]);
    support::assert_success("config sources", &sources);
    support::write_cli_artifact("config-sources", "config-sources", &sources);
    let sources_json = support::parse_json(&sources);
    assert_eq!(
        sources_json["schema_version"].as_str(),
        Some("harness-config-sources-v1")
    );
    assert!(
        sources_json["layers"]
            .as_array()
            .is_some_and(|l| !l.is_empty()),
        "expected discovered layers: {sources_json}"
    );

    // config explain model
    let explain = support::run_harness(&[
        "--config",
        config.to_str().unwrap_or_abort(),
        "config",
        "explain",
        "model",
    ]);
    support::assert_success("config explain model", &explain);
    support::write_cli_artifact("config-explain", "config-explain", &explain);
    let explain_json = support::parse_json(&explain);
    assert_eq!(
        explain_json["schema_version"].as_str(),
        Some("harness-config-explain-v1")
    );
    assert_eq!(explain_json["found"].as_bool(), Some(true));
    assert_eq!(explain_json["redacted"].as_bool(), Some(true));
    assert!(
        explain_json["source_path"]
            .as_str()
            .is_some_and(|p| !p.is_empty()),
        "expected winning source_path: {explain_json}"
    );
}

#[test]
fn journey_memory_cli_produces_real_evidence() {
    let workspace = tempfile::tempdir().unwrap_or_abort();
    let ws = workspace.path().to_str().unwrap_or_abort();

    // put
    let put = support::run_harness(&[
        "memory",
        "put",
        "journey.parity.memory",
        "verified-value",
        "--workspace",
        ws,
    ]);
    support::assert_success("memory put", &put);
    support::write_cli_artifact("memory-cli", "memory-put", &put);
    let put_json = support::parse_json(&put);
    assert_eq!(put_json["key"].as_str(), Some("journey.parity.memory"));
    assert_eq!(put_json["value"].as_str(), Some("verified-value"));

    // get
    let get = support::run_harness(&["memory", "get", "journey.parity.memory", "--workspace", ws]);
    support::assert_success("memory get", &get);
    support::write_cli_artifact("memory-cli", "memory-get", &get);
    let get_json = support::parse_json(&get);
    assert_eq!(get_json["value"].as_str(), Some("verified-value"));

    // list
    let list = support::run_harness(&["memory", "list", "--workspace", ws]);
    support::assert_success("memory list", &list);
    support::write_cli_artifact("memory-cli", "memory-list", &list);
    let list_json = support::parse_json(&list);
    assert!(
        list_json["entries"]
            .as_array()
            .is_some_and(|e| !e.is_empty()),
        "expected non-empty memory list: {list_json}"
    );
}

#[test]
fn journey_wait_any_all_produces_real_evidence() {
    use harness_core::coord::{background_wait_condition_satisfied, BackgroundWaitMode};

    let any = BackgroundWaitMode::parse("any").unwrap_or_abort();
    let all = BackgroundWaitMode::parse("all").unwrap_or_abort();
    let partial = [("req_a", false), ("req_b", true), ("req_c", false)];
    let all_terminal = [("req_a", true), ("req_b", true), ("req_c", true)];

    assert!(
        background_wait_condition_satisfied(any, &partial),
        "wait_any must fire when any peer is terminal"
    );
    assert!(
        !background_wait_condition_satisfied(all, &partial),
        "wait_all must not fire while peers remain non-terminal"
    );
    assert!(
        background_wait_condition_satisfied(all, &all_terminal),
        "wait_all must fire when every peer is terminal"
    );

    // Verify L5 owner source exists
    let l5 = repo_root().join(
        "crates/harness-tools/tests/native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/10_background_output_wait_any_all_test.rs",
    );
    assert!(l5.is_file(), "wait-any L5 owner missing: {}", l5.display());

    let receipt = serde_json::json!({
        "schema_version": "harness-journey-wait-any-parity-v1",
        "journey_id": "JOURNEY-WAIT-ANY-ALL",
        "any_fires_on_partial": true,
        "all_blocks_on_partial": true,
        "all_fires_on_all_terminal": true,
        "l5_owner_path": l5.to_str().unwrap_or_abort(),
        "surface": "compiled_coordinator_background_wait_api",
    });
    write_json_artifact("wait-any-all", "wait-any-receipt", &receipt);
}

#[test]
fn journey_folder_trust_deny_produces_real_evidence() {
    use harness_core::folder_trust::{gate_repository_local_executable, LocalExecutableGate};

    let ws = tempfile::tempdir().unwrap_or_abort();
    let ws_path = ws.path();
    fs::create_dir_all(ws_path.join("scripts")).unwrap_or_abort();
    fs::write(
        ws_path.join("scripts/tool.sh"),
        "#!/bin/sh\necho should-not-run\n",
    )
    .unwrap_or_abort();

    let gate = gate_repository_local_executable("./scripts/tool.sh", ws_path, None);
    match gate {
        LocalExecutableGate::Denied { ref reason } => {
            assert!(
                reason.contains("folder trust"),
                "deny reason must mention folder trust: {reason}"
            );
        }
        ref other => panic!("expected Denied before spawn, got {other:?}"),
    }

    // Verify L2 and L5 owners exist
    let l2 = repo_root().join("crates/harness-core/src/folder_trust.rs");
    let l5 = repo_root().join("crates/harness-tools/src/shell_safety.rs");
    assert!(l2.is_file(), "folder trust L2 missing: {}", l2.display());
    assert!(l5.is_file(), "folder trust L5 missing: {}", l5.display());

    let receipt = serde_json::json!({
        "schema_version": "harness-journey-folder-trust-parity-v1",
        "journey_id": "JOURNEY-FOLDER-TRUST-DENY",
        "denied": true,
        "spawn_attempted": false,
        "l2_owner_path": l2.to_str().unwrap_or_abort(),
        "l5_owner_path": l5.to_str().unwrap_or_abort(),
        "surface": "compiled_core_folder_trust_gate",
    });
    write_json_artifact("folder-trust-deny", "folder-trust-receipt", &receipt);
}

#[test]
fn journey_always_approve_mode_produces_real_evidence() {
    use std::process::Command;

    // Run the compiled harness binary with golden_path scenario,
    // which produces real permission_requested + permission_resolved
    // events through the coordinator.
    let scenario_ws = tempfile::tempdir().unwrap_or_abort();
    let events_path = support::artifact_dir("always-approve-mode").join("golden-path-events.jsonl");
    let run_output = Command::new(harness_binary())
        .args([
            "run",
            "--scenario",
            "golden_path",
            "--cwd",
            scenario_ws.path().to_str().unwrap_or_abort(),
            "--out",
            events_path.to_str().unwrap_or_abort(),
            "--print-run-dir",
        ])
        .current_dir(repo_root())
        .output()
        .unwrap_or_else(|err| panic!("failed to run golden_path scenario: {err}"));
    support::assert_success("run --scenario golden_path", &run_output);

    let events_src = fs::read_to_string(&events_path).unwrap_or_abort();
    let mut has_perm_requested = false;
    let mut has_perm_resolved = false;
    let mut perm_decision_allow = false;
    for line in events_src.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let evt: Value = serde_json::from_str(line).unwrap_or_abort();
        let et = evt["payload"]["event_type"].as_str().unwrap_or("");
        if et == "permission_requested" {
            has_perm_requested = true;
        }
        if et == "permission_resolved" {
            has_perm_resolved = true;
            if evt["payload"]["data"]["decision"].as_str() == Some("allow") {
                perm_decision_allow = true;
            }
        }
    }
    assert!(
        has_perm_requested,
        "golden_path scenario must produce permission_requested event"
    );
    assert!(
        has_perm_resolved,
        "golden_path scenario must produce permission_resolved event"
    );
    assert!(
        perm_decision_allow,
        "golden_path scenario must resolve permission with allow"
    );

    // Verify L2 and L5 owners exist
    let l2 = repo_root().join("crates/harness-tui/src/app/tests/permission_modal_tests.rs");
    let l5 = repo_root().join("crates/harness-tui/src/keybindings/tests.rs");
    assert!(
        l2.is_file(),
        "always-approve L2 owner missing: {}",
        l2.display()
    );
    assert!(
        l5.is_file(),
        "always-approve L5 owner missing: {}",
        l5.display()
    );

    let receipt = serde_json::json!({
        "schema_version": "harness-journey-always-approve-parity-v1",
        "journey_id": "JOURNEY-ALWAYS-APPROVE-MODE",
        "event_log_path": events_path.to_str().unwrap_or_abort(),
        "has_permission_requested": has_perm_requested,
        "has_permission_resolved": has_perm_resolved,
        "permission_decision_allow": perm_decision_allow,
        "l2_owner_path": l2.to_str().unwrap_or_abort(),
        "l5_owner_path": l5.to_str().unwrap_or_abort(),
        "surface": "compiled_cli_scenario_plus_event_log_postconditions",
    });
    write_json_artifact("always-approve-mode", "always-approve-receipt", &receipt);
}

#[test]
fn journey_settings_editor_produces_real_evidence() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use harness_core::config::{
        read_effective_hashline_edit, settings_registry, write_project_hashline_edit,
    };
    use harness_tui::app::AppState;

    let ws = tempfile::tempdir().unwrap_or_abort();
    let config_path = ws.path().join("harness.json");
    fs::write(
        &config_path,
        r#"{
  "providers": {
    "default": {
      "type": "openai_compatible",
      "base_url": "http://127.0.0.1:8317/v1",
      "api_key": "test-key",
      "models": { "gpt-4o-mini": { "display_name": "GPT 4o mini" } }
    }
  },
  "agents": {
    "build": {
      "description": "Build work",
      "model_ref": "default:gpt-4o-mini",
      "tools": ["read"]
    }
  },
  "permissions": {
    "defaults": { "edit": "ask", "shell": "ask", "network": "deny" }
  },
  "runtime": {
    "background_tasks": {
      "default_concurrency": 2,
      "provider_concurrency": 2,
      "model_concurrency": 2,
      "stale_timeout_ms": 15000,
      "message_staleness_timeout_ms": 5000
    },
    "session_dir": ".agent-harness/sessions",
    "deterministic": { "enabled": false, "seed": 42 },
    "compaction": { "enabled": true }
  },
  "hashline_edit": true
}"#,
    )
    .unwrap_or_abort();

    let initial = read_effective_hashline_edit(&config_path).unwrap_or_abort();
    assert!(initial, "fixture hashline_edit must start true");

    let mut app = AppState::new_live(None, false, None);
    app.bind_settings_project_config(&config_path, initial, true, true, true, true, false);

    // Open settings via real slash command
    app.execute_slash_command("settings", None);
    assert!(
        app.settings_editor_is_visible(),
        "settings editor must open via /settings"
    );

    let summary = app.settings_editor_summary();
    assert!(summary.bound, "project config must be bound");
    assert!(summary.writable_paths > 0, "expected writable paths");

    // Navigate to hashline_edit and toggle
    let hashline_idx = settings_registry()
        .iter()
        .position(|entry| entry.setting_id.as_str() == "hashline_edit")
        .unwrap_or_abort();
    while app.settings_editor_selected_index() != hashline_idx {
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    assert_eq!(
        app.settings_editor_selected_id(),
        Some("hashline_edit"),
        "hashline_edit row must be selected"
    );
    app.settings_editor_activate_selected();

    let after_edit = read_effective_hashline_edit(&config_path).unwrap_or_abort();
    assert!(
        !after_edit,
        "hashline_edit effective value must be persisted as false"
    );

    // Restore and verify reload
    write_project_hashline_edit(&config_path, true).unwrap_or_abort();
    let restored = read_effective_hashline_edit(&config_path).unwrap_or_abort();
    assert!(restored, "restored value must be true");
    app.bind_settings_project_config(&config_path, restored, true, true, true, true, false);
    assert!(
        app.settings_hashline_edit(),
        "rebound AppState must reflect reloaded true"
    );

    let receipt = serde_json::json!({
        "schema_version": "harness-journey-settings-editor-parity-v1",
        "journey_id": "JOURNEY-SETTINGS-EDITOR",
        "editor_visible": app.settings_editor_is_visible(),
        "hashline_round_trip": {
            "initial": initial,
            "after_edit": after_edit,
            "after_reload": restored,
        },
        "surface": "compiled_tui_appstate_real_slash_and_key_interactions",
    });
    write_json_artifact("settings-editor", "settings-editor-receipt", &receipt);
}

#[test]
fn journey_worktree_ctrl_w_produces_real_evidence() {
    use harness_core::worktree::{create_session_worktree, CreateWorktreeOptions};

    let ws = tempfile::tempdir().unwrap_or_abort();
    let repo_root = ws.path();

    // Initialize a git repo for the worktree test
    let git_init = std::process::Command::new("git")
        .args(["init"])
        .current_dir(repo_root)
        .output()
        .unwrap_or_abort();
    assert!(
        git_init.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&git_init.stderr)
    );

    // Create an initial commit
    fs::write(repo_root.join("README.md"), "# test\n").unwrap_or_abort();
    let _git_add = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(repo_root)
        .output()
        .unwrap_or_abort();
    let _git_commit = std::process::Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(repo_root)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .unwrap_or_abort();

    let options = CreateWorktreeOptions {
        repository_root: repo_root,
        worktree_parent: None,
        slug: Some("test-wt-27"),
        start_point: None,
    };
    let created = create_session_worktree(options).unwrap_or_abort();

    assert!(created.path.is_dir(), "worktree must be created on disk");
    assert!(
        created.branch.contains("harness/wt-"),
        "branch name must contain harness/wt-, got: {}",
        created.branch
    );

    // Verify L2 owner and core module exist
    let l2 = support::repo_root().join("crates/harness-tui/src/app/tests/lifecycle_shell_tests.rs");
    let core = support::repo_root().join("crates/harness-core/src/worktree.rs");
    assert!(l2.is_file(), "worktree L2 owner missing: {}", l2.display());
    assert!(
        core.is_file(),
        "worktree core module missing: {}",
        core.display()
    );

    let receipt = serde_json::json!({
        "schema_version": "harness-journey-worktree-parity-v1",
        "journey_id": "JOURNEY-WORKTREE-CTRL-W",
        "worktree_path": created.path.to_str().unwrap_or_abort(),
        "branch_name": created.branch,
        "worktree_exists": true,
        "branch_created": true,
        "surface": "compiled_core_worktree_api",
    });
    write_json_artifact("worktree-ctrl-w", "worktree-receipt", &receipt);

    // Cleanup
    let _ = std::process::Command::new("git")
        .args(["worktree", "remove", "--force"])
        .arg(&created.path)
        .current_dir(repo_root)
        .output();
}

// ---------------------------------------------------------------------------
// Aggregator: all journeys covered, all postconditions pass
// ---------------------------------------------------------------------------

#[test]
fn journey_all_rows_covered_all_postconditions_pass() {
    let manifest = checked_in_manifest();
    let rows = journey_rows(&manifest);
    let rows_expected = JOURNEY_IDS.len();

    // Collect all postconditions from all journeys
    let postconditions = collect_all_postconditions();

    // Verify all postconditions are true
    let all_pass = verify_all_postconditions(&postconditions);
    assert!(
        all_pass,
        "not all journey postconditions pass: {postconditions}"
    );

    // Write the result
    let result = serde_json::json!({
        "rows_covered": rows.len(),
        "rows_expected": rows_expected,
        "postconditions": postconditions,
        "all_postconditions_pass": all_pass,
    });
    write_json_artifact("aggregator", "journey-parity-result", &result);

    assert_eq!(rows.len(), rows_expected);
}

// ---------------------------------------------------------------------------
// Failure recovery: mutate one postcondition and verify rejection
// ---------------------------------------------------------------------------

#[test]
fn journey_failure_mutated_postcondition_rejected() {
    let mutated_rejected = verify_mutated_postcondition_rejected();
    assert!(
        mutated_rejected,
        "mutated postcondition must be rejected (not all true)"
    );

    let result = serde_json::json!({
        "outcome": "expected_failure",
        "mutated_row_rejected": true,
        "surface": "journey",
        "case": "failure",
        "mutate": "one-postcondition",
    });
    write_json_artifact("failure", "journey-parity-failure-result", &result);
}

// ---------------------------------------------------------------------------
// Manifest structural validation for journey rows
// ---------------------------------------------------------------------------

#[test]
fn journey_rows_have_correct_structure() {
    let manifest = checked_in_manifest();
    let rows = journey_rows(&manifest);

    for row in &rows {
        let id = row["behavior_id"].as_str().unwrap_or_abort();
        assert_eq!(
            row["row_kind"].as_str(),
            Some("journey"),
            "{id}: row_kind must be journey"
        );
        assert_eq!(
            row["status"].as_str(),
            Some("pass"),
            "{id}: status must be pass (promoted 2026-07-30 signoff-parity clean room)"
        );
        assert!(
            !row["capability_id"].as_str().unwrap_or("").is_empty(),
            "{id}: capability_id must not be empty"
        );
        assert!(
            !row["backend_owner"].as_str().unwrap_or("").is_empty(),
            "{id}: backend_owner must not be empty"
        );
        let l1_prefix =
            "target/test-lanes/latest/signoff-parity/evidence/reference/freeze/journey-";
        let l4_prefix = "target/test-lanes/latest/signoff-parity/evidence/receipts/journey-";
        let l1 = row["evidence_paths"]["L1"].as_str().unwrap_or("");
        assert!(
            l1.starts_with(l1_prefix) && l1.ends_with("-l1-ref-v1/"),
            "{id}: L1 must be a canonical reference-CLI freeze path, got {l1}"
        );
        let l4 = row["evidence_paths"]["L4"].as_str().unwrap_or("");
        assert!(
            l4.starts_with(l4_prefix) && l4.ends_with("-l4-differential-v1.json"),
            "{id}: L4 must be a canonical differential receipt path, got {l4}"
        );
        assert!(
            !row["evidence_paths"]["L2"]
                .as_str()
                .unwrap_or("")
                .is_empty(),
            "{id}: L2 must not be empty"
        );
        assert!(
            !row["evidence_paths"]["L3"]
                .as_str()
                .unwrap_or("")
                .is_empty(),
            "{id}: L3 must not be empty"
        );
        assert!(
            !row["evidence_paths"]["L5"]
                .as_str()
                .unwrap_or("")
                .is_empty(),
            "{id}: L5 must not be empty"
        );
        assert!(
            !row["evidence_paths"]["L6"]
                .as_str()
                .unwrap_or("")
                .is_empty(),
            "{id}: L6 must not be empty"
        );
    }
}
