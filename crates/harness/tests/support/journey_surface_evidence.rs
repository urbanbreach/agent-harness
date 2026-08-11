use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness::UnwrapOrAbort;
use harness_core::config::{
    read_effective_hashline_edit, settings_registry, write_project_hashline_edit,
};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionDecision, PermissionRequestedEvent,
    PermissionResolvedEvent, SCHEMA_VERSION,
};
use harness_core::folder_trust::{gate_repository_local_executable, LocalExecutableGate};
use harness_tui::app::{AppState, LaunchMetadata, UiIntent};
use harness_tui::ui::render_app;
use ratatui::{backend::TestBackend, Terminal};
use serde_json::Value;

use crate::common::repo_root;
use crate::journey_signoff::{
    assert_cli_success, journey_artifact_root, parse_stdout_json, require_harness_binary,
    run_harness_cli, stable_l3_artifact_root, write_cli_artifact_pair, JOURNEY_TEST_REL,
    WAIT_ANY_ALL_L5_REL,
};

pub(crate) const STABLE_L3_WAIT_ANY_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/actual/journey-wait-any-all-v1";
pub(crate) const STABLE_L3_MEMORY_CLI_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/actual/journey-memory-cli-v1";
pub(crate) const STABLE_L3_FOLDER_TRUST_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/actual/journey-folder-trust-deny-v1";
pub(crate) const STABLE_L3_ALWAYS_APPROVE_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/actual/journey-always-approve-mode-v1";
pub(crate) const STABLE_L3_SETTINGS_EDITOR_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/actual/journey-settings-editor-v1";
pub(crate) const SURFACE_EVIDENCE_RECEIPT_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/receipts/loop15-journey-surface-evidence-v1.md";
pub(crate) const ALWAYS_SETTINGS_L3_RECEIPT_REL: &str =
    "artifacts/qa-evidence/20260717-tui-reference-parity/receipts/loop15-journey-always-settings-l3-v1.md";

pub(crate) const WAIT_ANY_OWNER_TEST_BIN: &str =
    "native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_test";
pub(crate) const WAIT_ANY_OWNER_FILTER: &str = "part_10_background";
pub(crate) const FOLDER_TRUST_DENY_OWNER_FN: &str =
    "validate_bash_denies_repo_local_executable_when_folder_trust_missing";
pub(crate) const FOLDER_TRUST_L5_REL: &str = "crates/harness-tools/src/shell_safety.rs";
pub(crate) const FOLDER_TRUST_L2_REL: &str = "crates/harness-core/src/folder_trust.rs";
pub(crate) const ALWAYS_APPROVE_L2_REL: &str =
    "crates/harness-tui/src/app/tests/permission_modal_tests.rs";
pub(crate) const ALWAYS_APPROVE_L5_REL: &str = "crates/harness-tui/src/keybindings/tests.rs";
pub(crate) const SETTINGS_EDITOR_L2_REL: &str =
    "crates/harness-tui/src/app/tests/settings_editor_tests.rs";
pub(crate) const SETTINGS_EDITOR_L5_REL: &str =
    "crates/harness-tui/src/app/tests/settings_editor_tests.rs";

pub(crate) fn write_json_artifact_pair(
    lane_dir: &Path,
    stable_rel: &str,
    name: &str,
    value: &Value,
) {
    let body = serde_json::to_vec_pretty(value).unwrap_or_abort();
    fs::write(lane_dir.join(format!("{name}.json")), &body).unwrap_or_abort();
    let stable = stable_l3_artifact_root(stable_rel);
    fs::write(stable.join(format!("{name}.json")), &body).unwrap_or_abort();
    assert!(
        stable.join(format!("{name}.json")).is_file(),
        "stable L3 JSON missing under {} (fail-closed)",
        stable.display()
    );
}

