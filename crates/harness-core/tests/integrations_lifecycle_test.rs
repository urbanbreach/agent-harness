//! Plugin lifecycle foundation tests (T10).
//!
//! Happy / edge / adjacent coverage for descriptor-only plugin install and the
//! durable plugin registry.

use std::fs;
use std::path::{Path, PathBuf};

use harness_core::extension_manifest::EXTENSION_MANIFEST_V1_SCHEMA_VERSION;
use harness_core::integrations::{
    run_multi_descriptor_discover_product, run_multi_plugin_lifecycle_product,
    PluginActivationPermission, PluginEnablement, PluginLifecycleError, PluginLifecycleRegistry,
    PluginLifecycleSummary, PluginRuntimeContract, PLUGIN_ENTRY_FILE_NAME, PLUGIN_HOOKS_FILE_NAME,
    PLUGIN_LOAD_RECEIPT_FILE_NAME, PLUGIN_MANIFEST_FILE_NAME, PLUGIN_REGISTRY_REL,
    PLUGIN_SKILLS_DIR_NAME, PROBE_EXTENSION_ALT_ID, PROBE_EXTENSION_PRIMARY_ID,
    PROBE_EXTENSION_TOOLS_ID, PROBE_PLUGIN_PRIMARY_ID, PROBE_PLUGIN_SECONDARY_ID,
};
use harness_core::UnwrapOrAbort;
use serde_json::json;

fn valid_manifest_body(id: &str) -> String {
    json!({
        "schemaVersion": EXTENSION_MANIFEST_V1_SCHEMA_VERSION,
        "id": id,
        "displayName": "Test plugin",
        "version": "0.1.0",
        "capabilities": [
            {"id": "cap.demo", "defaultEnabled": true}
        ]
    })
    .to_string()
}

fn write_plugin_package(workspace: &Path, dir_name: &str, manifest_id: &str) -> PathBuf {
    let package = workspace.join(dir_name);
    fs::create_dir_all(&package).unwrap_or_abort();
    fs::write(
        package.join(PLUGIN_MANIFEST_FILE_NAME),
        valid_manifest_body(manifest_id),
    )
    .unwrap_or_abort();
    package
}

fn write_hooks_json(package: &Path) {
    fs::write(
        package.join(PLUGIN_HOOKS_FILE_NAME),
        r#"{"hooks":[{"id":"demo.on_start","event":"run_started"}]}"#,
    )
    .unwrap_or_abort();
}

fn write_skills_dir(package: &Path) {
    let skill = package.join(PLUGIN_SKILLS_DIR_NAME).join("demo-skill");
    fs::create_dir_all(&skill).unwrap_or_abort();
    fs::write(skill.join("SKILL.md"), "# demo\n").unwrap_or_abort();
}

fn write_plugin_entry(package: &Path, entrypoints: &[&str]) {
    let body = json!({
        "schemaVersion": "plugin.entry.v1",
        "entrypoints": entrypoints,
    })
    .to_string();
    fs::write(package.join(PLUGIN_ENTRY_FILE_NAME), body).unwrap_or_abort();
}

#[test]
fn install_and_activate_valid_plugin_descriptor() {
    // arrange
    // act
    // assert
    // Given: workspace with a valid descriptor package
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_plugin_package(&workspace, "plugins/demo", "demo.plugin");
    let mut registry = PluginLifecycleRegistry::new(&workspace);

    // When: install then activate with permission granted
    let installed = registry
        .install_from_package_root(&package)
        .unwrap_or_abort();
    assert_eq!(installed.id, "demo.plugin");
    assert_eq!(installed.enablement, PluginEnablement::Disabled);
    assert!(!installed.manifest.runtime_effects().loads_external_code);

    let active = registry
        .activate("demo.plugin", PluginActivationPermission::Granted)
        .unwrap_or_abort();

    // Then: registered and enabled; still descriptor-only effects
    assert_eq!(active.enablement, PluginEnablement::Enabled);
    assert!(registry.is_enabled("demo.plugin"));
    assert_eq!(registry.len(), 1);
}

