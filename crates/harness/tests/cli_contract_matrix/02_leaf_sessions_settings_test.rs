// ---------------------------------------------------------------------------
// CLI leaf paths via CliIo/CliDeps — run, prompt, headless
// ---------------------------------------------------------------------------

#[test]
fn run_help_exits_zero_and_lists_mock_flag() {
    // arrange
    // act
    let (code, stdout, _stderr) = run_cli_in_workspace(&["run", "--help"]);

    // assert
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--mock"),
        "run help must advertise --mock flag for headless deterministic use"
    );
    assert!(stdout.contains("--model"));
    assert!(stdout.contains("--out"));
}

#[test]
fn prompt_help_exits_zero_and_lists_text_flag() {
    // arrange
    // act
    let (code, stdout, _stderr) = run_cli_in_workspace(&["prompt", "--help"]);

    // assert
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--text"),
        "prompt help must advertise --text flag"
    );
    assert!(stdout.contains("--out"));
}

#[test]
fn run_deterministic_scenario_produces_events_file() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let config = r#"{
        "provider": {
            "mock": {
                "type": "openai_compatible",
                "baseURL": "http://127.0.0.1:1/v1",
                "apiKey": "DUMMY",
                "apiMode": "responses",
                "models": { "test-model": { "name": "Test" } }
            }
        },
        "model": "mock/test-model",
        "agent": { "default": { "model": "mock/test-model" } },
        "permission": "allow"
    }"#;
    let config_path = write_config(temp.path(), config);
    let out_path = temp.path().join("events.jsonl");
    let session_dir = temp.path().join("sessions");
    let deps = CliDeps::real()
        .with_current_dir(temp.path().to_path_buf())
        .with_env("HOME", temp.path().to_string_lossy())
        .with_env(
            "XDG_CONFIG_HOME",
            temp.path().join("config").to_string_lossy(),
        );

    // act
    let (code, _stdout, stderr) = run_cli(
        &[
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "run",
            "--scenario",
            "golden_path",
            "--deterministic",
            "--session-dir",
            session_dir.to_str().unwrap_or_abort(),
            "--out",
            out_path.to_str().unwrap_or_abort(),
        ],
        deps,
    );

    // assert
    assert_eq!(
        code, 0,
        "deterministic scenario run must exit 0; stderr: {stderr}"
    );
    assert!(
        out_path.exists(),
        "run --deterministic must write an events file"
    );
}

// ---------------------------------------------------------------------------
// Sessions leaf path — list via CliIo
// ---------------------------------------------------------------------------

#[test]
fn sessions_list_json_in_temp_session_dir_returns_valid_json() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let config = r#"{
        "provider": {
            "default": {
                "type": "openai_compatible",
                "baseURL": "http://127.0.0.1:1/v1",
                "apiKey": "DUMMY",
                "models": { "test-model": { "name": "Test" } }
            }
        },
        "model": "default/test-model",
        "agent": { "default": { "model": "default/test-model" } },
        "permission": "allow"
    }"#;
    let config_path = write_config(temp.path(), config);
    let session_dir = temp.path().join("sessions");
    std::fs::create_dir_all(&session_dir).unwrap_or_abort();
    let run_dir = session_dir.join("run_t23_list");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    std::fs::write(
        run_dir.join("events.jsonl"),
        concat!(
            r#"{"schema_version":1,"event_id":"evt-0001","seq":1,"run_id":"run_t23_list","mono_ms":1,"ts":null,"actor":{"kind":"system","label":"test"},"correlation_id":null,"causation_id":null,"stream_key":"run:run_t23_list","payload":{"type":"run_started","run_name":"t23","workspace_root":"/tmp"}}"#,
            "\n",
            r#"{"schema_version":1,"event_id":"evt-0002","seq":2,"run_id":"run_t23_list","mono_ms":2,"ts":null,"actor":{"kind":"system","label":"test"},"correlation_id":null,"causation_id":null,"stream_key":"run:run_t23_list","payload":{"type":"run_finished","summary":"done"}}"#,
            "\n",
        ),
    )
    .unwrap_or_abort();
    let deps = CliDeps::real()
        .with_current_dir(temp.path().to_path_buf())
        .with_env("HOME", temp.path().to_string_lossy())
        .with_env(
            "XDG_CONFIG_HOME",
            temp.path().join("config").to_string_lossy(),
        );

    // act
    let (code, stdout, stderr) = run_cli(
        &[
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "--session-dir",
            session_dir.to_str().unwrap_or_abort(),
            "sessions",
            "list",
            "--json",
        ],
        deps,
    );

    // assert
    assert_eq!(code, 0, "sessions list must exit 0; stderr: {stderr}");
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_abort();
    let entries = parsed
        .as_array()
        .or_else(|| parsed["entries"].as_array())
        .unwrap_or_abort();
    assert!(
        entries.iter().any(|e| e["run_id"] == "run_t23_list"),
        "sessions list --json must contain the seeded run"
    );
}

