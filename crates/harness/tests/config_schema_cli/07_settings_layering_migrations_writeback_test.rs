use harness::UnwrapOrAbort;
use harness_core::config::{
    explain_setting, harness_schema_pretty_json, harness_tui_schema_pretty_json,
    read_effective_compaction_auto_retry_overflow, read_effective_compaction_enabled,
    read_effective_hashline_edit, read_effective_worktree_branch_prefix,
    read_effective_worktree_relative_base, resolve_setting_id, setting_definition,
    settings_compat_migrations, settings_registry, summarize_settings_registry,
    write_project_compaction_auto_retry_overflow, write_project_compaction_enabled,
    write_project_hashline_edit, write_project_worktree_branch_prefix,
    write_project_worktree_relative_base,
};

fn minimal_runtime_config_json() -> String {
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
    }"#
    .to_string()
}

fn write_test_config(dir: &Path) -> PathBuf {
    let config_path = dir.join("harness.jsonc");
    fs::write(&config_path, minimal_runtime_config_json()).unwrap_or_abort();
    config_path
}

#[test]
fn settings_registry_has_runtime_and_tui_surfaces_with_secret_classification() {
    // arrange
    // act
    let registry = settings_registry();
    let summary = summarize_settings_registry();

    // assert
    assert!(registry.len() >= 20, "registry must cover the public settings surface");
    assert!(summary.runtime > 0);
    assert!(summary.tui > 0);
    assert_eq!(summary.total, summary.runtime + summary.tui);
    assert!(summary.secret >= 1, "provider.apiKey must be classified secret");
    assert!(summary.editable > 0, "some settings must be editable");
    assert!(summary.read_only > 0, "some settings must be read-only");
}

#[test]
fn settings_registry_tui_surface_excludes_runtime_only_keys() {
    // arrange
    // act
    let tui_entries: Vec<_> = settings_registry()
        .iter()
        .filter(|def| {
            let explanation = explain_setting(def.setting_id.as_str()).unwrap_or_abort();
            explanation.surface == "tui"
        })
        .collect();

    // assert
    assert!(
        tui_entries.iter().all(|def| {
            let id = def.setting_id.as_str();
            id != "model" && id != "provider" && id != "permission"
        }),
        "TUI surface must not contain runtime-only keys"
    );
    assert!(
        tui_entries.iter().any(|def| def.setting_id.as_str() == "keybinds"),
        "keybinds must be a TUI-surface setting"
    );
}

#[test]
fn legacy_setting_ids_resolve_through_compat_migrations() {
    // arrange
    let migrations = settings_compat_migrations();

    // act
    // assert
    assert!(
        !migrations.is_empty(),
        "at least one legacy migration must exist"
    );
    for migration in migrations {
        let canonical = resolve_setting_id(migration.legacy_id);
        assert_eq!(
            canonical,
            Some(migration.canonical_id),
            "legacy `{}` must resolve to `{}`",
            migration.legacy_id,
            migration.canonical_id
        );
        let definition = setting_definition(migration.legacy_id);
        assert!(
            definition.is_some(),
            "legacy `{}` must resolve to a valid registry entry",
            migration.legacy_id
        );
    }
}

#[test]
fn writeback_hashline_edit_roundtrips_bool_value() {
    // arrange
    let dir = tempdir().unwrap_or_abort();
    let config_path = write_test_config(dir.path());

    // act
    write_project_hashline_edit(&config_path, false).unwrap_or_abort();
    let disabled = read_effective_hashline_edit(&config_path).unwrap_or_abort();

    // assert
    assert!(!disabled, "hashline_edit must read false after write");

    write_project_hashline_edit(&config_path, true).unwrap_or_abort();
    let enabled = read_effective_hashline_edit(&config_path).unwrap_or_abort();
    assert!(enabled, "hashline_edit must read true after re-enable");
}

#[test]
fn writeback_compaction_settings_roundtrip_independently() {
    // arrange
    let dir = tempdir().unwrap_or_abort();
    let config_path = write_test_config(dir.path());

    // act
    write_project_compaction_enabled(&config_path, false).unwrap_or_abort();
    write_project_compaction_auto_retry_overflow(&config_path, false).unwrap_or_abort();

    // assert
    let compaction = read_effective_compaction_enabled(&config_path).unwrap_or_abort();
    let auto_retry =
        read_effective_compaction_auto_retry_overflow(&config_path).unwrap_or_abort();
    assert!(!compaction);
    assert!(!auto_retry);
}

