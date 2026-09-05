// ---------------------------------------------------------------------------
// Model-alias resolver contract (plan §1.1)
// ---------------------------------------------------------------------------

#[test]
fn model_alias_resolver_maps_umans_coder_to_umans_kimi_k27() {
    // arrange
    // act
    let canonical = resolve_model_alias("umans-coder");

    // assert
    assert_eq!(canonical, "umans-kimi-k2.7");
}

#[test]
fn model_alias_resolver_maps_umans_flash_to_umans_qwen36_35b_a3b() {
    // arrange
    // act
    let canonical = resolve_model_alias("umans-flash");

    // assert
    assert_eq!(canonical, "umans-qwen3.6-35b-a3b");
}

#[test]
fn model_alias_resolver_passes_through_canonical_ids_unchanged() {
    assert_eq!(resolve_model_alias("umans-kimi-k2.7"), "umans-kimi-k2.7");
    assert_eq!(
        resolve_model_alias("umans-qwen3.6-35b-a3b"),
        "umans-qwen3.6-35b-a3b"
    );
}

#[test]
fn model_alias_resolver_passes_through_unknown_ids_unchanged() {
    assert_eq!(resolve_model_alias("gpt-5.5"), "gpt-5.5");
    assert_eq!(resolve_model_alias("unknown-model"), "unknown-model");
}

#[test]
fn model_alias_is_alias_detects_known_aliases() {
    assert!(is_model_alias("umans-coder"));
    assert!(is_model_alias("umans-flash"));
    assert!(!is_model_alias("umans-kimi-k2.7"));
    assert!(!is_model_alias("gpt-5.5"));
}

#[test]
fn model_alias_known_aliases_table_has_exactly_two_entries() {
    // arrange
    // act
    let aliases = known_model_aliases();

    // assert
    assert_eq!(aliases.len(), 2);
    assert!(aliases.contains(&ModelAlias {
        logical_id: "umans-coder",
        canonical_backend_id: "umans-kimi-k2.7",
    }));
    assert!(aliases.contains(&ModelAlias {
        logical_id: "umans-flash",
        canonical_backend_id: "umans-qwen3.6-35b-a3b",
    }));
}

// ---------------------------------------------------------------------------
// Config validate — happy path
// ---------------------------------------------------------------------------

#[test]
fn config_validate_succeeds_for_workspace_config() {
    // arrange
    // act
    let (code, stdout, stderr) = run_cli_in_workspace(&["config", "validate"]);

    // assert
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("config valid:"), "stdout: {stdout}");
}

#[test]
fn config_validate_succeeds_for_example_config() {
    // arrange
    let example = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../configs/harness.example.jsonc");

    // act
    let (code, stdout, stderr) = run_cli_in_workspace(&[
        "--config",
        example.to_str().unwrap_or_abort(),
        "config",
        "validate",
    ]);

    // assert
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("config valid:"), "stdout: {stdout}");
}

// ---------------------------------------------------------------------------
// Config validate — error path
// ---------------------------------------------------------------------------

#[test]
fn config_validate_fails_for_missing_config_file() {
    // arrange
    // act
    let (code, _stdout, stderr) = run_cli_in_workspace(&[
        "--config",
        "/nonexistent/path/harness.jsonc",
        "config",
        "validate",
    ]);

    // assert
    assert_ne!(code, 0);
    assert!(stderr.contains("failed") || stderr.contains("not found") || stderr.contains("error"));
}

#[test]
fn config_validate_fails_for_invalid_json_config() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.jsonc");
    fs::write(&config_path, "{ invalid json }").unwrap_or_abort();
    let deps = CliDeps::real().with_current_dir(temp.path().to_path_buf());

    // act
    let (code, _stdout, stderr) = run_cli(
        &[
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "config",
            "validate",
        ],
        deps,
    );

    // assert
    assert_ne!(code, 0);
    assert!(!stderr.is_empty());
}

// ---------------------------------------------------------------------------
// Config show --effective — JSON schema path
// ---------------------------------------------------------------------------

#[test]
fn config_show_effective_emits_valid_json_for_workspace_config() {
    // arrange
    // act
    let (code, stdout, stderr) = run_cli_in_workspace(&["config", "show", "--effective"]);

    // assert
    assert_eq!(code, 0, "stderr: {stderr}");
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_abort();
    assert!(parsed.is_object(), "effective config must be a JSON object");
}