// ---------------------------------------------------------------------------
// Doctor — no network/provider execution contract
// ---------------------------------------------------------------------------

#[test]
fn doctor_json_reports_no_network_probes() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let config = r#"{
        "provider": {
            "default": {
                "type": "openai_compatible",
                "baseURL": "http://127.0.0.1:1/v1",
                "apiKey": "DUMMY",
                "models": { "test-model": { "name": "Test" } }
            }
        },
        "model": "default/test-model",
        "agent": { "default": { "model": "default/test-model" } },
        "permission": "allow"
    }"#;
    let config_path = write_config(temp.path(), config);
    let deps = CliDeps::real()
        .with_current_dir(temp.path().to_path_buf())
        .with_env("HOME", temp.path().to_string_lossy())
        .with_env(
            "XDG_CONFIG_HOME",
            temp.path().join("config").to_string_lossy(),
        );

    // act
    let (_code, stdout, _stderr) = run_cli(
        &[
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "doctor",
            "--json",
        ],
        deps,
    );

    // assert
    let trimmed = stdout.trim();
    assert!(!trimmed.is_empty(), "doctor --json must produce output");
    let parsed: serde_json::Value = serde_json::from_str(trimmed).unwrap_or_abort();
    assert_eq!(
        parsed["no_network_probes"], true,
        "doctor must perform no network/provider execution"
    );
}

// ---------------------------------------------------------------------------
// Config settings — typed registry metadata contract
// ---------------------------------------------------------------------------

#[test]
fn config_settings_json_includes_typed_surface_and_sensitivity() {
    // arrange
    // act
    let (code, stdout, stderr) = run_cli_in_workspace(&["config", "settings"]);

    // assert
    assert_eq!(code, 0, "stderr: {stderr}");
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_abort();
    let settings = parsed["settings"].as_array().unwrap_or_abort();
    assert!(!settings.is_empty(), "settings registry must be non-empty");

    // Every entry must have surface, sensitivity, and mutability metadata
    for entry in settings {
        assert!(
            entry["surface"].is_string(),
            "each setting must declare a surface (runtime|tui)"
        );
        assert!(
            entry["sensitivity"].is_string(),
            "each setting must declare a sensitivity"
        );
        assert!(
            entry["mutability"].is_string(),
            "each setting must declare mutability"
        );
    }

    // provider.apiKey must be marked secret
    let api_key_setting = settings
        .iter()
        .find(|s| s["setting_id"] == "provider.apiKey")
        .unwrap_or_abort();
    assert_eq!(api_key_setting["sensitivity"], "secret");
    assert_eq!(api_key_setting["surface"], "runtime");
}

// ---------------------------------------------------------------------------
// Config explain — deterministic attribution for nested keys
// ---------------------------------------------------------------------------

#[test]
fn config_explain_nested_provider_path_attributes_to_explicit_config() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let config = r#"{
        "provider": {
            "myprov": {
                "type": "openai_compatible",
                "baseURL": "http://127.0.0.1:9/v1",
                "apiKey": "DUMMY",
                "models": { "m": { "name": "M" } }
            }
        },
        "model": "myprov/m",
        "agent": { "default": { "model": "myprov/m" } },
        "permission": "allow"
    }"#;
    let config_path = write_config(temp.path(), config);
    let deps = CliDeps::real()
        .with_current_dir(temp.path().to_path_buf())
        .with_env("HOME", temp.path().to_string_lossy())
        .with_env(
            "XDG_CONFIG_HOME",
            temp.path().join("config").to_string_lossy(),
        );

    // act
    let (code, stdout, stderr) = run_cli(
        &[
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "config",
            "explain",
            "provider.myprov.type",
        ],
        deps,
    );

    // assert
    assert_eq!(code, 0, "stderr: {stderr}");
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_abort();
    assert!(parsed.is_object());
    assert_eq!(parsed["source_value"], "openai_compatible");
    assert!(
        parsed["source_path"]
            .as_str()
            .is_some_and(|s| s.contains("harness.jsonc")),
        "attribution must point to the explicit config path"
    );
}