#[test]
fn plugin_lifecycle_summary_counts_enabled_and_disabled() {
    // arrange
    // act
    // assert
    // Given: two installed packages, one activated
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let enabled_pkg = write_plugin_package(&workspace, "plugins/on", "on.plugin");
    let disabled_pkg = write_plugin_package(&workspace, "plugins/off", "off.plugin");
    let mut registry = PluginLifecycleRegistry::new(&workspace);
    registry
        .install_from_package_root(&enabled_pkg)
        .unwrap_or_abort();
    registry
        .install_from_package_root(&disabled_pkg)
        .unwrap_or_abort();
    registry
        .activate("on.plugin", PluginActivationPermission::Granted)
        .unwrap_or_abort();

    // When
    let summary = registry.summary();

    // Then
    assert_eq!(
        summary,
        PluginLifecycleSummary {
            installed: 2,
            enabled: 1,
            disabled: 1,
        }
    );
    assert!(summary.has_enabled());
    assert!(summary.one_line().contains("2 installed"));
    assert!(summary.one_line().contains("1 enabled"));
    assert!(summary.one_line().contains("1 disabled"));
    assert_eq!(
        PluginLifecycleRegistry::new(&workspace).summary().installed,
        0
    );
}

#[test]
fn corrupt_descriptor_fails_with_no_leftover_registration() {
    // arrange
    // act
    // assert
    // Given: package with corrupt JSON / invalid schema
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = workspace.join("plugins/bad");
    fs::create_dir_all(&package).unwrap_or_abort();
    fs::write(
        package.join(PLUGIN_MANIFEST_FILE_NAME),
        r#"{"schemaVersion":"not-a-real-schema","id":"bad.plugin"}"#,
    )
    .unwrap_or_abort();
    let mut registry = PluginLifecycleRegistry::new(&workspace);

    // When
    let err = registry
        .install_from_package_root(&package)
        .expect_err("corrupt descriptor must fail");

    // Then: fail closed, registry empty
    assert!(matches!(err, PluginLifecycleError::ManifestInvalid { .. }));
    assert!(registry.is_empty());
    assert!(registry.get("bad.plugin").is_none());
}

#[test]
fn invalid_json_leaves_no_stale_registration_even_after_prior_install() {
    // Given: one valid plugin already installed
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let good = write_plugin_package(&workspace, "plugins/good", "good.plugin");
    let mut registry = PluginLifecycleRegistry::new(&workspace);
    registry.install_from_package_root(&good).unwrap_or_abort();

    let bad = workspace.join("plugins/bad");
    fs::create_dir_all(&bad).unwrap_or_abort();
    fs::write(bad.join(PLUGIN_MANIFEST_FILE_NAME), "{not-json").unwrap_or_abort();

    // When
    let err = registry
        .install_from_package_root(&bad)
        .expect_err("invalid json must fail");

    // Then: only the prior good registration remains
    assert!(matches!(err, PluginLifecycleError::ManifestInvalid { .. }));
    assert_eq!(registry.len(), 1);
    assert!(registry.get("good.plugin").is_some());
    assert!(registry.get("bad.plugin").is_none());
}

#[test]
fn activation_denied_without_permission_leaves_disabled() {
    // arrange
    // act
    // assert
    // Given: installed disabled plugin
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_plugin_package(&workspace, "plugins/demo", "demo.plugin");
    let mut registry = PluginLifecycleRegistry::new(&workspace);
    registry
        .install_from_package_root(&package)
        .unwrap_or_abort();

    // When
    let err = registry
        .activate("demo.plugin", PluginActivationPermission::Denied)
        .expect_err("denied permission must block activation");

    // Then
    assert_eq!(
        err,
        PluginLifecycleError::ActivationDenied {
            id: "demo.plugin".to_string()
        }
    );
    assert!(!registry.is_enabled("demo.plugin"));
}

#[test]
fn hooks_and_skills_only_package_activates_without_code_load() {
    // arrange — package with hooks.json + skills/ but no plugin_entry.json
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_plugin_package(&workspace, "plugins/tools-only", "tools.plugin");
    write_hooks_json(&package);
    write_skills_dir(&package);
    let mut registry = PluginLifecycleRegistry::new(&workspace);
    registry
        .install_from_package_root(&package)
        .unwrap_or_abort();

    // act
    let active = registry
        .activate("tools.plugin", PluginActivationPermission::Granted)
        .unwrap_or_abort();

    // assert — descriptor-only packages enable without any code-load claim
    assert_eq!(active.enablement, PluginEnablement::Enabled);
    assert!(!active.manifest.runtime_effects().loads_external_code);
    assert!(registry.is_enabled("tools.plugin"));
}