#[test]
fn writeback_worktree_settings_roundtrip_string_values() {
    // arrange
    let dir = tempdir().unwrap_or_abort();
    let config_path = dir.path().join("harness.jsonc");
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
            "worktree": { "relative_base": ".worktrees", "branch_prefix": "harness/" }
        }"#,
    )
    .unwrap_or_abort();

    // act
    let written_base =
        write_project_worktree_relative_base(&config_path, "custom-worktrees").unwrap_or_abort();
    let written_prefix =
        write_project_worktree_branch_prefix(&config_path, "harness/feature/").unwrap_or_abort();

    // assert
    assert_eq!(written_base, "custom-worktrees");
    assert_eq!(written_prefix, "harness/feature/");
    let base = read_effective_worktree_relative_base(&config_path).unwrap_or_abort();
    let prefix = read_effective_worktree_branch_prefix(&config_path).unwrap_or_abort();
    assert_eq!(base.as_deref(), Some("custom-worktrees"));
    assert_eq!(prefix.as_deref(), Some("harness/feature/"));
}

#[test]
fn schema_generation_runtime_and_tui_contracts_are_separate() {
    // arrange
    // act
    let runtime_json = harness_schema_pretty_json().unwrap_or_abort();
    let tui_json = harness_tui_schema_pretty_json().unwrap_or_abort();

    // assert
    let runtime: Value = serde_json::from_str(&runtime_json).unwrap_or_abort();
    let tui: Value = serde_json::from_str(&tui_json).unwrap_or_abort();

    let runtime_props = runtime["properties"].as_object().unwrap_or_abort();
    let tui_props = tui["properties"].as_object().unwrap_or_abort();

    for key in ["provider", "model", "permission", "agent", "mcp", "skills"] {
        assert!(
            runtime_props.contains_key(key),
            "runtime schema must own `{key}`"
        );
    }
    for key in ["provider", "model", "permission", "agent"] {
        assert!(
            !tui_props.contains_key(key),
            "TUI schema must not own runtime key `{key}`"
        );
    }
}

#[test]
fn explain_setting_reports_project_write_support_for_editable_bool_settings() {
    // arrange
    let editable_bool_ids = [
        "hashline_edit",
        "runtime.compaction.enabled",
        "runtime.compaction.auto_retry_overflow",
        "runtime.deterministic.enabled",
    ];

    // act
    // assert
    for setting_id in editable_bool_ids {
        let explanation = explain_setting(setting_id).unwrap_or_abort();
        assert_eq!(
            explanation.mutability, "editable",
            "{setting_id} must be editable"
        );
        assert_eq!(
            explanation.surface, "runtime",
            "{setting_id} must be a runtime setting"
        );
        assert!(
            explanation.project_write_supported,
            "{setting_id} must support project-file write"
        );
    }
}

#[test]
fn explain_setting_marks_secret_provider_api_key_read_only() {
    // arrange
    // act
    let explanation = explain_setting("provider.apiKey").unwrap_or_abort();

    // assert
    assert_eq!(explanation.sensitivity, "secret");
    assert_eq!(explanation.mutability, "read_only");
    assert!(!explanation.project_write_supported);
}

#[test]
fn config_validate_cli_accepts_layered_harness_and_tui_files() {
    // arrange
    let dir = tempdir().unwrap_or_abort();
    let config = canonical_runtime_config();
    let config_path = dir.path().join("harness.jsonc");
    write_config(&config_path, &config);

    let tui_config = serde_json::json!({ "keybinds": {} });
    write_config(&dir.path().join("tui.jsonc"), &tui_config);

    // act
    let output = harness_command()
        .arg("--config")
        .arg(config_path.to_str().unwrap_or_abort())
        .arg("config")
        .arg("validate")
        .current_dir(dir.path())
        .output()
        .unwrap_or_abort();

    // assert
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("config valid:"));
}

#[test]
fn effective_config_is_deterministic_across_repeated_calls() {
    // arrange
    let dir = tempdir().unwrap_or_abort();
    let config = canonical_runtime_config();
    let config_path = dir.path().join("harness.jsonc");
    write_config(&config_path, &config);

    // act
    let first = harness_command()
        .arg("--config")
        .arg(config_path.to_str().unwrap_or_abort())
        .arg("config")
        .arg("show")
        .arg("--effective")
        .current_dir(dir.path())
        .output()
        .unwrap_or_abort();
    let second = harness_command()
        .arg("--config")
        .arg(config_path.to_str().unwrap_or_abort())
        .arg("config")
        .arg("show")
        .arg("--effective")
        .current_dir(dir.path())
        .output()
        .unwrap_or_abort();

    // assert
    assert!(first.status.success());
    assert!(second.status.success());
    assert_eq!(
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&second.stdout),
        "effective config must be deterministic across calls"
    );
}
