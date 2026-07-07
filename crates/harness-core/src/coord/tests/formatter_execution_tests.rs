use crate::UnwrapOrAbort;
use std::collections::BTreeMap;
use std::fs;

use crate::config::{FormatterConfig, FormatterOverride};
use crate::coord::formatter::{run_formatter_for_path_with_discovery, FakeFormatterDiscovery};

fn sh(script: &str) -> Vec<String> {
    vec!["sh".to_string(), "-c".to_string(), script.to_string()]
}

pub(super) async fn file_substitution_replaces_token_and_falls_back_to_append() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    fs::write(workspace.join("target.txt"), "hello").unwrap_or_abort();

    let mut overrides = BTreeMap::new();
    overrides.insert(
        "backup".to_string(),
        FormatterOverride {
            disabled: false,
            command: Some(vec![
                "sh".to_string(),
                "-c".to_string(),
                "cp $FILE $FILE.bak".to_string(),
            ]),
            environment: None,
            extensions: Some(vec![".txt".to_string()]),
        },
    );
    let config = FormatterConfig {
        enabled: true,
        experimental_oxfmt: false,
        overrides,
    };

    run_formatter_for_path_with_discovery(
        &config,
        &workspace,
        "target.txt",
        &FakeFormatterDiscovery::default(),
    )
    .await
    .unwrap_or_abort();

    assert!(workspace.join("target.txt.bak").exists());
    assert_eq!(
        fs::read_to_string(workspace.join("target.txt.bak")).unwrap_or_abort(),
        "hello"
    );
}

pub(super) async fn environment_variables_merge_with_override_winning() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    fs::write(workspace.join("test.js"), "x").unwrap_or_abort();

    let mut env = BTreeMap::new();
    env.insert("BUN_BE_BUN".to_string(), "2".to_string());
    env.insert("CUSTOM".to_string(), "1".to_string());

    let mut overrides = BTreeMap::new();
    overrides.insert(
        "prettier".to_string(),
        FormatterOverride {
            disabled: false,
            command: Some(sh(r#"printf "%s %s\n" "$BUN_BE_BUN" "$CUSTOM" > env.txt"#)),
            environment: Some(env),
            extensions: None,
        },
    );
    let config = FormatterConfig {
        enabled: true,
        experimental_oxfmt: false,
        overrides,
    };

    let discovery = FakeFormatterDiscovery::new(["prettier"]);
    run_formatter_for_path_with_discovery(&config, &workspace, "test.js", &discovery)
        .await
        .unwrap_or_abort();

    let env = fs::read_to_string(workspace.join("env.txt")).unwrap_or_abort();
    assert_eq!(env.trim(), "2 1");
}

pub(super) async fn path_escape_returns_warning_and_does_not_touch_external_file() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    let outside = temp_dir.path().join("outside.txt");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    fs::write(&outside, "outside").unwrap_or_abort();

    let mut overrides = BTreeMap::new();
    overrides.insert(
        "escape".to_string(),
        FormatterOverride {
            disabled: false,
            command: Some(sh("echo leaked > $1")),
            environment: None,
            extensions: Some(vec![".txt".to_string()]),
        },
    );
    let config = FormatterConfig {
        enabled: true,
        experimental_oxfmt: false,
        overrides,
    };

    let err = run_formatter_for_path_with_discovery(
        &config,
        &workspace,
        "../outside.txt",
        &FakeFormatterDiscovery::default(),
    )
    .await
    .expect_err("escape must be rejected");
    assert!(
        err.contains("escapes workspace root"),
        "expected workspace escape error, got: {err}"
    );
    assert_eq!(fs::read_to_string(&outside).unwrap_or_abort(), "outside");
}

pub(super) async fn enabled_false_skips_all_formatters() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    fs::write(workspace.join("target.rs"), "fn main() {}").unwrap_or_abort();

    let mut overrides = BTreeMap::new();
    overrides.insert(
        "rustfmt".to_string(),
        FormatterOverride {
            disabled: false,
            command: Some(sh("echo ran > marker.txt")),
            environment: None,
            extensions: None,
        },
    );
    let config = FormatterConfig {
        enabled: false,
        experimental_oxfmt: false,
        overrides,
    };

    let discovery = FakeFormatterDiscovery::new(["rustfmt"]);
    run_formatter_for_path_with_discovery(&config, &workspace, "target.rs", &discovery)
        .await
        .unwrap_or_abort();
    assert!(
        !workspace.join("marker.txt").exists(),
        "no formatter should run when enabled is false"
    );
}

pub(super) async fn extension_override_replaces_builtin_extension_list() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    fs::write(workspace.join("test.rs"), "fn main() {}").unwrap_or_abort();
    fs::write(workspace.join("test.py"), "x = 1\n").unwrap_or_abort();

    let mut overrides = BTreeMap::new();
    overrides.insert(
        "rustfmt".to_string(),
        FormatterOverride {
            disabled: false,
            command: Some(sh("echo rustfmt > marker.txt")),
            environment: None,
            extensions: Some(vec![".py".to_string()]),
        },
    );
    let config = FormatterConfig {
        enabled: true,
        experimental_oxfmt: false,
        overrides,
    };

    let discovery = FakeFormatterDiscovery::new(["rustfmt"]);

    fs::write(workspace.join("marker.txt"), "sentinel").unwrap_or_abort();

    run_formatter_for_path_with_discovery(&config, &workspace, "test.rs", &discovery)
        .await
        .unwrap_or_abort();
    assert_eq!(
        fs::read_to_string(workspace.join("marker.txt")).unwrap_or_abort(),
        "sentinel",
        ".rs should not have triggered rustfmt"
    );

    run_formatter_for_path_with_discovery(&config, &workspace, "test.py", &discovery)
        .await
        .unwrap_or_abort();
    assert_eq!(
        fs::read_to_string(workspace.join("marker.txt")).unwrap_or_abort(),
        "rustfmt\n"
    );
}