#[test]
fn deactivate_and_remove_lifecycle() {
    // arrange
    // act
    // assert
    // Given: installed + activated plugin
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_plugin_package(&workspace, "plugins/demo", "demo.plugin");
    let mut registry = PluginLifecycleRegistry::new(&workspace);
    registry
        .install_from_package_root(&package)
        .unwrap_or_abort();
    registry
        .activate("demo.plugin", PluginActivationPermission::Granted)
        .unwrap_or_abort();

    // When/Then: remove while enabled fails closed
    let remove_err = registry
        .remove("demo.plugin")
        .expect_err("remove while enabled must fail");
    assert!(matches!(
        remove_err,
        PluginLifecycleError::RemoveWhileEnabled { .. }
    ));
    assert_eq!(registry.len(), 1);

    // When: deactivate then remove
    registry.deactivate("demo.plugin").unwrap_or_abort();
    let removed = registry.remove("demo.plugin").unwrap_or_abort();

    // Then
    assert_eq!(removed.id, "demo.plugin");
    assert!(registry.is_empty());
}

#[test]
fn activate_loads_hooks_json_and_writes_receipt() {
    // arrange
    // act
    // assert
    // Given: package with descriptor + hooks.json
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_plugin_package(&workspace, "plugins/hooks", "hooks.plugin");
    write_hooks_json(&package);
    let mut registry = PluginLifecycleRegistry::new(&workspace);
    registry
        .install_from_package_root(&package)
        .unwrap_or_abort();

    // When
    let active = registry
        .activate("hooks.plugin", PluginActivationPermission::Granted)
        .unwrap_or_abort();

    // Then: loaded code + filesystem receipt
    assert!(active.loads_code());
    assert!(active.one_line().contains("loads_code=true"));
    let receipt = package.join(PLUGIN_LOAD_RECEIPT_FILE_NAME);
    assert!(
        receipt.is_file(),
        "expected load receipt at {}",
        receipt.display()
    );
    let receipt_raw = fs::read_to_string(&receipt).unwrap_or_abort();
    assert!(receipt_raw.contains("hooks.plugin"));
    assert!(receipt_raw.contains("hooks_json") || receipt_raw.contains("hooksJson"));
}

#[test]
fn full_lifecycle_install_activate_load_deactivate_remove() {
    // arrange
    // act
    // assert
    // Given: package with plugin_entry + skills
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_plugin_package(&workspace, "plugins/full", "full.plugin");
    write_skills_dir(&package);
    write_plugin_entry(&package, &[PLUGIN_SKILLS_DIR_NAME]);
    let mut registry = PluginLifecycleRegistry::new(&workspace);

    // When: install → activate (loads) → deactivate → remove
    registry
        .install_from_package_root(&package)
        .unwrap_or_abort();
    let active = registry
        .activate("full.plugin", PluginActivationPermission::Granted)
        .unwrap_or_abort();
    assert!(active.loads_code());
    let receipt = package.join(PLUGIN_LOAD_RECEIPT_FILE_NAME);
    assert!(receipt.is_file());

    registry.deactivate("full.plugin").unwrap_or_abort();
    assert!(!receipt.exists(), "deactivate must remove load receipt");
    let disabled = registry.get("full.plugin").expect("still registered");
    assert!(!disabled.loads_code());
    assert_eq!(disabled.enablement, PluginEnablement::Disabled);

    let removed = registry.remove("full.plugin").unwrap_or_abort();
    assert_eq!(removed.id, "full.plugin");
    assert!(registry.is_empty());
}