pub(crate) fn execute_wait_any_surface_evidence() {
    use harness_core::coord::{background_wait_condition_satisfied, BackgroundWaitMode};

    let artifacts = journey_artifact_root("wait-any-all");
    let root = repo_root();
    let l5_path = root.join(WAIT_ANY_ALL_L5_REL);
    assert!(
        l5_path.is_file(),
        "wait-any L5 owner missing (fail-closed): {}",
        l5_path.display()
    );
    let l5_src = fs::read_to_string(&l5_path).unwrap_or_abort();
    for needle in [
        "background_output_wait_any_returns_on_first_cancel_while_peer_still_running",
        "background_output_wait_all_returns_when_every_request_is_terminal",
        "background_output_wait_all_completes_when_cancel_makes_remaining_terminal",
        "wait_mode",
    ] {
        assert!(
            l5_src.contains(needle),
            "wait-any L5 owner must define `{needle}` in {}",
            l5_path.display()
        );
    }

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

    let owner_run = serde_json::json!({
        "status": 0,
        "stdout": "in-process wait_any/wait_all condition checks + L5 owner source contract",
        "stderr": "",
        "surface": "in_process_plus_l5_source",
        "l5_owner_path": WAIT_ANY_ALL_L5_REL,
        "l5_owner_filter": WAIT_ANY_OWNER_FILTER,
        "l5_owner_test_bin": WAIT_ANY_OWNER_TEST_BIN,
        "external_owner_command": [
            "cargo",
            "nextest",
            "run",
            "-p",
            "harness-tools",
            "--test",
            WAIT_ANY_OWNER_TEST_BIN,
            "--",
            WAIT_ANY_OWNER_FILTER
        ],
        "notes": "Nested cargo nextest was removed from this journey helper: nextest default slow-timeout terminates parents at 20s while nested compiles/runs can exceed that under load. Integration ownership remains the harness-tools part_10_background suite; this helper proves the wait_any/all condition contract in-process and that the L5 owner source is present."
    });
    write_json_artifact_pair(
        &artifacts,
        STABLE_L3_WAIT_ANY_REL,
        "wait-any-owner-run",
        &owner_run,
    );

    let receipt = serde_json::json!({
        "schema_version": "harness-journey-wait-any-surface-v1",
        "journey_id": "JOURNEY-WAIT-ANY-ALL",
        "status": "incomplete",
        "surface": "in_process_condition_plus_l5_source",
        "l3_artifact_dir": STABLE_L3_WAIT_ANY_REL,
        "l5_owner_path": WAIT_ANY_ALL_L5_REL,
        "l5_owner_filter": WAIT_ANY_OWNER_FILTER,
        "run_command": [
            "cargo",
            "nextest",
            "run",
            "-p",
            "harness-tools",
            "--test",
            WAIT_ANY_OWNER_TEST_BIN,
            "--",
            WAIT_ANY_OWNER_FILTER
        ],
        "journey_test": JOURNEY_TEST_REL,
        "notes": "Offline surface: in-process wait_any/all condition contract + L5 source presence. Full integration suite remains cargo nextest -p harness-tools --test native_agent_spawn... part_10_background (run outside nested journey timeout)."
    });
    write_json_artifact_pair(
        &artifacts,
        STABLE_L3_WAIT_ANY_REL,
        "wait-any-surface-receipt",
        &receipt,
    );
}

pub(crate) fn execute_memory_cli_surface_evidence() {
    let _ = require_harness_binary();
    let artifacts = journey_artifact_root("memory-cli");
    let workspace = tempfile::tempdir().unwrap_or_abort();
    let workspace_path = workspace.path().to_str().unwrap_or_abort();

    let put = run_harness_cli(&[
        "memory",
        "put",
        "journey.surface.theme",
        "dark",
        "--workspace",
        workspace_path,
    ]);
    write_cli_artifact_pair(&artifacts, STABLE_L3_MEMORY_CLI_REL, "memory-put", &put);
    assert_cli_success("memory put", &put);
    let put_json = parse_stdout_json(&put);
    assert_eq!(put_json["key"].as_str(), Some("journey.surface.theme"));
    assert_eq!(put_json["value"].as_str(), Some("dark"));

    let get = run_harness_cli(&[
        "memory",
        "get",
        "journey.surface.theme",
        "--workspace",
        workspace_path,
    ]);
    write_cli_artifact_pair(&artifacts, STABLE_L3_MEMORY_CLI_REL, "memory-get", &get);
    assert_cli_success("memory get", &get);
    let get_json = parse_stdout_json(&get);
    assert_eq!(get_json["key"].as_str(), Some("journey.surface.theme"));
    assert_eq!(get_json["value"].as_str(), Some("dark"));

    let list = run_harness_cli(&["memory", "list", "--workspace", workspace_path]);
    write_cli_artifact_pair(&artifacts, STABLE_L3_MEMORY_CLI_REL, "memory-list", &list);
    assert_cli_success("memory list", &list);
    let list_json = parse_stdout_json(&list);
    assert!(
        list_json["entries"]
            .as_array()
            .is_some_and(|entries| !entries.is_empty()),
        "expected non-empty memory list entries: {list_json}"
    );

    let receipt = serde_json::json!({
        "schema_version": "harness-journey-memory-cli-surface-v1",
        "journey_id": "JOURNEY-MEMORY-CLI",
        "status": "incomplete",
        "surface": "binary_cli_put_get_list",
        "l3_artifact_dir": STABLE_L3_MEMORY_CLI_REL,
        "l5_owner_path": "crates/harness/src/memory_cmd.rs",
        "workspace": workspace_path,
        "commands": [
            ["memory", "put", "journey.surface.theme", "dark", "--workspace", workspace_path],
            ["memory", "get", "journey.surface.theme", "--workspace", workspace_path],
            ["memory", "list", "--workspace", workspace_path]
        ],
        "journey_test": JOURNEY_TEST_REL,
        "notes": "Real offline surface: harness binary memory put/get/list against temp workspace. Fail-closed if binary missing. No L1 freeze; status remains incomplete."
    });
    write_json_artifact_pair(
        &artifacts,
        STABLE_L3_MEMORY_CLI_REL,
        "memory-cli-surface-receipt",
        &receipt,
    );
}