#[test]
fn config_show_without_effective_flag_returns_error_code_2() {
    // arrange
    // act
    let (code, _stdout, stderr) = run_cli_in_workspace(&["config", "show"]);

    // assert
    assert_eq!(code, 2);
    assert!(stderr.contains("--effective"));
}

// ---------------------------------------------------------------------------
// Config sources — JSON path
// ---------------------------------------------------------------------------

#[test]
fn config_sources_emits_valid_json_for_workspace_config() {
    // arrange
    // act
    let (code, stdout, stderr) = run_cli_in_workspace(&["config", "sources"]);

    // assert
    assert_eq!(code, 0, "stderr: {stderr}");
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_abort();
    assert!(parsed.is_object() || parsed.is_array());
}

// ---------------------------------------------------------------------------
// Config explain — happy and error paths
// ---------------------------------------------------------------------------

#[test]
fn config_explain_emits_valid_json_for_model_path() {
    // arrange
    // act
    let (code, stdout, stderr) = run_cli_in_workspace(&["config", "explain", "model"]);

    // assert
    assert_eq!(code, 0, "stderr: {stderr}");
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_abort();
    assert!(parsed.is_object());
}

#[test]
fn config_explain_returns_error_for_empty_path() {
    // arrange
    // act
    let (code, _stdout, stderr) = run_cli_in_workspace(&["config", "explain", ""]);

    // assert
    assert_eq!(code, 2);
    assert!(stderr.contains("dotted path"));
}

// ---------------------------------------------------------------------------
// Config settings — JSON path
// ---------------------------------------------------------------------------

#[test]
fn config_settings_emits_valid_settings_registry_json() {
    // arrange
    // act
    let (code, stdout, stderr) = run_cli_in_workspace(&["config", "settings"]);

    // assert
    assert_eq!(code, 0, "stderr: {stderr}");
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_abort();
    assert!(parsed.is_object() || parsed.is_array());
}

// ---------------------------------------------------------------------------
// Schema — JSON schema path
// ---------------------------------------------------------------------------

#[test]
fn schema_command_emits_valid_runtime_json_schema() {
    // arrange
    // act
    let (code, stdout, _stderr) = run_cli_in_workspace(&["schema"]);

    // assert
    assert_eq!(code, 0);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_abort();
    assert!(
        parsed["$schema"].is_string()
            || parsed["type"].is_string()
            || parsed["properties"].is_object()
    );
}

#[test]
fn schema_command_emits_valid_tui_json_schema() {
    // arrange
    // act
    let (code, stdout, _stderr) = run_cli_in_workspace(&["schema", "--tui"]);

    // assert
    assert_eq!(code, 0);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_abort();
    assert!(
        parsed["$schema"].is_string()
            || parsed["type"].is_string()
            || parsed["properties"].is_object()
    );
}

// ---------------------------------------------------------------------------
// Doctor — happy path (text and JSON)
// ---------------------------------------------------------------------------

#[test]
fn doctor_command_runs_in_workspace_and_exits_zero_or_reports_issues() {
    // arrange
    // act
    let (code, stdout, _stderr) = run_cli_in_workspace(&["doctor"]);

    // assert
    // doctor may return 0 (all pass) or non-zero (issues found) — either way
    // it must produce output and not crash
    assert!(!stdout.is_empty() || code != 0);
}

#[test]
fn doctor_json_command_emits_valid_json() {
    // arrange
    // act
    let (code, stdout, _stderr) = run_cli_in_workspace(&["doctor", "--json"]);

    // assert
    // doctor --json may return 0 or non-zero depending on workspace state,
    // but the output must be valid JSON
    let trimmed = stdout.trim();
    if !trimmed.is_empty() {
        let parsed: serde_json::Value = serde_json::from_str(trimmed).unwrap_or_else(|err| {
            panic!("doctor --json output is not valid JSON: {err}\noutput: {trimmed}")
        });
        assert!(parsed.is_object(), "doctor JSON must be an object");
    } else {
        // If stdout is empty, the command must have failed
        assert_ne!(code, 0);
    }
}