#[test]
fn activate_fails_closed_on_invalid_plugin_entry() {
    // arrange
    // act
    // assert
    // Given: package with corrupt plugin_entry.json
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_plugin_package(&workspace, "plugins/bad-entry", "bad.entry.plugin");
    fs::write(
        package.join(PLUGIN_ENTRY_FILE_NAME),
        r#"{"schemaVersion":"plugin.entry.v1","entrypoints":[]}"#,
    )
    .unwrap_or_abort();
    let mut registry = PluginLifecycleRegistry::new(&workspace);
    registry
        .install_from_package_root(&package)
        .unwrap_or_abort();

    // When
    let err = registry
        .activate("bad.entry.plugin", PluginActivationPermission::Granted)
        .expect_err("invalid entry must fail closed");

    // Then: remains disabled, no receipt
    assert!(matches!(
        err,
        PluginLifecycleError::PackageLoadFailed { .. }
    ));
    assert!(!registry.is_enabled("bad.entry.plugin"));
    assert!(!package.join(PLUGIN_LOAD_RECEIPT_FILE_NAME).exists());
}

#[test]
fn activate_fails_closed_when_declared_entrypoint_missing() {
    // arrange
    // act
    // assert
    // Given: plugin_entry declares skills but skills dir is absent
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_plugin_package(&workspace, "plugins/missing", "missing.entry.plugin");
    write_plugin_entry(&package, &[PLUGIN_SKILLS_DIR_NAME]);
    let mut registry = PluginLifecycleRegistry::new(&workspace);
    registry
        .install_from_package_root(&package)
        .unwrap_or_abort();

    // When
    let err = registry
        .activate("missing.entry.plugin", PluginActivationPermission::Granted)
        .expect_err("missing entrypoint must fail closed");

    // Then
    assert!(matches!(
        err,
        PluginLifecycleError::PackageLoadFailed { .. }
    ));
    assert!(!registry.is_enabled("missing.entry.plugin"));
    assert!(!package.join(PLUGIN_LOAD_RECEIPT_FILE_NAME).exists());
}

#[test]
fn activate_fails_closed_on_invalid_hooks_json() {
    // arrange
    // act
    // assert
    // Given
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_plugin_package(&workspace, "plugins/bad-hooks", "bad.hooks.plugin");
    fs::write(package.join(PLUGIN_HOOKS_FILE_NAME), "not-json").unwrap_or_abort();
    let mut registry = PluginLifecycleRegistry::new(&workspace);
    registry
        .install_from_package_root(&package)
        .unwrap_or_abort();

    // When
    let err = registry
        .activate("bad.hooks.plugin", PluginActivationPermission::Granted)
        .expect_err("invalid hooks must fail");

    // Then
    assert!(matches!(
        err,
        PluginLifecycleError::PackageLoadFailed { .. }
    ));
    assert!(!registry.is_enabled("bad.hooks.plugin"));
}

#[test]
fn package_path_outside_workspace_is_rejected() {
    // arrange
    // act
    // assert
    // Given: two sibling directories; registry rooted at one
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    let outside = temp.path().join("outside");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    fs::create_dir_all(&outside).unwrap_or_abort();
    fs::write(
        outside.join(PLUGIN_MANIFEST_FILE_NAME),
        valid_manifest_body("evil.plugin"),
    )
    .unwrap_or_abort();
    let mut registry = PluginLifecycleRegistry::new(&workspace);

    // When
    let err = registry
        .install_from_package_root(&outside)
        .expect_err("outside workspace must fail");

    // Then
    assert!(matches!(
        err,
        PluginLifecycleError::PathEscapesWorkspace { .. }
    ));
    assert!(registry.is_empty());
}

#[test]
fn relative_parent_traversal_is_rejected() {
    // arrange
    // act
    // assert
    // Given
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let mut registry = PluginLifecycleRegistry::new(&workspace);

    // When
    let err = registry
        .install_from_package_root(Path::new("../escape"))
        .expect_err("parent traversal must fail");

    // Then
    assert!(matches!(
        err,
        PluginLifecycleError::PathEscapesWorkspace { .. }
            | PluginLifecycleError::PackageRootNotDirectory { .. }
    ));
    assert!(registry.is_empty());
}

