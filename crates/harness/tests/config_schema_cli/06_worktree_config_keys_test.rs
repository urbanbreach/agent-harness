use harness::UnwrapOrAbort;
use harness_core::config::{
    load_config_from_file, write_project_setting_string, read_effective_worktree_relative_base,
    read_effective_worktree_branch_prefix, PublicRuntimeConfig, SettingWriteError,
};

fn worktree_runtime_config(relative_base: &str) -> serde_json::Value {
    let mut config = canonical_runtime_config();
    if let Some(obj) = config.as_object_mut() {
        obj.insert(
            "worktree".to_string(),
            serde_json::json!({ "relative_base": relative_base }),
        );
    }
    config
}

fn worktree_runtime_config_with_branch_prefix(branch_prefix: &str) -> serde_json::Value {
    let mut config = canonical_runtime_config();
    if let Some(obj) = config.as_object_mut() {
        obj.insert(
            "worktree".to_string(),
            serde_json::json!({ "branch_prefix": branch_prefix }),
        );
    }
    config
}

#[test]
fn public_runtime_config_parses_worktree_settings() {
    // arrange
    // act
    // assert
    let parsed: PublicRuntimeConfig = json5::from_str(
        r#"
        {
          worktree: {
            relative_base: ".agent-harness/worktrees",
            branch_prefix: "harness/wt-"
          }
        }
        "#,
    )
    .unwrap_or_abort();

    assert_eq!(
        parsed.worktree.relative_base.as_deref(),
        Some(".agent-harness/worktrees")
    );
    assert_eq!(
        parsed.worktree.branch_prefix.as_deref(),
        Some("harness/wt-")
    );
}

#[test]
fn public_runtime_config_accepts_camel_case_worktree_aliases() {
    // arrange
    // act
    // assert
    let parsed: PublicRuntimeConfig = json5::from_str(
        r#"
        {
          worktree: {
            relativeBase: ".git/worktrees",
            branchPrefix: "feature/wt-"
          }
        }
        "#,
    )
    .unwrap_or_abort();

    assert_eq!(
        parsed.worktree.relative_base.as_deref(),
        Some(".git/worktrees")
    );
    assert_eq!(
        parsed.worktree.branch_prefix.as_deref(),
        Some("feature/wt-")
    );
}

#[test]
fn public_runtime_config_defaults_worktree_to_empty() {
    // arrange
    // act
    // assert
    let parsed: PublicRuntimeConfig = json5::from_str(r#"{}"#).unwrap_or_abort();
    assert!(parsed.worktree.relative_base.is_none());
    assert!(parsed.worktree.branch_prefix.is_none());
}

#[test]
fn config_validation_rejects_absolute_worktree_relative_base() {
    // arrange
    let temp = tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.jsonc");
    write_config(&config_path, &worktree_runtime_config("/absolute/path"));

    // act
    let result = load_config_from_file(&config_path);

    // assert
    let err = result.expect_err("absolute relative_base should fail validation");
    let msg = err.to_string();
    assert!(
        msg.contains("worktree.relative_base") && msg.contains("absolute"),
        "expected absolute path rejection, got: {msg}"
    );
}

#[test]
fn config_validation_rejects_parent_dir_traversal_in_worktree_relative_base() {
    // arrange
    let temp = tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.jsonc");
    write_config(&config_path, &worktree_runtime_config("../escape/attempt"));

    // act
    let result = load_config_from_file(&config_path);

    // assert
    let err = result.expect_err("parent-dir traversal should fail validation");
    let msg = err.to_string();
    assert!(
        msg.contains("worktree.relative_base") && msg.contains(".."),
        "expected parent-dir rejection, got: {msg}"
    );
}

#[test]
fn config_validation_accepts_valid_worktree_relative_base() {
    // arrange
    let temp = tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.jsonc");
    write_config(&config_path, &worktree_runtime_config(".agent-harness/worktrees"));

    // act
    let config = load_config_from_file(&config_path).unwrap_or_abort();

    // assert
    assert_eq!(
        config.worktree.relative_base.as_deref(),
        Some(".agent-harness/worktrees")
    );
}

#[test]
fn config_explain_attributes_worktree_relative_base() {
    // arrange
    let temp = tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.jsonc");
    let mut config = canonical_runtime_config();
    if let Some(obj) = config.as_object_mut() {
        obj.insert(
            "worktree".to_string(),
            serde_json::json!({
                "relative_base": ".agent-harness/worktrees",
                "branch_prefix": "harness/wt-"
            }),
        );
    }
    write_config(&config_path, &config);

    // act
    let output = harness_command()
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "config",
            "explain",
            "worktree.relative_base",
        ])
        .output()
        .unwrap_or_abort();

    // assert
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("harness-config-explain-v1"));
    assert!(stdout.contains(".agent-harness/worktrees"));
    assert!(stdout.contains("\"found\": true"));
}