// ---------------------------------------------------------------------------
// Config show --effective — apiKey redaction with multiple providers
// ---------------------------------------------------------------------------

#[test]
fn config_show_effective_redacts_all_provider_api_keys() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let config = r#"{
        "provider": {
            "prov_a": {
                "type": "openai_compatible",
                "baseURL": "http://127.0.0.1:1/v1",
                "apiKey": "sk-secret-prov-a-123456",
                "models": { "m": { "name": "M" } }
            },
            "prov_b": {
                "type": "openai_compatible",
                "baseURL": "http://127.0.0.1:2/v1",
                "apiKey": "sk-secret-prov-b-789012",
                "models": { "n": { "name": "N" } }
            }
        },
        "model": "prov_a/m",
        "agent": { "default": { "model": "prov_a/m" } },
        "permission": "allow"
    }"#;
    let config_path = write_config(temp.path(), config);
    let deps = CliDeps::real()
        .with_current_dir(temp.path().to_path_buf())
        .with_env("HOME", temp.path().to_string_lossy())
        .with_env(
            "XDG_CONFIG_HOME",
            temp.path().join("config").to_string_lossy(),
        );

    // act
    let (code, stdout, stderr) = run_cli(
        &[
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "config",
            "show",
            "--effective",
        ],
        deps,
    );

    // assert
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(
        !stdout.contains("sk-secret-prov-a-123456"),
        "must redact prov_a key"
    );
    assert!(
        !stdout.contains("sk-secret-prov-b-789012"),
        "must redact prov_b key"
    );
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_abort();
    assert!(parsed.is_object());
}

// ---------------------------------------------------------------------------
// Worktree subcommand — help surface
// ---------------------------------------------------------------------------

#[test]
fn worktree_help_exits_zero_and_lists_subcommands() {
    // arrange
    // act
    let (code, stdout, _stderr) = run_cli_in_workspace(&["worktree", "--help"]);

    // assert
    assert_eq!(code, 0);
    assert!(
        stdout.contains("list") || stdout.contains("List"),
        "worktree help must list the list subcommand"
    );
}

// ---------------------------------------------------------------------------
// Memory subcommand — help surface
// ---------------------------------------------------------------------------

#[test]
fn memory_help_exits_zero() {
    // arrange
    // act
    let (code, stdout, _stderr) = run_cli_in_workspace(&["memory", "--help"]);

    // assert
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--help") || stdout.contains("Commands:"),
        "memory must produce help output"
    );
}

// ---------------------------------------------------------------------------
// Prompt-queue subcommand — help surface
// ---------------------------------------------------------------------------

#[test]
fn prompt_queue_help_exits_zero() {
    // arrange
    // act
    let (code, stdout, _stderr) = run_cli_in_workspace(&["prompt-queue", "--help"]);

    // assert
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--help") || stdout.contains("Commands:"),
        "prompt-queue must produce help output"
    );
}

// ---------------------------------------------------------------------------
// Update subcommand — help surface
// ---------------------------------------------------------------------------

#[test]
fn update_help_exits_zero() {
    // arrange
    // act
    let (code, stdout, _stderr) = run_cli_in_workspace(&["update", "--help"]);

    // assert
    assert_eq!(code, 0);
    assert!(
        stdout.contains("--help") || stdout.contains("Commands:"),
        "update must produce help output"
    );
}

// ---------------------------------------------------------------------------
// Config validate — TUI config accepted alongside runtime config
// ---------------------------------------------------------------------------

#[test]
fn config_validate_with_separate_tui_config_succeeds() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let config = r#"{
        "provider": {
            "default": {
                "type": "openai_compatible",
                "baseURL": "http://127.0.0.1:1/v1",
                "apiKey": "DUMMY",
                "models": { "test-model": { "name": "Test" } }
            }
        },
        "model": "default/test-model",
        "agent": { "default": { "model": "default/test-model" } },
        "permission": "allow"
    }"#;
    let config_path = write_config(temp.path(), config);
    std::fs::write(temp.path().join("tui.jsonc"), r#"{ "keybinds": {} }"#).unwrap_or_abort();
    let deps = CliDeps::real()
        .with_current_dir(temp.path().to_path_buf())
        .with_env("HOME", temp.path().to_string_lossy())
        .with_env(
            "XDG_CONFIG_HOME",
            temp.path().join("config").to_string_lossy(),
        );

    // act
    let (code, stdout, stderr) = run_cli(
        &[
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "config",
            "validate",
        ],
        deps,
    );

    // assert
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("config valid:"), "stdout: {stdout}");
}

