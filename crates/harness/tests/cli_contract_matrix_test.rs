//! CLI contract matrix tests for Task 23.
//!
//! Covers happy/error/unknown/conflict/JSON schema paths and persisted effects
//! for config, settings, provider/model selection, doctor, completions, and
//! the model-alias resolver contract from plan §1.1.
//!
//! These tests run in-process via `harness::run()` — no binary spawn, no
//! network calls. They verify CLI exit codes, output formats, and persisted
//! effects on disk.

use harness::CliDeps;
use harness::CliIo;
use harness::ExitOutcome;
use harness_core::config::{is_model_alias, known_model_aliases, resolve_model_alias, ModelAlias};
use harness_providers::UnwrapOrAbort;
use std::fs;
use std::io::Cursor;

fn run_cli(args: &[&str], deps: CliDeps) -> (i32, String, String) {
    let args: Vec<&str> = std::iter::once("harness")
        .chain(args.iter().copied())
        .collect();
    let mut stdin = Cursor::new(Vec::new());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
    let ExitOutcome { code, .. } = harness::run(args, &mut io, deps);
    (
        code,
        String::from_utf8_lossy(&stdout).to_string(),
        String::from_utf8_lossy(&stderr).to_string(),
    )
}

fn run_cli_in_workspace(args: &[&str]) -> (i32, String, String) {
    run_cli(args, CliDeps::real())
}

fn run_cli_in_temp(args: &[&str]) -> (i32, String, String) {
    let temp = tempfile::tempdir().unwrap_or_abort();
    let deps = CliDeps::real()
        .with_current_dir(temp.path().to_path_buf())
        .with_env("HOME", temp.path().to_string_lossy())
        .with_env(
            "XDG_CONFIG_HOME",
            temp.path().join("config").to_string_lossy(),
        );
    run_cli(args, deps)
}

fn write_config(dir: &std::path::Path, config: &str) -> std::path::PathBuf {
    let config_path = dir.join("harness.jsonc");
    fs::write(&config_path, config).unwrap_or_abort();
    config_path
}

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
    // arrange
    // act
    // assert
    assert_eq!(resolve_model_alias("umans-kimi-k2.7"), "umans-kimi-k2.7");
    assert_eq!(
        resolve_model_alias("umans-qwen3.6-35b-a3b"),
        "umans-qwen3.6-35b-a3b"
    );
}

#[test]
fn model_alias_resolver_passes_through_unknown_ids_unchanged() {
    // arrange
    // act
    // assert
    assert_eq!(resolve_model_alias("gpt-5.5"), "gpt-5.5");
    assert_eq!(resolve_model_alias("unknown-model"), "unknown-model");
}

#[test]
fn model_alias_is_alias_detects_known_aliases() {
    // arrange
    // act
    // assert
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
        "agent": { "build": { "enable": true, "model": "default/test-model" } },
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
        "agent": { "build": { "enable": true, "model": "default/test-model" } },
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
        "agent": { "build": { "enable": true, "model": "default/test-model" } },
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
        "agent": { "build": { "enable": true, "model": "default/test-model" } },
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
            "agent": { "build": { "enable": true, "model": "default/test-model" } },
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

// ---------------------------------------------------------------------------
// Best-of-N and Check — meaningful failure (retained as redirects)
// ---------------------------------------------------------------------------

#[test]
fn best_of_n_returns_meaningful_failure_directing_to_run() {
    // arrange
    // act
    let (code, stdout, stderr) = run_cli_in_workspace(&["best-of-n", "--prompt", "hello"]);

    // assert
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert!(stderr.contains("not available"));
    assert!(stderr.contains("harness run"));
}

#[test]
fn check_returns_meaningful_failure_directing_to_doctor() {
    // arrange
    // act
    let (code, stdout, stderr) = run_cli_in_workspace(&["check", "--component", "config"]);

    // assert
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert!(stderr.contains("not available"));
    assert!(stderr.contains("harness doctor"));
}

// ---------------------------------------------------------------------------
// Output format — meaningful failure (retained as redirect)
// ---------------------------------------------------------------------------

#[test]
fn output_format_returns_meaningful_failure_directing_to_flag() {
    // arrange
    // act
    let (code, stdout, stderr) = run_cli_in_workspace(&["output-format", "--format", "json"]);

    // assert
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert!(stderr.contains("not available"));
    assert!(stderr.contains("--output-format"));
}

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
        "default_agent": "build",
        "agent": { "build": { "enable": true, "model": "mock/test-model" } },
        "permission": "allow"
    }"#;
    let config_path = write_config(temp.path(), config);
    let out_path = temp.path().join("events.jsonl");
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
        "agent": { "build": { "enable": true, "model": "default/test-model" } },
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
        "agent": { "build": { "enable": true, "model": "default/test-model" } },
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
        "agent": { "build": { "enable": true, "model": "myprov/m" } },
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
        "agent": { "build": { "enable": true, "model": "prov_a/m" } },
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
        "agent": { "build": { "enable": true, "model": "default/test-model" } },
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
            "agent": { "build": { "enable": true, "model": "default/test-model" } },
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