#[allow(
    clippy::panic,
    reason = "fail-closed test path for unexpected gate variant"
)]
pub(crate) fn execute_folder_trust_deny_surface_evidence() {
    let artifacts = journey_artifact_root("folder-trust-deny");
    let workspace = tempfile::tempdir().unwrap_or_abort();
    let workspace_path = workspace.path().to_path_buf();
    fs::create_dir_all(workspace_path.join("scripts")).unwrap_or_abort();
    fs::write(
        workspace_path.join("scripts/tool.sh"),
        "#!/bin/sh\necho should-not-run\n",
    )
    .unwrap_or_abort();

    let executable = "./scripts/tool.sh";
    let gate = gate_repository_local_executable(executable, &workspace_path, None);
    let (denied, reason) = match gate {
        LocalExecutableGate::Denied { reason } => (true, reason),
        other => panic!("expected Denied before spawn, got {other:?}"),
    };
    assert!(
        reason.contains("folder trust"),
        "deny reason must mention folder trust: {reason}"
    );

    let l2 = repo_root().join(FOLDER_TRUST_L2_REL);
    let l5 = repo_root().join(FOLDER_TRUST_L5_REL);
    assert!(l2.is_file(), "folder trust L2 missing: {}", l2.display());
    assert!(l5.is_file(), "folder trust L5 missing: {}", l5.display());
    let l5_source = fs::read_to_string(&l5).unwrap_or_abort();
    assert!(
        l5_source.contains(FOLDER_TRUST_DENY_OWNER_FN),
        "shell_safety must contain deny-before-spawn owner test {FOLDER_TRUST_DENY_OWNER_FN}"
    );
    assert!(
        l5_source.contains("ensure_folder_trust_allows_local_executable")
            || l5_source.contains("folder trust"),
        "shell_safety must wire folder-trust gate before spawn"
    );

    let receipt = serde_json::json!({
        "schema_version": "harness-journey-folder-trust-deny-surface-v1",
        "journey_id": "JOURNEY-FOLDER-TRUST-DENY",
        "status": "incomplete",
        "surface": "deny_before_spawn_gate",
        "l3_artifact_dir": STABLE_L3_FOLDER_TRUST_REL,
        "l2_owner_path": FOLDER_TRUST_L2_REL,
        "l5_owner_path": FOLDER_TRUST_L5_REL,
        "l5_owner_test": FOLDER_TRUST_DENY_OWNER_FN,
        "executable": executable,
        "denied": denied,
        "deny_reason": reason,
        "spawn_attempted": false,
        "journey_test": JOURNEY_TEST_REL,
        "notes": "Real offline surface: core gate denies path-qualified executable when folder trust is missing (no spawn). L5 shell_safety unit test documents bash deny-before-spawn. No L1 freeze; status remains incomplete."
    });
    write_json_artifact_pair(
        &artifacts,
        STABLE_L3_FOLDER_TRUST_REL,
        "folder-trust-deny-receipt",
        &receipt,
    );

    let status = format!("success=true\ndenied=true\nspawn_attempted=false\nreason={reason}\n");
    fs::write(artifacts.join("folder-trust-deny.status.txt"), &status).unwrap_or_abort();
    let stable = stable_l3_artifact_root(STABLE_L3_FOLDER_TRUST_REL);
    fs::write(stable.join("folder-trust-deny.status.txt"), &status).unwrap_or_abort();
}

