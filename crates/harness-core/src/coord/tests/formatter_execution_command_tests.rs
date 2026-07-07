use crate::UnwrapOrAbort;
use std::collections::BTreeMap;
use std::fs;

use crate::config::{FormatterConfig, FormatterOverride};
use crate::coord::formatter::{run_formatter_for_path_with_discovery, FakeFormatterDiscovery};

fn sh(script: &str) -> Vec<String> {
    vec!["sh".to_string(), "-c".to_string(), script.to_string()]
}

pub(super) async fn override_command_replaces_built_in_and_failure_is_non_fatal() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    fs::write(workspace.join("test.rs"), "fn main() {}").unwrap_or_abort();

    let mut overrides = BTreeMap::new();
    overrides.insert(
        "rustfmt".to_string(),
        FormatterOverride {
            disabled: false,
            command: Some(vec!["false".to_string()]),
            environment: None,
            extensions: Some(vec![".rs".to_string()]),
        },
    );
    let config = FormatterConfig {
        enabled: true,
        experimental_oxfmt: false,
        overrides,
    };

    let discovery = FakeFormatterDiscovery::new(["rustfmt"]);
    let err = run_formatter_for_path_with_discovery(&config, &workspace, "test.rs", &discovery)
        .await
        .expect_err("failing formatter returns warning");
    assert!(
        err.contains("formatter `false` failed"),
        "error surfaces command failure: {err}"
    );
}

pub(super) async fn disabled_override_skips_formatter() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    fs::write(workspace.join("test.rs"), "fn main() {}").unwrap_or_abort();

    let mut overrides = BTreeMap::new();
    overrides.insert(
        "rustfmt".to_string(),
        FormatterOverride {
            disabled: true,
            command: Some(sh("echo rustfmt > marker.txt")),
            environment: None,
            extensions: None,
        },
    );
    let config = FormatterConfig {
        enabled: true,
        experimental_oxfmt: false,
        overrides,
    };

    let discovery = FakeFormatterDiscovery::new(["rustfmt"]);
    run_formatter_for_path_with_discovery(&config, &workspace, "test.rs", &discovery)
        .await
        .unwrap_or_abort();
    assert!(!workspace.join("marker.txt").exists());
}

pub(super) async fn success_continues_after_one_formatter_fails() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    fs::write(workspace.join("test.rs"), "fn main() {}").unwrap_or_abort();

    let mut overrides = BTreeMap::new();
    overrides.insert(
        "a_first".to_string(),
        FormatterOverride {
            disabled: false,
            command: Some(vec!["false".to_string()]),
            environment: None,
            extensions: Some(vec![".rs".to_string()]),
        },
    );
    overrides.insert(
        "z_second".to_string(),
        FormatterOverride {
            disabled: false,
            command: Some(sh("echo ok > marker.txt")),
            environment: None,
            extensions: Some(vec![".rs".to_string()]),
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
        "test.rs",
        &FakeFormatterDiscovery::default(),
    )
    .await
    .expect_err("first failure is reported");
    assert!(err.contains("formatter `false` failed"));
    assert_eq!(
        fs::read_to_string(workspace.join("marker.txt")).unwrap_or_abort(),
        "ok\n"
    );
}

pub(super) async fn override_command_runs_even_when_builtin_not_on_path() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    fs::write(workspace.join("test.rs"), "fn main() {}").unwrap_or_abort();

    let mut overrides = BTreeMap::new();
    overrides.insert(
        "rustfmt".to_string(),
        FormatterOverride {
            disabled: false,
            command: Some(sh("echo override > marker.txt")),
            environment: None,
            extensions: None,
        },
    );
    let config = FormatterConfig {
        enabled: true,
        experimental_oxfmt: false,
        overrides,
    };

    let discovery = FakeFormatterDiscovery::default();
    run_formatter_for_path_with_discovery(&config, &workspace, "test.rs", &discovery)
        .await
        .unwrap_or_abort();

    assert_eq!(
        fs::read_to_string(workspace.join("marker.txt")).unwrap_or_abort(),
        "override\n"
    );
}