// ---------------------------------------------------------------------------
// Settings write-back — generic write_project_setting_bool roundtrip
// ---------------------------------------------------------------------------

#[test]
fn settings_write_project_setting_bool_roundtrips_hashline_edit() {
    // arrange
    use harness_core::config::{read_effective_hashline_edit, write_project_setting_bool};

    let temp = tempfile::tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.jsonc");
    fs::write(
        &config_path,
        r#"{
            "provider": {
                "default": {
                    "type": "openai_compatible",
                    "baseURL": "http://127.0.0.1:1/v1",
                    "apiKey": "DUMMY",
                    "models": { "test-model": { "name": "Test" } }
                }
            },
            "model": "default/test-model",
            "agent": { "default": { "model": "default/test-model" } },
            "permission": "allow"
        }"#,
    )
    .unwrap_or_abort();

    // act — write hashline_edit=false
    write_project_setting_bool(&config_path, "hashline_edit", false).unwrap_or_abort();

    // assert
    let value = read_effective_hashline_edit(&config_path).unwrap_or_abort();
    assert!(!value, "hashline_edit must be false after write");
}

// ---------------------------------------------------------------------------
// Settings registry — typed metadata API contract
// ---------------------------------------------------------------------------

#[test]
fn settings_registry_has_separate_runtime_and_tui_surfaces() {
    // arrange
    use harness_core::config::summarize_settings_registry;

    // act
    let summary = summarize_settings_registry();

    // assert
    assert!(
        summary.runtime > 0,
        "registry must contain runtime settings"
    );
    assert!(summary.tui > 0, "registry must contain TUI settings");
    assert_eq!(
        summary.total,
        summary.runtime + summary.tui,
        "every setting belongs to exactly one surface"
    );
    assert!(summary.secret >= 1, "provider.apiKey must be secret");
    assert!(summary.with_default > 0, "some defaults must exist");
}

#[test]
fn settings_registry_resolves_legacy_migrations() {
    // arrange
    use harness_core::config::{resolve_setting_id, settings_compat_migrations};

    // act
    let migrations = settings_compat_migrations();

    // assert
    assert!(
        !migrations.is_empty(),
        "settings registry must track compat migrations"
    );
    for migration in migrations {
        let resolved = resolve_setting_id(migration.legacy_id);
        assert_eq!(
            resolved,
            Some(migration.canonical_id),
            "legacy id {} must resolve to {}",
            migration.legacy_id,
            migration.canonical_id
        );
    }
}

// ---------------------------------------------------------------------------
// Schema generation — both surfaces produce valid JSON Schema
// ---------------------------------------------------------------------------

#[test]
fn schema_generation_runtime_and_tui_are_distinct_contracts() {
    // arrange
    use harness_core::config::{harness_schema_pretty_json, harness_tui_schema_pretty_json};

    // act
    let runtime_json = harness_schema_pretty_json().unwrap_or_abort();
    let tui_json = harness_tui_schema_pretty_json().unwrap_or_abort();

    // assert
    let runtime: serde_json::Value = serde_json::from_str(&runtime_json).unwrap_or_abort();
    let tui: serde_json::Value = serde_json::from_str(&tui_json).unwrap_or_abort();

    // Runtime schema must have provider/model/permission top-level keys
    let runtime_props = runtime["properties"].as_object().unwrap_or_abort();
    assert!(runtime_props.contains_key("provider"));
    assert!(runtime_props.contains_key("model"));
    assert!(runtime_props.contains_key("permission"));

    // TUI schema must NOT have provider/model/permission keys (separate contract)
    let tui_props = tui["properties"].as_object().unwrap_or_abort();
    assert!(
        !tui_props.contains_key("provider"),
        "TUI schema must not own runtime provider keys"
    );
    assert!(
        !tui_props.contains_key("permission"),
        "TUI schema must not own runtime permission keys"
    );
}
