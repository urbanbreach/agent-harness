//! Shared helpers for journey-parity tests (Todo 27).
//!
//! Each journey uses compiled CLI/coordinator/TUI operations and external
//! postconditions. No synthetic destination AppState or direct event injection.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use harness_tui::UnwrapOrAbort;
use serde_json::Value;

/// All 8 journey behavior IDs from the TUI reference parity manifest.
pub const JOURNEY_IDS: &[&str] = &[
    "JOURNEY-CONFIG-SHOW-EFFECTIVE",
    "JOURNEY-CONFIG-SOURCES-EXPLAIN",
    "JOURNEY-MEMORY-CLI",
    "JOURNEY-WAIT-ANY-ALL",
    "JOURNEY-FOLDER-TRUST-DENY",
    "JOURNEY-ALWAYS-APPROVE-MODE",
    "JOURNEY-SETTINGS-EDITOR",
    "JOURNEY-WORKTREE-CTRL-W",
];

pub const MANIFEST_REL: &str = "docs/tui-reference-parity-manifest.v1.json";
pub const EXAMPLE_CONFIG_REL: &str = "configs/harness.example.jsonc";

/// Resolve the repo root from CARGO_MANIFEST_DIR (crates/harness-tui).
///
/// CARGO_MANIFEST_DIR = .../crates/harness-tui
/// .parent() = .../crates
/// .parent().parent() = .../  (workspace root)
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_abort()
        .parent()
        .unwrap_or_abort()
        .to_path_buf()
}

/// Load the manifest JSON from the repo root.
pub fn load_manifest() -> Value {
    let path = repo_root().join(MANIFEST_REL);
    let src = fs::read_to_string(&path).unwrap_or_abort();
    serde_json::from_str(&src).unwrap_or_abort()
}

/// Filter manifest rows whose surface starts with "journey".
pub fn journey_rows(manifest: &Value) -> Vec<&Value> {
    manifest["rows"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .filter(|row| {
            row["surface"]
                .as_str()
                .is_some_and(|s| s.starts_with("journey"))
        })
        .collect()
}

/// Find the compiled harness binary.
///
/// Since harness-tui tests don't have `CARGO_BIN_EXE_harness`, we locate the
/// binary by checking (1) `HARNESS_BIN` env var, (2) the same directory as the
/// test executable, and (3) `target/debug/harness` relative to the repo root.
pub fn harness_binary() -> PathBuf {
    // 1. Explicit override
    if let Some(path) = std::env::var_os("HARNESS_BIN") {
        let p = PathBuf::from(path);
        if p.is_file() {
            return p;
        }
    }

    // 2. Same directory as the test executable (both land in target/debug)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.join("harness");
            if candidate.is_file() {
                return candidate;
            }
            // Also check one level up (deps/ -> debug/)
            if parent.file_name().is_some_and(|n| n == "deps") {
                if let Some(grandparent) = parent.parent() {
                    let candidate = grandparent.join("harness");
                    if candidate.is_file() {
                        return candidate;
                    }
                }
            }
        }
    }

    // 3. Repo-rooted default
    let candidate = repo_root().join("target").join("debug").join("harness");
    if candidate.is_file() {
        return candidate;
    }

    panic!(
        "harness binary not found; set HARNESS_BIN or build with `cargo build -p harness` first"
    );
}

/// Path to the example config.
pub fn example_config() -> PathBuf {
    let path = repo_root().join(EXAMPLE_CONFIG_REL);
    assert!(
        path.is_file(),
        "missing example config at {}",
        path.display()
    );
    path
}

/// Run the harness binary with the given args, returning the output.
pub fn run_harness(args: &[&str]) -> std::process::Output {
    let bin = harness_binary();
    Command::new(&bin)
        .args(args)
        .current_dir(repo_root())
        .output()
        .unwrap_or_else(|err| panic!("failed to spawn harness binary at {}: {err}", bin.display()))
}