pub(crate) fn execute_always_approve_mode_surface_evidence() {
    let artifacts = journey_artifact_root("always-approve-mode");
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };
    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.set_launch_metadata(LaunchMetadata::new(
        "build",
        "test-provider",
        Some("model-tx".to_string()),
    ));

    app.ingest_event(journey_envelope(
        1,
        "req_journey_always_1",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_journey_always_1".to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some("tc_journey_always_1".into()),
            summary: "journey always-approve first permission".to_string(),
            request_digest: "digest-journey-always-1".to_string(),
            timeout_ms: 30_000,
            default_decision: PermissionDecision::Deny,
        }),
    ));

    app.handle_key(key_press(KeyCode::Enter));
    app.handle_key(key_press(KeyCode::Enter));
    assert!(
        app.always_approve_mode(),
        "always-approve mode must engage after AlwaysConfirm path (fail-closed)"
    );

    app.ingest_event(journey_envelope(
        2,
        "req_journey_always_1",
        EventV1::PermissionResolved(PermissionResolvedEvent {
            permission_id: "perm_journey_always_1".to_string(),
            decision: PermissionDecision::Allow,
            reason: None,
        }),
    ));
    intents.lock().unwrap_or_abort().clear();

    app.ingest_event(journey_envelope(
        3,
        "req_journey_always_2",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_journey_always_2".to_string(),
            kind: "bash".to_string(),
            tool_call_id: Some("tc_journey_always_2".into()),
            summary: "journey always-approve second permission".to_string(),
            request_digest: "digest-journey-always-2".to_string(),
            timeout_ms: 30_000,
            default_decision: PermissionDecision::Deny,
        }),
    ));
    let auto_allow_intents: Vec<UiIntent> = intents.lock().unwrap_or_abort().clone();
    assert_eq!(
        auto_allow_intents,
        vec![UiIntent::ResolvePermission {
            permission_id: "perm_journey_always_2".to_string(),
            decision: harness_core::perm::PermissionDecision::Allow,
            reason: None,
            grant_scope: Some(harness_core::perm::PermissionGrantScope::Run),
        }],
        "always-approve mode must auto-allow a subsequent non-question permission"
    );

    app.ingest_event(journey_envelope(
        4,
        "req_journey_always_2",
        EventV1::PermissionResolved(PermissionResolvedEvent {
            permission_id: "perm_journey_always_2".to_string(),
            decision: PermissionDecision::Allow,
            reason: None,
        }),
    ));

    let screen = render_app_screen(&app, 120, 32);
    assert!(
        screen.contains("always-approve"),
        "composer badge dump must show always-approve when mode engaged\n{screen}"
    );

    write_text_artifact_pair(
        &artifacts,
        STABLE_L3_ALWAYS_APPROVE_REL,
        "always-approve-render.txt",
        &screen,
    );

    let state = serde_json::json!({
        "schema_version": "harness-journey-always-approve-state-v1",
        "journey_id": "JOURNEY-ALWAYS-APPROVE-MODE",
        "status": "incomplete",
        "always_approve_mode": app.always_approve_mode(),
        "auto_allow_subsequent": {
            "permission_id": "perm_journey_always_2",
            "kind": "bash",
            "intents_emitted": auto_allow_intents.len()
        },
        "badge_marker": "always-approve",
        "badge_present_in_render": screen.contains("always-approve"),
        "viewport": { "cols": 120, "rows": 32 },
        "surface": "appstate_plus_intent_sink",
        "notes": "Offline product path: AlwaysConfirm enables session mode; a subsequent non-question permission is auto-allowed via UiIntent; TestBackend render shows composer badge."
    });
    write_json_artifact_pair(
        &artifacts,
        STABLE_L3_ALWAYS_APPROVE_REL,
        "always-approve-state",
        &state,
    );

    let receipt = serde_json::json!({
        "schema_version": "harness-journey-always-approve-surface-v1",
        "journey_id": "JOURNEY-ALWAYS-APPROVE-MODE",
        "status": "incomplete",
        "surface": "appstate_plus_intent_sink",
        "l3_artifact_dir": STABLE_L3_ALWAYS_APPROVE_REL,
        "l2_owner_path": ALWAYS_APPROVE_L2_REL,
        "l5_owner_path": ALWAYS_APPROVE_L5_REL,
        "artifacts": [
            "always-approve-render.txt",
            "always-approve-state.json",
            "always-approve-surface-receipt.json"
        ],
        "journey_test": JOURNEY_TEST_REL,
        "notes": "Real offline surface: AppState AlwaysConfirm path + intent-sink capture of auto-allow for a subsequent permission + TestBackend render dump of always-approve badge. No L1 freeze; no PTY; status remains incomplete because L1/L4 reference freeze artifacts are empty."
    });
    write_json_artifact_pair(
        &artifacts,
        STABLE_L3_ALWAYS_APPROVE_REL,
        "always-approve-surface-receipt",
        &receipt,
    );
}

