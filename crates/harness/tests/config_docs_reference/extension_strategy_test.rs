use harness::UnwrapOrAbort;
use harness_core::extension_manifest::EXTENSION_MANIFEST_V1_SCHEMA_VERSION;
use serde_json::Value;

fn hook_lifecycle_event_names_from_source() -> Vec<String> {
    let source = read_doc("crates/harness-core/src/config.rs");
    let section = source
        .split("impl HookLifecycleEvent")
        .nth(1)
        .unwrap_or_abort();
    let events = section
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if !trimmed.starts_with("Self::") {
                return None;
            }
            let (_variant, value) = trimmed.split_once("=>")?;
            value.split('"').nth(1).map(str::to_string)
        })
        .collect::<Vec<_>>();
    assert!(
        !events.is_empty(),
        "HookLifecycleEvent source parser should find event strings"
    );
    events
}

#[test]
fn extension_strategy_documents_command_hook_lifecycle_map() {
    // arrange
    let extension = read_doc("docs/operations/extension-strategy.md");
    let lifecycle_section = extension
        .split("### Lifecycle phase map\n")
        .nth(1)
        .unwrap_or_abort()
        .split("\n## ")
        .next()
        .unwrap_or_abort();
    let rows = markdown_table_rows(lifecycle_section)
        .into_iter()
        .filter(|row| {
            row.first()
                .is_none_or(|cell| cell != "Hook lifecycle event")
        })
        .collect::<Vec<_>>();
    let valid_statuses = BTreeSet::from([
        "native".to_string(),
        "fallback".to_string(),
        "intentionally_unsupported".to_string(),
        "post_v1".to_string(),
    ]);

    // act
    let documented_events = hook_lifecycle_event_names_from_source();

    // assert: command stance and replay/permission boundaries are explicit.
    for anchor in [
        "TUI slash commands are built-in UI actions",
        "V1 does not execute markdown command\nfiles, substitute `$ARGUMENTS`",
        "interpolate commands",
        "or run during replay",
        "do not append events directly, schedule\ntasks directly, register tools, resolve permissions, or run during replay",
        "Critical hook failure fails closed at the coordinator boundary",
        "Noncritical hook failure records metadata",
        "Deterministic/replay modes suppress live hook\nexecution while preserving hook metadata",
    ] {
        assert!(
            extension.contains(anchor),
            "extension strategy missing command/hook stance anchor: {anchor}"
        );
    }

    // assert: every row has an allowed status label.
    assert!(
        rows.iter()
            .any(|row| row.get(1) == Some(&"intentionally_unsupported".to_string())),
        "lifecycle map must label intentionally unsupported command/context seams"
    );
    assert!(
        rows.iter()
            .any(|row| row.get(1) == Some(&"post_v1".to_string())),
        "lifecycle map must label post-V1 hook/plugin seams"
    );
    for row in &rows {
        let status = row
            .get(1)
            .unwrap_or_else(|| panic!("lifecycle row missing status: {row:?}"));
        assert!(
            valid_statuses.contains(status),
            "lifecycle row has invalid status `{status}`: {row:?}"
        );
    }

    // assert: current config enum values cannot drift away from docs.
    for event in documented_events {
        let event_cell = format!("`{event}`");
        let row = rows
            .iter()
            .find(|row| row.first().is_some_and(|cell| cell == &event_cell))
            .unwrap_or_else(|| panic!("lifecycle map missing HookLifecycleEvent `{event}`"));
        assert_eq!(
            row.get(1).map(String::as_str),
            Some("native"),
            "current HookLifecycleEvent `{event}` should be documented as native"
        );
    }
}

#[test]
fn extension_strategy_documents_descriptor_only_manifest_seam() {
    // arrange
    let root = repo_root();
    let extension = read_doc("docs/operations/extension-strategy.md");
    let config = read_doc("docs/configuration/config.md");
    let sessions = read_doc("docs/architecture/sessions-and-replay.md");
    let schema_path = root.join("configs/extension-manifest.v1.schema.json");

    // act
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(&schema_path).unwrap_or_abort(),
    )
    .unwrap_or_abort();

    // assert: docs describe descriptor-only scope and no runtime host behavior.
    for anchor in [
        "A typed extension manifest describes capabilities but does not execute\nplugins",
        "schemaVersion: \"extension.manifest.v1\"",
        "does not discover manifests at runtime,\nregister tools, execute commands, launch MCP servers, invoke provider\ndecorators, load external code, or mutate sessions",
        "Replay support is static metadata rendering",
        "Any future extension-provided\nbehavior must enter through the existing native registry",
    ] {
        assert!(
            extension.contains(anchor),
            "extension strategy missing descriptor-only manifest anchor: {anchor}"
        );
    }
    for anchor in [
        "Typed extension manifests are not a runtime config key in V1",
        "does not discover manifests\nfrom config, register tools, execute commands, launch MCP servers, invoke\nprovider decorators, load external code, or mutate sessions",
    ] {
        assert!(
            config.contains(anchor),
            "config docs missing descriptor-only manifest anchor: {anchor}"
        );
    }
    assert!(
        sessions
            .contains("Replay never discovers manifests, loads extension code, registers tools"),
        "sessions/replay docs must forbid manifest code loading during replay"
    );
    // assert: checked-in schema is the V1 descriptor schema and covers every class.
    assert_eq!(
        schema["title"], "ExtensionManifestV1",
        "unexpected extension manifest schema title"
    );
    let definitions = &schema["definitions"];
    assert!(definitions.get("ExtensionToolDescriptor").is_some());
    assert!(definitions.get("ExtensionHookDescriptor").is_some());
    assert!(definitions.get("ExtensionCommandDescriptor").is_some());
    assert!(definitions.get("ExtensionPromptDescriptor").is_some());
    assert!(definitions.get("ExtensionMcpBundleDescriptor").is_some());
    assert!(definitions.get("ExtensionDiagnosticDescriptor").is_some());
    assert!(definitions
        .get("ExtensionProviderDecoratorDescriptor")
        .is_some());
    assert!(
        schema_path.is_file(),
        "extension manifest schema file should be checked in"
    );
    assert!(
        extension.contains(EXTENSION_MANIFEST_V1_SCHEMA_VERSION),
        "extension strategy should name the schema version"
    );
}
