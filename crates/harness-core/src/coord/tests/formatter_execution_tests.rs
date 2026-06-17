use std::collections::BTreeMap;
use std::fs;

use crate::config::{FormatterConfig, FormatterOverride};
use crate::coord::formatter::{run_formatter_for_path_with_discovery, FakeFormatterDiscovery};

fn sh(script: &str) -> Vec<String> {
    vec!["sh".to_string(), "-c".to_string(), script.to_string()]
}

pub(super) async fn file_substitution_replaces_token_and_falls_back_to_append() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("create workspace");
    fs::write(workspace.join("target.txt"), "hello").expect("write target");

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
    .expect("substitution succeeds");

    assert!(workspace.join("target.txt.bak").exists());
    assert_eq!(
        fs::read_to_string(workspace.join("target.txt.bak")).expect("read backup"),
        "hello"
    );
}

pub(super) async fn override_command_replaces_built_in_and_failure_is_non_fatal() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("create workspace");
    fs::write(workspace.join("test.rs"), "fn main() {}").expect("write rs");

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
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("create workspace");
    fs::write(workspace.join("test.rs"), "fn main() {}").expect("write rs");

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
        .expect("skipped formatter returns ok");
    assert!(!workspace.join("marker.txt").exists());
}

pub(super) async fn environment_variables_merge_with_override_winning() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("create workspace");
    fs::write(workspace.join("test.js"), "x").expect("write js");

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
        .expect("formatter with env runs");

    let env = fs::read_to_string(workspace.join("env.txt")).expect("read env dump");
    assert_eq!(env.trim(), "2 1");
}

pub(super) async fn path_escape_returns_warning_and_does_not_touch_external_file() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    let outside = temp_dir.path().join("outside.txt");
    fs::create_dir_all(&workspace).expect("create workspace");
    fs::write(&outside, "outside").expect("write outside file");

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
    assert_eq!(
        fs::read_to_string(&outside).expect("read outside"),
        "outside"
    );
}

pub(super) async fn success_continues_after_one_formatter_fails() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("create workspace");
    fs::write(workspace.join("test.rs"), "fn main() {}").expect("write rs");

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
        fs::read_to_string(workspace.join("marker.txt")).expect("read marker"),
        "ok\n"
    );
}

pub(super) async fn override_command_runs_even_when_builtin_not_on_path() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("create workspace");
    fs::write(workspace.join("test.rs"), "fn main() {}").expect("write rs");

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
        .expect("override command runs without PATH discovery");

    assert_eq!(
        fs::read_to_string(workspace.join("marker.txt")).expect("read marker"),
        "override\n"
    );
}

pub(super) async fn extension_override_replaces_builtin_extension_list() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("create workspace");
    fs::write(workspace.join("test.rs"), "fn main() {}").expect("write rs");
    fs::write(workspace.join("test.py"), "x = 1\n").expect("write py");

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

    fs::write(workspace.join("marker.txt"), "sentinel").expect("write sentinel");

    run_formatter_for_path_with_discovery(&config, &workspace, "test.rs", &discovery)
        .await
        .expect("rustfmt no longer matches .rs after override");
    assert_eq!(
        fs::read_to_string(workspace.join("marker.txt")).expect("read marker after rs"),
        "sentinel",
        ".rs should not have triggered rustfmt"
    );

    run_formatter_for_path_with_discovery(&config, &workspace, "test.py", &discovery)
        .await
        .expect("rustfmt matches overridden .py extension");
    assert_eq!(
        fs::read_to_string(workspace.join("marker.txt")).expect("read marker after py"),
        "rustfmt\n"
    );
}
