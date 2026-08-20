// ---------------------------------------------------------------------------
// Plugins family
// ---------------------------------------------------------------------------

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

fn write_plugin_package(workspace: &Path, dir_name: &str, manifest_id: &str) -> std::path::PathBuf {
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

#[test]
fn plugins_boundary_e2e_install_activate_deactivate_remove_succeeds() {
    // arrange — workspace with a valid descriptor package
    let temp = tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_plugin_package(&workspace, "plugins/demo", "demo.plugin");
    let mut registry = PluginLifecycleRegistry::new(&workspace);

    // act — full lifecycle is executed
    registry
        .install_from_package_root(&package)
        .unwrap_or_abort();
    {
        let active = registry
            .activate("demo.plugin", PluginActivationPermission::Granted)
            .unwrap_or_abort();
        assert_eq!(active.enablement, PluginEnablement::Enabled);
    }
    registry.deactivate("demo.plugin").unwrap_or_abort();
    let removed = registry.remove("demo.plugin").unwrap_or_abort();

    // assert — each step succeeds and the plugin is removed
    assert_eq!(removed.id, "demo.plugin");
    assert!(registry.is_empty());
}

#[test]
fn plugins_bad_input_corrupt_descriptor_fails_without_stale_registration() {
    // arrange — package with corrupt JSON
    let temp = tempdir().unwrap_or_abort();
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

    // act — install is attempted
    let err = registry
        .install_from_package_root(&package)
        .expect_err("corrupt descriptor must fail");

    // assert — fail closed, registry empty
    assert!(matches!(err, PluginLifecycleError::ManifestInvalid { .. }));
    assert!(registry.is_empty());
}

#[test]
fn plugins_permission_denial_activation_without_permission_leaves_disabled() {
    // arrange — installed disabled plugin
    let temp = tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_plugin_package(&workspace, "plugins/demo", "demo.plugin");
    let mut registry = PluginLifecycleRegistry::new(&workspace);
    registry
        .install_from_package_root(&package)
        .unwrap_or_abort();

    // act — activation is attempted with denied permission
    let err = registry
        .activate("demo.plugin", PluginActivationPermission::Denied)
        .expect_err("denied permission must block activation");

    // assert — plugin remains disabled
    assert_eq!(
        err,
        PluginLifecycleError::ActivationDenied {
            id: "demo.plugin".to_string()
        }
    );
    assert!(!registry.is_enabled("demo.plugin"));
}

#[test]
fn plugins_process_failure_invalid_plugin_entry_fails_closed() {
    // arrange — package with corrupt plugin_entry.json
    let temp = tempdir().unwrap_or_abort();
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

    // act — activation is attempted
    let err = registry
        .activate("bad.entry.plugin", PluginActivationPermission::Granted)
        .expect_err("invalid entry must fail closed");

    // assert — remains disabled, no receipt
    assert!(matches!(
        err,
        PluginLifecycleError::PackageLoadFailed { .. }
    ));
    assert!(!registry.is_enabled("bad.entry.plugin"));
    assert!(!package.join(PLUGIN_LOAD_RECEIPT_FILE_NAME).exists());
}

#[test]
fn plugins_cancellation_restart_deactivate_then_reactivate_recovers() {
    // arrange — installed + activated plugin with hooks
    let temp = tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_plugin_package(&workspace, "plugins/hooks", "hooks.plugin");
    write_hooks_json(&package);
    let mut registry = PluginLifecycleRegistry::new(&workspace);
    registry
        .install_from_package_root(&package)
        .unwrap_or_abort();
    registry
        .activate("hooks.plugin", PluginActivationPermission::Granted)
        .unwrap_or_abort();
    assert!(registry.is_enabled("hooks.plugin"));
    let receipt = package.join(PLUGIN_LOAD_RECEIPT_FILE_NAME);
    assert!(receipt.is_file());

    // act — deactivate (cancel) then reactivate (restart)
    registry.deactivate("hooks.plugin").unwrap_or_abort();
    assert!(!registry.is_enabled("hooks.plugin"));
    assert!(!receipt.exists(), "deactivate must remove load receipt");

    registry
        .activate("hooks.plugin", PluginActivationPermission::Granted)
        .unwrap_or_abort();

    // assert — plugin is re-enabled and receipt is recreated
    assert!(registry.is_enabled("hooks.plugin"));
    assert!(receipt.is_file(), "reactivate must recreate load receipt");
}

#[test]
fn plugins_redaction_load_receipt_does_not_contain_secret_env_values() {
    // arrange — a plugin package with a manifest containing a secret-like value in env
    let temp = tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = workspace.join("plugins/secret");
    fs::create_dir_all(&package).unwrap_or_abort();
    let manifest = json!({
        "schemaVersion": EXTENSION_MANIFEST_V1_SCHEMA_VERSION,
        "id": "secret.plugin",
        "displayName": "Secret plugin",
        "version": "0.1.0",
        "capabilities": [
            {"id": "cap.demo", "defaultEnabled": true}
        ]
    });
    fs::write(
        package.join(PLUGIN_MANIFEST_FILE_NAME),
        manifest.to_string(),
    )
    .unwrap_or_abort();
    write_hooks_json(&package);
    let mut registry = PluginLifecycleRegistry::new(&workspace);
    registry
        .install_from_package_root(&package)
        .unwrap_or_abort();

    // act — the plugin is activated (writes a load receipt)
    registry
        .activate("secret.plugin", PluginActivationPermission::Granted)
        .unwrap_or_abort();

    // assert — the load receipt does not contain raw secret material
    let receipt = package.join(PLUGIN_LOAD_RECEIPT_FILE_NAME);
    assert!(receipt.is_file(), "load receipt must exist");
    let receipt_raw = fs::read_to_string(&receipt).unwrap_or_abort();
    assert!(
        !receipt_raw.contains("sk-AbCdEf") && !receipt_raw.contains("Bearer "),
        "load receipt must not contain raw secrets: {receipt_raw}"
    );
    assert!(receipt_raw.contains("secret.plugin"));
}

// ---------------------------------------------------------------------------
// Durable registry redaction
// ---------------------------------------------------------------------------

#[test]
fn plugins_durable_registry_journal_does_not_contain_secret_material() {
    // arrange — a workspace with a durable registry journal
    let temp = tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_plugin_package(&workspace, "plugins/demo", "demo.plugin");
    let mut registry = PluginLifecycleRegistry::open(&workspace).unwrap_or_abort();
    registry
        .install_from_package_root(&package)
        .unwrap_or_abort();
    registry
        .activate("demo.plugin", PluginActivationPermission::Granted)
        .unwrap_or_abort();
    drop(registry);

    // act — the journal is read
    let journal_path = workspace.join(PLUGIN_REGISTRY_REL);
    assert!(journal_path.is_file(), "journal must exist");
    let journal_raw = fs::read_to_string(&journal_path).unwrap_or_abort();

    // assert — the journal does not contain secret-like patterns
    assert!(
        !journal_raw.contains("Bearer "),
        "journal must not contain bearer tokens"
    );
    assert!(
        !journal_raw.contains("sk-AbCdEf"),
        "journal must not contain API keys"
    );
    assert!(
        journal_raw.contains("demo.plugin"),
        "journal must contain plugin id"
    );
}
