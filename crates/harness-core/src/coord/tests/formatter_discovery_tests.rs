use std::collections::BTreeMap;
use std::fs;

use crate::config::{FormatterConfig, FormatterOverride};
use crate::coord::formatter::{
    resolve_formatter_names, run_formatter_for_path_with_discovery, FakeFormatterDiscovery,
};

fn sh(script: &str) -> Vec<String> {
    vec!["sh".to_string(), "-c".to_string(), script.to_string()]
}

pub(super) async fn built_in_discovery_includes_rustfmt_when_on_path() {
    let config = FormatterConfig {
        enabled: true,
        experimental_oxfmt: false,
        overrides: BTreeMap::new(),
    };

    let absent = FakeFormatterDiscovery::new([] as [&str; 0]);
    let names = resolve_formatter_names(&config, "rs", &absent).await;
    assert!(
        names.is_empty(),
        "rustfmt should be absent when not on PATH"
    );

    let present = FakeFormatterDiscovery::new(["rustfmt"]);
    let names = resolve_formatter_names(&config, "rs", &present).await;
    assert_eq!(
        names,
        vec!["rustfmt"],
        "rustfmt should be selected for .rs when on PATH"
    );
}

pub(super) async fn built_in_command_still_requires_discovery_when_only_extensions_overridden() {
    let mut overrides = BTreeMap::new();
    overrides.insert(
        "rustfmt".to_string(),
        FormatterOverride {
            disabled: false,
            command: None,
            environment: None,
            extensions: Some(vec![".rs".to_string()]),
        },
    );
    let config = FormatterConfig {
        enabled: true,
        experimental_oxfmt: false,
        overrides,
    };

    let absent = FakeFormatterDiscovery::new([] as [&str; 0]);
    let names = resolve_formatter_names(&config, "rs", &absent).await;
    assert!(
        names.is_empty(),
        "rustfmt should be skipped when not on PATH"
    );

    let present = FakeFormatterDiscovery::new(["rustfmt"]);
    let names = resolve_formatter_names(&config, "rs", &present).await;
    assert_eq!(names, vec!["rustfmt"]);
}

pub(super) async fn multiple_matching_formatters_run_in_sorted_order() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("create workspace");
    fs::write(workspace.join("test.py"), "x = 1\n").expect("write py");

    let mut overrides = BTreeMap::new();
    overrides.insert(
        "ruff".to_string(),
        FormatterOverride {
            disabled: false,
            command: Some(sh("echo ruff >> markers.txt")),
            environment: None,
            extensions: None,
        },
    );
    overrides.insert(
        "uvformat".to_string(),
        FormatterOverride {
            disabled: false,
            command: Some(sh("echo uvformat >> markers.txt")),
            environment: None,
            extensions: None,
        },
    );
    let config = FormatterConfig {
        enabled: true,
        experimental_oxfmt: false,
        overrides,
    };

    let discovery = FakeFormatterDiscovery::new(["ruff", "uvformat"]);
    run_formatter_for_path_with_discovery(&config, &workspace, "test.py", &discovery)
        .await
        .expect("formatters run");

    let markers = fs::read_to_string(workspace.join("markers.txt")).expect("read markers");
    let lines: Vec<_> = markers.lines().collect();
    assert_eq!(lines, vec!["ruff", "uvformat"]);
}

pub(super) async fn ruff_uv_coupling_skips_both_when_one_disabled() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("create workspace");
    fs::write(workspace.join("test.py"), "x = 1\n").expect("write py");

    let mut overrides = BTreeMap::new();
    overrides.insert(
        "ruff".to_string(),
        FormatterOverride {
            disabled: true,
            command: None,
            environment: None,
            extensions: None,
        },
    );
    overrides.insert(
        "uvformat".to_string(),
        FormatterOverride {
            disabled: false,
            command: Some(sh("echo uvformat > marker.txt")),
            environment: None,
            extensions: None,
        },
    );
    let config = FormatterConfig {
        enabled: true,
        experimental_oxfmt: false,
        overrides,
    };

    let discovery = FakeFormatterDiscovery::new(["ruff", "uvformat"]);
    run_formatter_for_path_with_discovery(&config, &workspace, "test.py", &discovery)
        .await
        .expect("coupled skip returns ok");
    assert!(!workspace.join("marker.txt").exists());
}