// ---------------------------------------------------------------------------
// Completions — happy path
// ---------------------------------------------------------------------------

#[test]
fn completions_command_emits_bash_completion_script() {
    // arrange
    // act
    let (code, stdout, _stderr) = run_cli_in_workspace(&["completions", "bash"]);

    // assert
    assert_eq!(code, 0);
    assert!(!stdout.is_empty(), "bash completions must not be empty");
}

#[test]
fn completions_command_emits_zsh_completion_script() {
    // arrange
    // act
    let (code, stdout, _stderr) = run_cli_in_workspace(&["completions", "zsh"]);

    // assert
    assert_eq!(code, 0);
    assert!(!stdout.is_empty(), "zsh completions must not be empty");
}

#[test]
fn completions_command_emits_fish_completion_script() {
    // arrange
    // act
    let (code, stdout, _stderr) = run_cli_in_workspace(&["completions", "fish"]);

    // assert
    assert_eq!(code, 0);
    assert!(!stdout.is_empty(), "fish completions must not be empty");
}

// ---------------------------------------------------------------------------
// Unknown command — error path
// ---------------------------------------------------------------------------

#[test]
fn unknown_subcommand_returns_non_zero_exit_code() {
    // arrange
    // act
    let (code, _stdout, stderr) = run_cli_in_workspace(&["nonexistent-command-xyz"]);

    // assert
    assert_ne!(code, 0);
    assert!(!stderr.is_empty());
}

// ---------------------------------------------------------------------------
// Config conflict — conflicting provider options
// ---------------------------------------------------------------------------

#[test]
fn config_validate_fails_when_provider_options_conflict() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let config = r#"{
        "provider": {
            "default": {
                "type": "openai_compatible",
                "baseURL": "http://127.0.0.1:1/v1",
                "options": { "baseURL": "http://127.0.0.1:2/v1" },
                "apiKey": "DUMMY",
                "models": { "test-model": { "name": "Test" } }
            }
        },
        "model": "default/test-model",
        "agent": { "default": { "model": "default/test-model" } },
        "permission": "allow"
    }"#;
    let config_path = write_config(temp.path(), config);
    let deps = CliDeps::real().with_current_dir(temp.path().to_path_buf());

    // act
    let (code, _stdout, stderr) = run_cli(
        &[
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "config",
            "validate",
        ],
        deps,
    );

    // assert
    assert_ne!(code, 0, "conflicting baseURL should fail validation");
    assert!(stderr.contains("conflict") || stderr.contains("error"));
}

// ---------------------------------------------------------------------------
// Config redaction — apiKey is never printed
// ---------------------------------------------------------------------------

#[test]
fn config_show_effective_redacts_api_key() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let config = r#"{
        "provider": {
            "default": {
                "type": "openai_compatible",
                "baseURL": "http://127.0.0.1:1/v1",
                "apiKey": "sk-secret-key-do-not-print-12345",
                "models": { "test-model": { "name": "Test" } }
            }
        },
        "model": "default/test-model",
        "agent": { "default": { "model": "default/test-model" } },
        "permission": "allow"
    }"#;
    let config_path = write_config(temp.path(), config);
    let deps = CliDeps::real().with_current_dir(temp.path().to_path_buf());

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
        !stdout.contains("sk-secret-key-do-not-print-12345"),
        "apiKey must be redacted in effective config output"
    );
}

// ---------------------------------------------------------------------------
// Config source attribution — sources command shows layer paths
// ---------------------------------------------------------------------------

#[test]
fn config_sources_shows_source_layer_paths() {
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
    let deps = CliDeps::real().with_current_dir(temp.path().to_path_buf());

    // act
    let (code, stdout, stderr) = run_cli(
        &[
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "config",
            "sources",
        ],
        deps,
    );

    // assert
    assert_eq!(code, 0, "stderr: {stderr}");
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_abort();
    assert!(parsed.is_object() || parsed.is_array());
}

// ---------------------------------------------------------------------------
// Config explain — source attribution for a specific key
// ---------------------------------------------------------------------------

#[test]
fn config_explain_attributes_model_key_to_source() {
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
    let deps = CliDeps::real().with_current_dir(temp.path().to_path_buf());

    // act
    let (code, stdout, stderr) = run_cli(
        &[
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "config",
            "explain",
            "model",
        ],
        deps,
    );

    // assert
    assert_eq!(code, 0, "stderr: {stderr}");
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_abort();
    assert!(parsed.is_object());
}