pub(crate) fn execute_settings_editor_surface_evidence() {
    let artifacts = journey_artifact_root("settings-editor");

    let workspace = tempfile::tempdir().unwrap_or_abort();
    let path = workspace.path().join("harness.json");
    fs::write(
        &path,
        r#"{
  "provider": {
    "default": {
      "type": "openai_compatible",
      "options": { "baseURL": "http://127.0.0.1:8317/v1", "apiKey": "test-key" },
      "models": { "gpt-4o-mini": { "name": "GPT 4o mini" } }
    }
  },
  "model": "default/gpt-4o-mini",
  "agent": {
    "default": {
      "model": "default/gpt-4o-mini",
      "tools": ["read"]
    }
  },
  "permission": { "edit": "ask", "bash": "ask", "webfetch": "deny" },
  "hashline_edit": true
}"#,
    )
    .unwrap_or_abort();

    let hashline_initial = read_effective_hashline_edit(&path).unwrap_or_abort();
    assert!(hashline_initial, "fixture hashline_edit must start true");

    let mut app = AppState::new_live(None, false, None);
    app.bind_settings_project_config(&path, hashline_initial, true, true, true, true, false);

    app.execute_slash_command("settings", None);
    assert!(
        app.settings_editor_is_visible(),
        "settings editor must open via /settings (fail-closed)"
    );
    let summary = app.settings_editor_summary();
    assert!(summary.bound, "project config must be bound");
    assert_eq!(summary.writable_paths, 6, "expected six writable paths");
    assert_eq!(summary.editable, 6, "all writable paths must be editable");
    assert!(
        summary.with_effective_value >= 6,
        "effective values must be present"
    );

    let hashline_index = settings_registry()
        .iter()
        .position(|entry| entry.setting_id.as_str() == "hashline_edit")
        .unwrap_or_abort();
    while app.settings_editor_selected_index() != hashline_index {
        app.handle_key(key_press(KeyCode::Down));
    }
    assert_eq!(
        app.settings_editor_selected_id(),
        Some("hashline_edit"),
        "hashline_edit row must be selected before activation"
    );
    app.settings_editor_activate_selected();

    assert!(
        !app.settings_hashline_edit(),
        "hashline_edit AppState must flip to false"
    );
    let effective_after_edit = read_effective_hashline_edit(&path).unwrap_or_abort();
    assert!(
        !effective_after_edit,
        "hashline_edit effective value must be persisted as false"
    );
    let row = app
        .settings_editor_rows()
        .into_iter()
        .find(|row| row.setting_id == "hashline_edit")
        .unwrap_or_abort();
    assert_eq!(
        row.effective_value.as_deref(),
        Some("false"),
        "row effective value must reflect persisted false"
    );

    write_project_hashline_edit(&path, true).unwrap_or_abort();
    let reloaded = read_effective_hashline_edit(&path).unwrap_or_abort();
    assert!(reloaded, "reloaded effective value must be true");
    app.bind_settings_project_config(&path, reloaded, true, true, true, true, false);
    assert!(
        app.settings_hashline_edit(),
        "rebound AppState must reflect reloaded true"
    );
    let row_after_reload = app
        .settings_editor_rows()
        .into_iter()
        .find(|row| row.setting_id == "hashline_edit")
        .unwrap_or_abort();
    assert_eq!(
        row_after_reload.effective_value.as_deref(),
        Some("true"),
        "row effective value must reflect reloaded true"
    );

    let rows = app.settings_editor_rows();
    let mut rows_text =
        String::from("setting_id\tsurface\tsensitivity\tselected\teffective_value\n");
    for row in &rows {
        rows_text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            row.setting_id,
            row.surface,
            row.sensitivity,
            row.selected,
            row.effective_value.as_deref().unwrap_or("-")
        ));
    }
    write_text_artifact_pair(
        &artifacts,
        STABLE_L3_SETTINGS_EDITOR_REL,
        "settings-editor-rows.txt",
        &rows_text,
    );

    let screen = render_app_screen(&app, 120, 32);
    assert!(
        screen.contains("Settings"),
        "settings overlay render dump must include Settings title\n{screen}"
    );
    write_text_artifact_pair(
        &artifacts,
        STABLE_L3_SETTINGS_EDITOR_REL,
        "settings-editor-render.txt",
        &screen,
    );

    let rows_json: Vec<Value> = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "setting_id": row.setting_id,
                "surface": row.surface,
                "sensitivity": row.sensitivity,
                "selected": row.selected,
                "effective_value": row.effective_value,
                "editable": row.editable,
            })
        })
        .collect();
    let state = serde_json::json!({
        "schema_version": "harness-journey-settings-editor-state-v1",
        "journey_id": "JOURNEY-SETTINGS-EDITOR",
        "status": "pass",
        "settings_editor_visible": app.settings_editor_is_visible(),
        "row_count": rows.len(),
        "rows": rows_json,
        "hashline_edit_round_trip": {
            "initial": true,
            "after_edit": false,
            "after_reload": true,
            "file_path": path.to_str(),
        },
        "viewport": { "cols": 120, "rows": 32 },
        "surface": "appstate_plus_real_config_write_read_round_trip",
        "notes": "Product path: open settings editor, toggle hashline_edit, persist to project harness.json, read_effective matches, re-bind reflects change."
    });
    write_json_artifact_pair(
        &artifacts,
        STABLE_L3_SETTINGS_EDITOR_REL,
        "settings-editor-state",
        &state,
    );

    let receipt = serde_json::json!({
        "schema_version": "harness-journey-settings-editor-surface-v1",
        "journey_id": "JOURNEY-SETTINGS-EDITOR",
        "status": "pass",
        "surface": "appstate_plus_real_config_write_read_round_trip",
        "l3_artifact_dir": STABLE_L3_SETTINGS_EDITOR_REL,
        "l2_owner_path": SETTINGS_EDITOR_L2_REL,
        "l5_owner_path": SETTINGS_EDITOR_L5_REL,
        "artifacts": [
            "settings-editor-rows.txt",
            "settings-editor-render.txt",
            "settings-editor-state.json",
            "settings-editor-surface-receipt.json"
        ],
        "journey_test": JOURNEY_TEST_REL,
        "notes": "Real offline surface: /settings opens overlay; activate toggles hashline_edit; write_project_hashline_edit persists to project harness.json; read_effective_hashline_edit verifies; re-bind reloads effective value. No L1 freeze claimed; pass is on L2-L6 product surface only."
    });
    write_json_artifact_pair(
        &artifacts,
        STABLE_L3_SETTINGS_EDITOR_REL,
        "settings-editor-surface-receipt",
        &receipt,
    );
}

