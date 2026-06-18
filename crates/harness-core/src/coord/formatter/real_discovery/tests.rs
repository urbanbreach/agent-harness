use super::{DiscoveryContext, RealFormatterDiscovery};
use crate::coord::formatter::FormatterDiscovery;

#[test]
fn first_line_returns_first_line_or_empty() {
    assert_eq!(super::first_line("alpha\nbeta"), "alpha");
    assert_eq!(super::first_line(""), "");
    assert_eq!(super::first_line("only"), "only");
}

#[tokio::test]
async fn prettier_discovery_uses_local_npm_binary_when_dep_present() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    let node_bin = workspace.join("node_modules").join(".bin");
    std::fs::create_dir_all(&node_bin).expect("create node bin dir");
    std::fs::write(node_bin.join("prettier"), "").expect("create fake prettier binary");
    std::fs::write(
        workspace.join("package.json"),
        r#"{"devDependencies": {"prettier": "1.0.0"}}"#,
    )
    .expect("write package.json");

    let context = DiscoveryContext {
        workspace_root: workspace.clone(),
        target_dir: workspace.clone(),
        experimental_oxfmt: false,
    };

    let discovery = RealFormatterDiscovery;
    let command = discovery
        .resolve("prettier", &context)
        .await
        .expect("prettier discovered");
    assert_eq!(command.len(), 3);
    assert_eq!(command[1], "--write");
    assert_eq!(command[2], "$FILE");
}

#[tokio::test]
async fn biome_discovery_requires_biome_json() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    let node_bin = workspace.join("node_modules").join(".bin");
    std::fs::create_dir_all(&node_bin).expect("create node bin dir");
    let biome_bin = node_bin.join("biome");
    std::fs::write(&biome_bin, "#!/bin/sh\n").expect("create fake biome binary");
    make_executable(&biome_bin);
    std::fs::write(workspace.join("biome.json"), "{}").expect("write biome.json");

    let context = DiscoveryContext {
        workspace_root: workspace.clone(),
        target_dir: workspace.clone(),
        experimental_oxfmt: false,
    };

    let discovery = RealFormatterDiscovery;
    let command = discovery
        .resolve("biome", &context)
        .await
        .expect("biome discovered");
    assert_eq!(command[1..], ["format", "--write", "$FILE"]);
}

#[tokio::test]
async fn oxfmt_discovery_gated_by_experimental_oxfmt() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    let node_bin = workspace.join("node_modules").join(".bin");
    std::fs::create_dir_all(&node_bin).expect("create node bin dir");
    std::fs::write(node_bin.join("oxfmt"), "#!/bin/sh\n").expect("create fake oxfmt binary");
    make_executable(&node_bin.join("oxfmt"));
    std::fs::write(
        workspace.join("package.json"),
        r#"{"dependencies": {"oxfmt": "1.0.0"}}"#,
    )
    .expect("write package.json");

    let disabled_context = DiscoveryContext {
        workspace_root: workspace.clone(),
        target_dir: workspace.clone(),
        experimental_oxfmt: false,
    };
    let discovery = RealFormatterDiscovery;
    assert!(
        discovery
            .resolve("oxfmt", &disabled_context)
            .await
            .is_none(),
        "oxfmt is gated by experimentalOxfmt"
    );

    let enabled_context = DiscoveryContext {
        workspace_root: workspace.clone(),
        target_dir: workspace.clone(),
        experimental_oxfmt: true,
    };
    let command = discovery
        .resolve("oxfmt", &enabled_context)
        .await
        .expect("oxfmt discovered when enabled");
    assert_eq!(command.len(), 2);
    assert_eq!(command[1], "$FILE");
}

#[tokio::test]
async fn pint_discovery_requires_laravel_pint_in_composer() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("create workspace");
    std::fs::write(
        workspace.join("composer.json"),
        r#"{"require-dev": {"laravel/pint": "^1.0"}}"#,
    )
    .expect("write composer.json");

    let context = DiscoveryContext {
        workspace_root: workspace.clone(),
        target_dir: workspace.clone(),
        experimental_oxfmt: false,
    };
    let discovery = RealFormatterDiscovery;
    let command = discovery
        .resolve("pint", &context)
        .await
        .expect("pint discovered");
    assert_eq!(command, vec!["./vendor/bin/pint", "$FILE"]);
}

pub(super) fn make_executable(path: &std::path::Path) {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}