#[test]
fn config_explain_attributes_worktree_branch_prefix() {
    // arrange
    let temp = tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.jsonc");
    write_config(&config_path, &worktree_runtime_config_with_branch_prefix("custom/wt-"));

    // act
    let output = harness_command()
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "config",
            "explain",
            "worktree.branch_prefix",
        ])
        .output()
        .unwrap_or_abort();

    // assert
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("harness-config-explain-v1"));
    assert!(stdout.contains("custom/wt-"));
    assert!(stdout.contains("\"found\": true"));
}

#[test]
fn settings_write_persists_worktree_relative_base_atomically() {
    // arrange
    let temp = tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.jsonc");
    write_config(&config_path, &canonical_runtime_config());

    // act
    let reloaded = write_project_setting_string(&config_path, "worktree.relative_base", ".git/wt")
        .unwrap_or_abort();

    // assert
    assert_eq!(reloaded, ".git/wt");
    let effective = read_effective_worktree_relative_base(&config_path).unwrap_or_abort();
    assert_eq!(effective.as_deref(), Some(".git/wt"));
}

#[test]
fn settings_write_persists_worktree_branch_prefix_atomically() {
    // arrange
    let temp = tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.jsonc");
    write_config(&config_path, &canonical_runtime_config());

    // act
    let reloaded =
        write_project_setting_string(&config_path, "worktree.branch_prefix", "feature/wt-")
            .unwrap_or_abort();

    // assert
    assert_eq!(reloaded, "feature/wt-");
    let effective = read_effective_worktree_branch_prefix(&config_path).unwrap_or_abort();
    assert_eq!(effective.as_deref(), Some("feature/wt-"));
}

#[test]
fn settings_write_rejects_unregistered_worktree_key() {
    // arrange
    let temp = tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.jsonc");
    write_config(&config_path, &canonical_runtime_config());

    // act
    let result = write_project_setting_string(&config_path, "worktree.unknown_key", "value");

    // assert
    let err = result.expect_err("unregistered key should fail");
    assert!(matches!(err, SettingWriteError::UnknownSetting(_)));
}

#[test]
fn generated_schema_includes_worktree_definition() {
    // arrange
    let schema = harness_core::config::harness_schema_pretty_json().unwrap_or_abort();
    let schema: serde_json::Value = serde_json::from_str(&schema).unwrap_or_abort();

    // act
    let worktree_prop = &schema["properties"]["worktree"];
    let worktree_def = &schema["definitions"]["PublicWorktreeConfig"];

    // assert
    assert!(worktree_prop.is_object(), "worktree must be a schema property");
    assert!(
        worktree_def["properties"]["relative_base"].is_object(),
        "PublicWorktreeConfig must define relative_base"
    );
    assert!(
        worktree_def["properties"]["branch_prefix"].is_object(),
        "PublicWorktreeConfig must define branch_prefix"
    );
}
