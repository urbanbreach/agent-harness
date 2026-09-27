use harness_core::extension_manifest::*;
use serde_json::{json, Value};

#[test]
fn extension_discovery_validates_references_and_remains_descriptor_only(
) -> Result<(), Box<dyn std::error::Error>> {
    let valid = json!({
        "schemaVersion":"extension.manifest.v1", "id":"review.helpers",
        "capabilities":[{"id":"review", "defaultEnabled":false}],
        "tools":[{"id":"review.file", "capabilityId":"review", "permission":"edit"}],
        "hooks":[{"id":"review.done", "capabilityId":"review", "lifecycleEvent":"run_finished", "status":"intentionally_unsupported"}],
        "replay":{"label":"Review", "summaryTemplate":"Review descriptors"}
    });
    let manifest = ExtensionManifestV1::parse_json(&valid.to_string())?;
    assert_eq!(
        manifest.replay_metadata().disabled_capability_ids,
        ["review"]
    );
    assert_eq!(
        manifest.runtime_effects(),
        ExtensionManifestRuntimeEffects::descriptor_only()
    );
    let mut cases = Vec::new();
    for (path, value) in [
        ("/tools/0/capabilityId", json!("missing")),
        ("/hooks/0/lifecycleEvent", json!("arbitrary_hook")),
        ("/id", json!("../outside")),
        ("/replay/summaryTemplate", json!("$(touch surprise)")),
        ("/schemaVersion", json!("extension.manifest.v2")),
    ] {
        let mut invalid = valid.clone();
        *invalid.pointer_mut(path).ok_or("invalid test pointer")? = value;
        cases.push(invalid);
    }
    let mut duplicate = valid.clone();
    let first = duplicate["tools"][0].clone();
    duplicate["tools"]
        .as_array_mut()
        .ok_or("tools missing")?
        .push(first);
    cases.push(duplicate);
    let mut executable = valid.clone();
    executable["command"] = json!(["sh", "-c", "touch surprise"]);
    cases.push(executable);
    for invalid in cases {
        assert!(ExtensionManifestV1::parse_json(&invalid.to_string()).is_err());
    }
    assert!(ExtensionManifestV1::parse_json(
        r#"{"id":"x","id":"y","schemaVersion":"extension.manifest.v1"}"#
    )
    .is_err());
    let temp = tempfile::tempdir()?;
    assert!(discover_extension_manifests(temp.path()).is_empty());
    assert_eq!(std::fs::read_dir(temp.path())?.count(), 0);
    let path = temp.path().join(EXTENSION_MANIFEST_FILE_NAME);
    std::fs::write(&path, valid.to_string())?;
    let summary = discover_extension_manifests(temp.path());
    assert_eq!(summary.len(), 1);
    assert!(!summary[0].loads_external_code);
    #[cfg(unix)]
    {
        let linked = temp.path().join("linked");
        std::os::unix::fs::symlink(temp.path(), &linked)?;
        assert!(
            load_extension_manifest_from_path(&linked.join(EXTENSION_MANIFEST_FILE_NAME)).is_err()
        );
        assert_eq!(discover_extension_manifests(temp.path()).len(), 1);
    }
    assert_eq!(
        serde_json::from_str::<Value>(&std::fs::read_to_string(path)?)?,
        valid
    );
    assert!(!temp.path().join("surprise").exists());
    use harness_core::extension_registry::ExtensionDescriptorRegistry;
    let mut first = ExtensionDescriptorRegistry::open(temp.path())?;
    let mut second = ExtensionDescriptorRegistry::open(temp.path())?;
    first.register_manifest_path(EXTENSION_MANIFEST_FILE_NAME)?;
    let other = temp.path().join("other");
    std::fs::create_dir(&other)?;
    let mut other_manifest = valid.clone();
    other_manifest["id"] = json!("other.helpers");
    std::fs::write(
        other.join(EXTENSION_MANIFEST_FILE_NAME),
        other_manifest.to_string(),
    )?;
    second.register_manifest_path(other.join(EXTENSION_MANIFEST_FILE_NAME))?;
    let reopened = ExtensionDescriptorRegistry::open(temp.path())?;
    assert_eq!(reopened.summary().registered, 2);
    let before = std::fs::read(reopened.registry_path())?;
    let external = tempfile::tempdir()?;
    std::fs::write(
        external.path().join(EXTENSION_MANIFEST_FILE_NAME),
        valid.to_string(),
    )?;
    assert!(first.discover_and_register(external.path()).is_err());
    assert_eq!(std::fs::read(reopened.registry_path())?, before);
    let mut corrupt = serde_json::from_slice::<Value>(&before)?;
    corrupt["entries"]["review.helpers"]["manifestPath"] = json!("../outside.json");
    std::fs::write(reopened.registry_path(), corrupt.to_string())?;
    assert!(ExtensionDescriptorRegistry::open(temp.path()).is_err());
    let mut corrupt = serde_json::from_slice::<Value>(&before)?;
    corrupt["entries"]["review.helpers"]["tools"] = json!(u64::MAX);
    std::fs::write(reopened.registry_path(), corrupt.to_string())?;
    assert!(ExtensionDescriptorRegistry::open(temp.path()).is_err());
    Ok(())
}