#[test]
fn durable_registry_persists_install_and_activate_across_reopen() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_plugin_package(&workspace, "plugins/demo", "demo.plugin");
    let journal = workspace.join(PLUGIN_REGISTRY_REL);

    // act — install + activate in one durable registry instance
    let mut registry = PluginLifecycleRegistry::open(&workspace).unwrap_or_abort();
    registry
        .install_from_package_root(&package)
        .unwrap_or_abort();
    registry
        .activate("demo.plugin", PluginActivationPermission::Granted)
        .unwrap_or_abort();
    drop(registry);

    // assert — journal written, and a fresh instance over the same workspace sees the
    // enabled plugin without re-installing
    assert!(journal.is_file(), "durable journal must persist");
    let reopened = PluginLifecycleRegistry::open(&workspace).unwrap_or_abort();
    assert_eq!(reopened.len(), 1);
    assert!(reopened.is_enabled("demo.plugin"));
    let plugin = reopened.get("demo.plugin").expect("restored plugin");
    assert_eq!(plugin.id, "demo.plugin");
    assert_eq!(plugin.enablement, PluginEnablement::Enabled);
    assert_eq!(plugin.manifest.id, "demo.plugin");
}

#[test]
fn durable_open_on_empty_workspace_reports_no_plugins_and_no_journal() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();

    // act
    let registry = PluginLifecycleRegistry::open(&workspace).unwrap_or_abort();

    // assert — empty registry and no journal until a mutation persists
    assert!(registry.is_empty());
    assert!(!workspace.join(PLUGIN_REGISTRY_REL).exists());
}

#[test]
fn durable_remove_persists_deletion_across_reopen() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_plugin_package(&workspace, "plugins/gone", "gone.plugin");

    // act — install durably, then remove in a separate (reopened) instance
    PluginLifecycleRegistry::open(&workspace)
        .unwrap_or_abort()
        .install_from_package_root(&package)
        .unwrap_or_abort();
    let mut reopened = PluginLifecycleRegistry::open(&workspace).unwrap_or_abort();
    assert_eq!(reopened.len(), 1);
    reopened.remove("gone.plugin").unwrap_or_abort();
    drop(reopened);

    // assert — a further reopen sees the deletion
    let after = PluginLifecycleRegistry::open(&workspace).unwrap_or_abort();
    assert!(after.is_empty());
    assert!(after.get("gone.plugin").is_none());
}

#[test]
fn durable_contract_upgrade_persists_across_reopen_and_preserves_enablement() {
    // arrange — a durable contract with an installed + activated package
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let v1 = write_plugin_package(&workspace, "plugins/du-v1", "du.plugin");
    let v2 = write_plugin_package(&workspace, "plugins/du-v2", "du.plugin");
    let journal = workspace.join(PLUGIN_REGISTRY_REL);
    let mut contract = PluginRuntimeContract::open(&workspace).unwrap_or_abort();
    contract.install_from_package_root(&v1).unwrap_or_abort();
    contract
        .activate("du.plugin", PluginActivationPermission::Granted)
        .unwrap_or_abort();

    // act — upgrade to the replacement package and drop the durable contract
    contract
        .upgrade_plugin("du.plugin", &v2, PluginActivationPermission::Granted)
        .unwrap_or_abort();
    drop(contract);

    // assert — a fresh durable open over the same workspace sees the upgraded, enabled plugin
    assert!(journal.is_file(), "durable journal must persist");
    let reopened = PluginLifecycleRegistry::open(&workspace).unwrap_or_abort();
    let plugin = reopened.get("du.plugin").expect("restored plugin");
    assert_eq!(plugin.enablement, PluginEnablement::Enabled);
    assert!(
        plugin.package_root.ends_with("plugins/du-v2"),
        "upgrade must persist the new package root: {}",
        plugin.package_root.display()
    );
    assert_eq!(plugin.manifest.id, "du.plugin");
}

#[test]
fn in_memory_new_registry_does_not_write_a_durable_journal() {
    // arrange — the coordinator/run path uses `new`, which must stay side-effect free
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_plugin_package(&workspace, "plugins/mem", "mem.plugin");

    // act
    let mut registry = PluginLifecycleRegistry::new(&workspace);
    registry
        .install_from_package_root(&package)
        .unwrap_or_abort();

    // assert — installed in memory but no journal file is written
    assert_eq!(registry.len(), 1);
    assert!(registry.registry_path().is_none());
    assert!(!workspace.join(PLUGIN_REGISTRY_REL).exists());
}