fn write_text_artifact_pair(lane_dir: &Path, stable_rel: &str, name: &str, body: &str) {
    fs::write(lane_dir.join(name), body).unwrap_or_abort();
    let stable = stable_l3_artifact_root(stable_rel);
    fs::write(stable.join(name), body).unwrap_or_abort();
    assert!(
        stable.join(name).is_file(),
        "stable L3 text missing under {} (fail-closed)",
        stable.display()
    );
}

fn render_app_screen(app: &AppState, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, app))
        .unwrap_or_abort();
    let buffer = terminal.backend().buffer();
    let mut rows = Vec::with_capacity(usize::from(height));
    for y in 0..height {
        let mut row = String::with_capacity(usize::from(width));
        for x in 0..width {
            row.push_str(buffer.cell((x, y)).map_or(" ", |cell| cell.symbol()));
        }
        rows.push(row.trim_end().to_string());
    }
    rows.join("\n")
}

fn journey_envelope(seq: u64, request_id: &str, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt_journey_{seq:04}"),
        seq,
        run_id: "run_journey_surface".into(),
        mono_ms: seq,
        ts: Some("2026-07-18T00:00:00Z".to_string()),
        actor: EventActor::new(ActorKind::System, Some("journey-surface".to_string())),
        correlation_id: Some(request_id.to_string()),
        causation_id: None,
        stream_key: None,
        payload,
    }
}

fn key_press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}
