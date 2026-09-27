use harness_core::integrations::*;
use serde_json::json;
use std::path::Path;

fn package(
    root: &Path,
    directory: &str,
    id: &str,
    version: &str,
) -> std::io::Result<std::path::PathBuf> {
    let path = root.join(directory);
    std::fs::create_dir(&path)?;
    std::fs::write(
        path.join("extension.manifest.json"),
        json!({"schemaVersion":"extension.manifest.v1", "id":id, "version":version}).to_string(),
    )?;
    std::fs::write(path.join("hooks.json"), r#"{"command":"touch unexpected"}"#)?;
    Ok(path)
}

#[test]
fn plugin_activation_and_upgrade_commit_only_after_permission_and_validation(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let mut runtime = PluginRuntimeContract::open(temp.path())?;
    assert_eq!(std::fs::read_dir(temp.path())?.count(), 0);
    let original = package(temp.path(), "original", "review.plugin", "1")?;
    let replacement = package(temp.path(), "replacement", "review.plugin", "2")?;
    let wrong = package(temp.path(), "wrong", "different.plugin", "2")?;
    runtime.install_from_package_root(&original)?;
    let receipt = original.join(PLUGIN_LOAD_RECEIPT_FILE_NAME);
    assert!(!receipt.exists());
    assert!(runtime
        .activate("review.plugin", PluginActivationPermission::Denied)
        .is_err());
    assert!(!receipt.exists());
    assert!(runtime
        .activate("review.plugin", PluginActivationPermission::Granted)?
        .loads_code());
    assert!(receipt.is_file());
    assert!(!temp.path().join("unexpected").exists());
    let registry_path = runtime
        .registry()
        .registry_path()
        .ok_or("missing registry path")?
        .to_owned();
    let before = std::fs::read(&registry_path)?;
    let receipt_before = std::fs::read(&receipt)?;
    let events_before = runtime.events().len();
    assert!(runtime
        .upgrade_plugin("review.plugin", &wrong, PluginActivationPermission::Granted)
        .is_err());
    assert_eq!(
        (
            std::fs::read(&registry_path)?,
            std::fs::read(&receipt)?,
            runtime.events().len()
        ),
        (before.clone(), receipt_before.clone(), events_before)
    );
    assert!(runtime
        .upgrade_plugin(
            "review.plugin",
            &replacement,
            PluginActivationPermission::Denied
        )
        .is_err());
    std::fs::create_dir(replacement.join(PLUGIN_LOAD_RECEIPT_FILE_NAME))?;
    assert!(runtime
        .upgrade_plugin(
            "review.plugin",
            &replacement,
            PluginActivationPermission::Granted
        )
        .is_err());
    assert_eq!(
        (std::fs::read(&registry_path)?, std::fs::read(&receipt)?),
        (before, receipt_before)
    );
    std::fs::remove_dir(replacement.join(PLUGIN_LOAD_RECEIPT_FILE_NAME))?;
    runtime.upgrade_plugin(
        "review.plugin",
        &replacement,
        PluginActivationPermission::Granted,
    )?;
    assert!(!receipt.exists());
    assert!(replacement.join(PLUGIN_LOAD_RECEIPT_FILE_NAME).is_file());
    assert_eq!(
        runtime
            .get("review.plugin")
            .ok_or("missing plugin")?
            .manifest
            .version
            .as_deref(),
        Some("2")
    );
    let mut reopened = PluginRuntimeContract::open(temp.path())?;
    assert!(reopened.is_enabled("review.plugin"));
    assert!(reopened.remove("review.plugin").is_err());
    reopened.deactivate("review.plugin")?;
    assert!(!replacement.join(PLUGIN_LOAD_RECEIPT_FILE_NAME).exists());
    reopened.remove("review.plugin")?;
    assert!(PluginRuntimeContract::open(temp.path())?.is_empty());
    Ok(())
}

#[test]
fn plugin_storage_rejects_escaping_entries_and_preserves_other_writers(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let external = tempfile::tempdir()?;
    let alpha = package(temp.path(), "alpha", "alpha.plugin", "1")?;
    let beta = package(temp.path(), "beta", "beta.plugin", "1")?;
    let outside = package(external.path(), "outside", "outside.plugin", "1")?;
    let mut first = PluginLifecycleRegistry::open(temp.path())?;
    let mut second = PluginLifecycleRegistry::open(temp.path())?;
    assert!(first.install_from_package_root(outside).is_err());
    first.install_from_package_root(&alpha)?;
    second.install_from_package_root(&beta)?;
    assert_eq!(PluginLifecycleRegistry::open(temp.path())?.len(), 2);
    std::fs::write(
        alpha.join(PLUGIN_ENTRY_FILE_NAME),
        r#"{"schemaVersion":"plugin.entry.v1","entrypoints":["../beta/hooks.json"]}"#,
    )?;
    assert!(first
        .activate("alpha.plugin", PluginActivationPermission::Granted)
        .is_err());
    assert!(!alpha.join(PLUGIN_LOAD_RECEIPT_FILE_NAME).exists());
    std::fs::remove_file(alpha.join(PLUGIN_ENTRY_FILE_NAME))?;
    #[cfg(unix)]
    {
        std::fs::remove_file(alpha.join(PLUGIN_HOOKS_FILE_NAME))?;
        std::os::unix::fs::symlink(
            beta.join(PLUGIN_HOOKS_FILE_NAME),
            alpha.join(PLUGIN_HOOKS_FILE_NAME),
        )?;
        assert!(first
            .activate("alpha.plugin", PluginActivationPermission::Granted)
            .is_err());
        std::fs::remove_file(alpha.join(PLUGIN_HOOKS_FILE_NAME))?;
        std::fs::write(alpha.join(PLUGIN_HOOKS_FILE_NAME), "{}")?;
        let skills = alpha.join(PLUGIN_SKILLS_DIR_NAME);
        let nested = skills.join("a/b/c");
        std::fs::create_dir_all(&nested)?;
        std::fs::write(skills.join("SKILL.md"), "Review changes.")?;
        std::os::unix::fs::symlink(
            beta.join(PLUGIN_HOOKS_FILE_NAME),
            nested.join("escape.json"),
        )?;
        assert!(first
            .activate("alpha.plugin", PluginActivationPermission::Granted)
            .is_err());
        assert!(!alpha.join(PLUGIN_LOAD_RECEIPT_FILE_NAME).exists());
        std::fs::remove_dir_all(skills)?;
    }
    let registry_path = first.registry_path().ok_or("missing registry")?.to_owned();
    std::fs::write(&registry_path, "{corrupt")?;
    assert!(first
        .activate("alpha.plugin", PluginActivationPermission::Granted)
        .is_err());
    assert!(!first.is_enabled("alpha.plugin"));
    assert!(!alpha.join(PLUGIN_LOAD_RECEIPT_FILE_NAME).exists());
    assert_eq!(std::fs::read_to_string(registry_path)?, "{corrupt");
    Ok(())
}