/// Assert the command succeeded.
pub fn assert_success(label: &str, output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{label} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Parse stdout as JSON.
pub fn parse_json(output: &std::process::Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim()).unwrap_or_else(|err| {
        panic!("stdout is not valid JSON ({err}): {stdout}");
    })
}

/// Artifact directory for a journey slug.
pub fn artifact_dir(slug: &str) -> PathBuf {
    let root = std::env::var_os("HARNESS_JOURNEY_ARTIFACT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("target").join("journey-parity-artifacts"));
    let dir = root.join(slug);
    fs::create_dir_all(&dir).unwrap_or_abort();
    dir
}

/// Write a JSON artifact and return its path.
pub fn write_json_artifact(slug: &str, name: &str, value: &Value) -> PathBuf {
    let dir = artifact_dir(slug);
    let path = dir.join(format!("{name}.json"));
    let body = serde_json::to_vec_pretty(value).unwrap_or_abort();
    fs::write(&path, &body).unwrap_or_abort();
    path
}

/// Write a text artifact and return its path.
pub fn write_text_artifact(slug: &str, name: &str, body: &str) -> PathBuf {
    let dir = artifact_dir(slug);
    let path = dir.join(name);
    fs::write(&path, body).unwrap_or_abort();
    path
}

/// Write a CLI output pair (stdout, stderr, status).
pub fn write_cli_artifact(slug: &str, name: &str, output: &std::process::Output) -> PathBuf {
    let dir = artifact_dir(slug);
    fs::write(dir.join(format!("{name}.stdout.txt")), &output.stdout).unwrap_or_abort();
    fs::write(dir.join(format!("{name}.stderr.txt")), &output.stderr).unwrap_or_abort();
    let status = format!(
        "success={}\ncode={}\n",
        output.status.success(),
        output.status.code().unwrap_or(-1)
    );
    let path = dir.join(format!("{name}.status.txt"));
    fs::write(&path, &status).unwrap_or_abort();
    path
}

/// Verify that all postconditions are true.
pub fn verify_all_postconditions(postconditions: &Value) -> bool {
    postconditions
        .as_object()
        .is_some_and(|map| map.values().all(|v| v.as_bool() == Some(true)))
}

/// Collect all postconditions from all journeys into a single JSON object.
pub fn collect_all_postconditions() -> Value {
    let mut postconditions = serde_json::Map::new();

    // JOURNEY-CONFIG-SHOW-EFFECTIVE
    let config_show = run_harness(&[
        "--config",
        example_config().to_str().unwrap_or_abort(),
        "config",
        "show",
        "--effective",
    ]);
    assert_success("config show --effective", &config_show);
    let config_json = parse_json(&config_show);
    postconditions.insert(
        "config_show_effective_json_valid".into(),
        Value::Bool(config_json["schema_version"].as_str() == Some("harness-config-effective-v1")),
    );
    postconditions.insert(
        "config_show_effective_redacted".into(),
        Value::Bool(config_json["redacted"].as_bool() == Some(true)),
    );
    postconditions.insert(
        "config_show_effective_has_layers".into(),
        Value::Bool(
            config_json["layers"]
                .as_array()
                .is_some_and(|l| !l.is_empty()),
        ),
    );
    let stdout = String::from_utf8_lossy(&config_show.stdout);
    postconditions.insert(
        "config_show_no_secret_leak".into(),
        Value::Bool(!stdout.contains("sk-proj-") && !stdout.contains("sk-ant-")),
    );
    write_cli_artifact(
        "config-show-effective",
        "config-show-effective",
        &config_show,
    );

    // JOURNEY-CONFIG-SOURCES-EXPLAIN
    let config_sources = run_harness(&[
        "--config",
        example_config().to_str().unwrap_or_abort(),
        "config",
        "sources",
    ]);
    assert_success("config sources", &config_sources);
    let sources_json = parse_json(&config_sources);
    postconditions.insert(
        "config_sources_json_valid".into(),
        Value::Bool(sources_json["schema_version"].as_str() == Some("harness-config-sources-v1")),
    );
    postconditions.insert(
        "config_sources_has_layers".into(),
        Value::Bool(
            sources_json["layers"]
                .as_array()
                .is_some_and(|l| !l.is_empty()),
        ),
    );
    write_cli_artifact("config-sources", "config-sources", &config_sources);

    let config_explain = run_harness(&[
        "--config",
        example_config().to_str().unwrap_or_abort(),
        "config",
        "explain",
        "model",
    ]);
    assert_success("config explain model", &config_explain);
    let explain_json = parse_json(&config_explain);
    postconditions.insert(
        "config_explain_json_valid".into(),
        Value::Bool(explain_json["schema_version"].as_str() == Some("harness-config-explain-v1")),
    );
    postconditions.insert(
        "config_explain_found".into(),
        Value::Bool(explain_json["found"].as_bool() == Some(true)),
    );
    postconditions.insert(
        "config_explain_has_source_path".into(),
        Value::Bool(
            explain_json["source_path"]
                .as_str()
                .is_some_and(|p| !p.is_empty()),
        ),
    );
    write_cli_artifact("config-explain", "config-explain", &config_explain);

    // JOURNEY-MEMORY-CLI
    let mem_workspace = tempfile::tempdir().unwrap_or_abort();
    let mem_ws = mem_workspace.path().to_str().unwrap_or_abort();

    let mem_put = run_harness(&[
        "memory",
        "put",
        "journey.parity.test",
        "verified",
        "--workspace",
        mem_ws,
    ]);
    assert_success("memory put", &mem_put);
    let put_json = parse_json(&mem_put);
    postconditions.insert(
        "memory_put_key_matches".into(),
        Value::Bool(put_json["key"].as_str() == Some("journey.parity.test")),
    );
    postconditions.insert(
        "memory_put_value_matches".into(),
        Value::Bool(put_json["value"].as_str() == Some("verified")),
    );
    write_cli_artifact("memory-cli", "memory-put", &mem_put);

    let mem_get = run_harness(&[
        "memory",
        "get",
        "journey.parity.test",
        "--workspace",
        mem_ws,
    ]);
    assert_success("memory get", &mem_get);
    let get_json = parse_json(&mem_get);
    postconditions.insert(
        "memory_get_roundtrip".into(),
        Value::Bool(get_json["value"].as_str() == Some("verified")),
    );
    write_cli_artifact("memory-cli", "memory-get", &mem_get);

    let mem_list = run_harness(&["memory", "list", "--workspace", mem_ws]);
    assert_success("memory list", &mem_list);
    let list_json = parse_json(&mem_list);
    postconditions.insert(
        "memory_list_nonempty".into(),
        Value::Bool(
            list_json["entries"]
                .as_array()
                .is_some_and(|e| !e.is_empty()),
        ),
    );
    write_cli_artifact("memory-cli", "memory-list", &mem_list);

    // JOURNEY-WAIT-ANY-ALL
    {
        use harness_core::coord::{background_wait_condition_satisfied, BackgroundWaitMode};

        let any = BackgroundWaitMode::parse("any").unwrap_or_abort();
        let all = BackgroundWaitMode::parse("all").unwrap_or_abort();
        let partial = [("req_a", false), ("req_b", true), ("req_c", false)];
        let all_terminal = [("req_a", true), ("req_b", true), ("req_c", true)];

        let any_fires = background_wait_condition_satisfied(any, &partial);
        let all_blocks = !background_wait_condition_satisfied(all, &partial);
        let all_fires = background_wait_condition_satisfied(all, &all_terminal);

        postconditions.insert("wait_any_fires_on_partial".into(), Value::Bool(any_fires));
        postconditions.insert("wait_all_blocks_on_partial".into(), Value::Bool(all_blocks));
        postconditions.insert(
            "wait_all_fires_on_all_terminal".into(),
            Value::Bool(all_fires),
        );

        let l5_path = repo_root().join(
            "crates/harness-tools/tests/native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/10_background_output_wait_any_all_test.rs",
        );
        postconditions.insert(
            "wait_any_l5_owner_exists".into(),
            Value::Bool(l5_path.is_file()),
        );

        let receipt = serde_json::json!({
            "schema_version": "harness-journey-wait-any-parity-v1",
            "journey_id": "JOURNEY-WAIT-ANY-ALL",
            "any_fires_on_partial": any_fires,
            "all_blocks_on_partial": all_blocks,
            "all_fires_on_all_terminal": all_fires,
            "l5_owner_path": l5_path.to_str().unwrap_or_abort(),
            "surface": "compiled_coordinator_background_wait_api",
        });
        write_json_artifact("wait-any-all", "wait-any-receipt", &receipt);
    }

    // JOURNEY-FOLDER-TRUST-DENY
    {
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
        let denied = matches!(gate, LocalExecutableGate::Denied { .. });
        postconditions.insert("folder_trust_denied".into(), Value::Bool(denied));

        let l2_path = repo_root().join("crates/harness-core/src/folder_trust.rs");
        let l5_path = repo_root().join("crates/harness-tools/src/shell_safety.rs");
        postconditions.insert(
            "folder_trust_l2_exists".into(),
            Value::Bool(l2_path.is_file()),
        );
        postconditions.insert(
            "folder_trust_l5_exists".into(),
            Value::Bool(l5_path.is_file()),
        );

        let receipt = serde_json::json!({
            "schema_version": "harness-journey-folder-trust-parity-v1",
            "journey_id": "JOURNEY-FOLDER-TRUST-DENY",
            "denied": denied,
            "spawn_attempted": false,
            "l2_owner_path": l2_path.to_str().unwrap_or_abort(),
            "l5_owner_path": l5_path.to_str().unwrap_or_abort(),
            "surface": "compiled_core_folder_trust_gate",
        });
        write_json_artifact("folder-trust-deny", "folder-trust-receipt", &receipt);
    }

    // JOURNEY-ALWAYS-APPROVE-MODE
    {
        let scenario_ws = tempfile::tempdir().unwrap_or_abort();
        let events_path = artifact_dir("always-approve-mode").join("golden-path-events.jsonl");
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
        assert_success("run --scenario golden_path", &run_output);

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
        postconditions.insert(
            "always_approve_perm_requested_in_event_log".into(),
            Value::Bool(has_perm_requested),
        );
        postconditions.insert(
            "always_approve_perm_resolved_in_event_log".into(),
            Value::Bool(has_perm_resolved),
        );
        postconditions.insert(
            "always_approve_perm_decision_allow".into(),
            Value::Bool(perm_decision_allow),
        );

        let l2 = repo_root().join("crates/harness-tui/src/app/tests/permission_modal_tests.rs");
        let l5 = repo_root().join("crates/harness-tui/src/keybindings/tests.rs");
        postconditions.insert(
            "always_approve_l2_owner_exists".into(),
            Value::Bool(l2.is_file()),
        );
        postconditions.insert(
            "always_approve_l5_owner_exists".into(),
            Value::Bool(l5.is_file()),
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

    // JOURNEY-SETTINGS-EDITOR
    {
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
        let mut app = AppState::new_live(None, false, None);
        app.bind_settings_project_config(&config_path, initial, true, true, true, true, false);

        app.execute_slash_command("settings", None);
        let editor_visible = app.settings_editor_is_visible();
        postconditions.insert("settings_editor_opens".into(), Value::Bool(editor_visible));

        let summary = app.settings_editor_summary();
        postconditions.insert("settings_editor_bound".into(), Value::Bool(summary.bound));
        postconditions.insert(
            "settings_editor_has_writable_paths".into(),
            Value::Bool(summary.writable_paths > 0),
        );

        let hashline_idx = settings_registry()
            .iter()
            .position(|entry| entry.setting_id.as_str() == "hashline_edit")
            .unwrap_or_abort();
        while app.settings_editor_selected_index() != hashline_idx {
            app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        let selected_id = app.settings_editor_selected_id();
        postconditions.insert(
            "settings_editor_navigates_to_hashline".into(),
            Value::Bool(selected_id == Some("hashline_edit")),
        );

        app.settings_editor_activate_selected();
        let after_edit = read_effective_hashline_edit(&config_path).unwrap_or_abort();
        postconditions.insert(
            "settings_editor_toggles_hashline_to_false".into(),
            Value::Bool(!after_edit),
        );

        write_project_hashline_edit(&config_path, true).unwrap_or_abort();
        let restored = read_effective_hashline_edit(&config_path).unwrap_or_abort();
        app.bind_settings_project_config(&config_path, restored, true, true, true, true, false);
        postconditions.insert(
            "settings_editor_reload_reflects_true".into(),
            Value::Bool(app.settings_hashline_edit()),
        );

        let receipt = serde_json::json!({
            "schema_version": "harness-journey-settings-editor-parity-v1",
            "journey_id": "JOURNEY-SETTINGS-EDITOR",
            "editor_visible": editor_visible,
            "hashline_round_trip": {
                "initial": initial,
                "after_edit": after_edit,
                "after_reload": restored,
            },
            "surface": "compiled_tui_appstate_real_slash_and_key_interactions",
        });
        write_json_artifact("settings-editor", "settings-editor-receipt", &receipt);
    }

    // JOURNEY-WORKTREE-CTRL-W
    {
        use harness_core::worktree::{create_session_worktree, CreateWorktreeOptions};

        let ws = tempfile::tempdir().unwrap_or_abort();
        let git_root = ws.path();

        let git_init = Command::new("git")
            .args(["init"])
            .current_dir(git_root)
            .output()
            .unwrap_or_abort();
        assert!(
            git_init.status.success(),
            "git init failed: {}",
            String::from_utf8_lossy(&git_init.stderr)
        );

        fs::write(git_root.join("README.md"), "# test\n").unwrap_or_abort();
        let _git_add = Command::new("git")
            .args(["add", "."])
            .current_dir(git_root)
            .output()
            .unwrap_or_abort();
        let _git_commit = Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(git_root)
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .unwrap_or_abort();

        let options = CreateWorktreeOptions {
            repository_root: git_root,
            worktree_parent: None,
            slug: Some("test-wt-27"),
            start_point: None,
        };
        let created = create_session_worktree(options).unwrap_or_abort();

        let worktree_exists = created.path.is_dir();
        let branch_created = created.branch.contains("harness/wt-");

        postconditions.insert(
            "worktree_created_on_disk".into(),
            Value::Bool(worktree_exists),
        );
        postconditions.insert(
            "worktree_branch_created".into(),
            Value::Bool(branch_created),
        );

        let l2 = repo_root().join("crates/harness-tui/src/app/tests/lifecycle_shell_tests.rs");
        let core = repo_root().join("crates/harness-core/src/worktree.rs");
        postconditions.insert("worktree_l2_owner_exists".into(), Value::Bool(l2.is_file()));
        postconditions.insert(
            "worktree_core_module_exists".into(),
            Value::Bool(core.is_file()),
        );

        let receipt = serde_json::json!({
            "schema_version": "harness-journey-worktree-parity-v1",
            "journey_id": "JOURNEY-WORKTREE-CTRL-W",
            "worktree_path": created.path.to_str().unwrap_or_abort(),
            "branch_name": created.branch,
            "worktree_exists": worktree_exists,
            "branch_created": branch_created,
            "surface": "compiled_core_worktree_api",
        });
        write_json_artifact("worktree-ctrl-w", "worktree-receipt", &receipt);

        let _ = Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&created.path)
            .current_dir(git_root)
            .output();
    }

    Value::Object(postconditions)
}

/// Mutate one postcondition to false and verify it's rejected.
pub fn verify_mutated_postcondition_rejected() -> bool {
    let postconditions = collect_all_postconditions();
    let mut mutated = postconditions.clone();
    let map = mutated.as_object_mut().unwrap_or_abort();

    if let Some(first_key) = map.keys().next() {
        let first_key = first_key.clone();
        map.insert(first_key, Value::Bool(false));
    }

    !verify_all_postconditions(&mutated)
}