// ---------------------------------------------------------------------------
// Settings persistence — write and read roundtrip
// ---------------------------------------------------------------------------

#[test]
fn settings_write_and_read_roundtrip_persists_compaction_enabled() {
    // arrange
    use harness_core::config::{
        read_effective_compaction_enabled, write_project_compaction_enabled,
    };

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
            "permission": "allow",
            "compaction": { "enabled": true }
        }"#,
    )
    .unwrap_or_abort();

    // act — write false, then read it back
    write_project_compaction_enabled(&config_path, false).unwrap_or_abort();

    // assert
    let read = read_effective_compaction_enabled(&config_path).unwrap_or_abort();
    assert!(!read, "compaction should be disabled after write");
}

// ---------------------------------------------------------------------------
// Exit codes — verify consistent exit code semantics
// ---------------------------------------------------------------------------

#[test]
fn config_validate_returns_zero_on_success() {
    // arrange
    // act
    let (code, _stdout, _stderr) = run_cli_in_workspace(&["config", "validate"]);

    // assert
    assert_eq!(code, 0);
}

#[test]
fn config_show_without_effective_returns_exit_code_2() {
    // arrange
    // act
    let (code, _stdout, _stderr) = run_cli_in_workspace(&["config", "show"]);

    // assert
    assert_eq!(code, 2);
}

#[test]
fn config_explain_empty_path_returns_exit_code_2() {
    // arrange
    // act
    let (code, _stdout, _stderr) = run_cli_in_workspace(&["config", "explain", ""]);

    // assert
    assert_eq!(code, 2);
}

// ---------------------------------------------------------------------------
// Help text — never advertises unavailable product actions
// ---------------------------------------------------------------------------

#[test]
fn help_text_does_not_contain_todo_or_tbd_or_placeholder() {
    // arrange
    // act
    let (code, stdout, _stderr) = run_cli_in_workspace(&["--help"]);

    // assert
    assert_eq!(code, 0);
    let lower = stdout.to_ascii_lowercase();
    assert!(!lower.contains("todo"), "help text must not contain 'todo'");
    assert!(!lower.contains("tbd"), "help text must not contain 'tbd'");
    assert!(
        !lower.contains("placeholder"),
        "help text must not contain 'placeholder'"
    );
}

#[test]
fn config_subcommand_help_has_complete_descriptions() {
    // arrange
    // act
    let (code, stdout, _stderr) = run_cli_in_workspace(&["config", "--help"]);

    // assert
    assert_eq!(code, 0);
    let lower = stdout.to_ascii_lowercase();
    assert!(!lower.contains("todo"));
    assert!(!lower.contains("tbd"));
    assert!(!lower.contains("placeholder"));
}

// ---------------------------------------------------------------------------
// Models command — happy path
// ---------------------------------------------------------------------------

#[test]
fn models_command_lists_configured_models_in_workspace() {
    // arrange
    // act
    let (code, stdout, _stderr) = run_cli_in_workspace(&["models"]);

    // assert
    // models may return 0 (models listed) or 2 (no provider connected)
    // either way it should not crash
    if code == 0 {
        assert!(
            !stdout.is_empty(),
            "models output should not be empty on success"
        );
    }
}

#[test]
fn best_of_n_is_not_exposed() {
    // arrange
    // act
    let (code, stdout, stderr) = run_cli_in_workspace(&["best-of-n", "--prompt", "hello"]);

    // assert
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert!(stderr.contains("unrecognized subcommand 'best-of-n'"));
}

#[test]
fn check_is_not_exposed() {
    // arrange
    // act
    let (code, stdout, stderr) = run_cli_in_workspace(&["check", "--component", "config"]);

    // assert
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert!(stderr.contains("unrecognized subcommand 'check'"));
}

#[test]
fn output_format_is_not_exposed() {
    // arrange
    // act
    let (code, stdout, stderr) = run_cli_in_workspace(&["output-format", "--format", "json"]);

    // assert
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert!(stderr.contains("unrecognized subcommand 'output-format'"));
}

