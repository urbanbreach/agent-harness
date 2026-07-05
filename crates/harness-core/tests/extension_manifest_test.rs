use harness_core::extension_manifest::{
    ExtensionManifestError, ExtensionManifestRuntimeEffects, ExtensionManifestV1,
    EXTENSION_MANIFEST_V1_SCHEMA_VERSION,
};
use schemars::generate::SchemaSettings;
use serde_json::{json, Value};

fn valid_manifest_json() -> String {
    json!({
        "schemaVersion": EXTENSION_MANIFEST_V1_SCHEMA_VERSION,
        "id": "demo.extension",
        "displayName": "Demo extension descriptor",
        "version": "0.1.0",
        "capabilities": [
            {"id": "cap.tooling", "defaultEnabled": true, "description": "Tool descriptor", "replayLabel": "Tooling descriptor"},
            {"id": "cap.disabled", "defaultEnabled": false, "description": "Disabled by default"},
            {"id": "cap.prompt", "defaultEnabled": true},
            {"id": "cap.mcp", "defaultEnabled": true},
            {"id": "cap.diag", "defaultEnabled": true},
            {"id": "cap.provider", "defaultEnabled": true}
        ],
        "tools": [
            {"id": "tool.structural-rewrite", "capabilityId": "cap.tooling", "permission": "edit", "description": "Descriptor only", "replayLabel": "structural rewrite descriptor"}
        ],
        "hooks": [
            {"id": "hook.permission", "capabilityId": "cap.tooling", "lifecycleEvent": "permission_requested", "status": "native", "replayLabel": "permission hook descriptor"}
        ],
        "commands": [
            {"id": "command.cleanup", "capabilityId": "cap.tooling", "status": "intentionally_unsupported", "replayLabel": "cleanup command descriptor"}
        ],
        "prompts": [
            {"id": "prompt.review", "capabilityId": "cap.prompt", "replayLabel": "review prompt descriptor"}
        ],
        "mcpBundles": [
            {"id": "mcp.local", "capabilityId": "cap.mcp", "status": "post_v1", "serverIds": ["local-docs"], "replayLabel": "local MCP descriptor"}
        ],
        "diagnostics": [
            {"id": "diagnostic.health", "capabilityId": "cap.diag", "replayLabel": "health diagnostic descriptor"}
        ],
        "providerDecorators": [
            {"id": "provider.params", "capabilityId": "cap.provider", "status": "post_v1", "providerScope": "openai", "replayLabel": "provider decorator descriptor"}
        ],
        "replay": {
            "label": "Demo extension descriptor",
            "summaryTemplate": "Static descriptor metadata only"
        }
    })
    .to_string()
}

#[test]
fn extension_manifest_parses_descriptor_only_surface_and_replay_metadata() {
    // arrange
    let input = valid_manifest_json();

    // act
    let manifest = ExtensionManifestV1::parse_json(&input).expect("valid manifest");

    // assert
    assert_eq!(manifest.id, "demo.extension");
    assert_eq!(manifest.tools[0].permission.public_name(), "edit");
    assert_eq!(
        manifest.runtime_effects(),
        ExtensionManifestRuntimeEffects {
            registers_tools: false,
            executes_commands: false,
            launches_mcp: false,
            invokes_provider_decorators: false,
            loads_external_code: false,
            mutates_sessions: false,
        }
    );

    let replay = manifest.replay_metadata();
    assert_eq!(replay.extension_id, "demo.extension");
    assert_eq!(replay.disabled_capability_ids, vec!["cap.disabled"]);
    assert_eq!(replay.tool_descriptor_count, 1);
    assert_eq!(replay.hook_descriptor_count, 1);
    assert_eq!(replay.command_descriptor_count, 1);
    assert_eq!(replay.prompt_descriptor_count, 1);
    assert_eq!(replay.mcp_bundle_descriptor_count, 1);
    assert_eq!(replay.diagnostic_descriptor_count, 1);
    assert_eq!(replay.provider_decorator_descriptor_count, 1);
    assert_eq!(
        serde_json::to_value(replay).expect("replay json")["replayLabel"],
        "Demo extension descriptor"
    );
}

#[test]
fn extension_manifest_rejects_host_behavior_and_ambiguous_descriptors() {
    // arrange
    let executable_tool = json!({
        "schemaVersion": EXTENSION_MANIFEST_V1_SCHEMA_VERSION,
        "id": "demo.extension",
        "capabilities": [{"id": "cap.tooling"}],
        "tools": [{
            "id": "tool.exec",
            "capabilityId": "cap.tooling",
            "permission": "bash",
            "command": ["sh", "-c", "echo not-v1"]
        }]
    })
    .to_string();

    // act/assert
    let err = ExtensionManifestV1::parse_json(&executable_tool)
        .expect_err("unknown executable fields are rejected");
    assert!(err.to_string().contains("unknown field `command`"));

    let duplicate_capability = json!({
        "schemaVersion": EXTENSION_MANIFEST_V1_SCHEMA_VERSION,
        "id": "demo.extension",
        "capabilities": [{"id": "cap.tooling"}, {"id": "cap.tooling"}]
    })
    .to_string();
    assert_eq!(
        ExtensionManifestV1::parse_json(&duplicate_capability).expect_err("duplicate cap"),
        ExtensionManifestError::DuplicateCapabilityId("cap.tooling".to_string())
    );

    let unknown_capability = json!({
        "schemaVersion": EXTENSION_MANIFEST_V1_SCHEMA_VERSION,
        "id": "demo.extension",
        "capabilities": [{"id": "cap.tooling"}],
        "tools": [{"id": "tool.edit", "capabilityId": "cap.missing", "permission": "edit"}]
    })
    .to_string();
    assert!(matches!(
        ExtensionManifestV1::parse_json(&unknown_capability).expect_err("unknown cap"),
        ExtensionManifestError::UnknownCapabilityRef { .. }
    ));

    let unknown_hook = json!({
        "schemaVersion": EXTENSION_MANIFEST_V1_SCHEMA_VERSION,
        "id": "demo.extension",
        "capabilities": [{"id": "cap.hook"}],
        "hooks": [{"id": "hook.future", "capabilityId": "cap.hook", "lifecycleEvent": "session_idle", "status": "post_v1"}]
    })
    .to_string();
    assert!(matches!(
        ExtensionManifestV1::parse_json(&unknown_hook).expect_err("unknown hook"),
        ExtensionManifestError::UnknownHookLifecycle { .. }
    ));

    let dynamic_replay = json!({
        "schemaVersion": EXTENSION_MANIFEST_V1_SCHEMA_VERSION,
        "id": "demo.extension",
        "capabilities": [{"id": "cap.tooling"}],
        "replay": {"label": "$(whoami)", "summaryTemplate": "static"}
    })
    .to_string();

    // assert
    assert!(matches!(
        ExtensionManifestV1::parse_json(&dynamic_replay).expect_err("dynamic replay"),
        ExtensionManifestError::DynamicReplayText { .. }
    ));
}

#[test]
fn extension_manifest_schema_file_matches_generated_descriptor_schema() {
    // arrange
    let schema_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../configs/extension-manifest.v1.schema.json"
    );

    // act
    let checked_in: Value = serde_json::from_str(
        &std::fs::read_to_string(schema_path).expect("read extension manifest schema"),
    )
    .expect("schema json");
    let generated = serde_json::to_value(
        SchemaSettings::draft07()
            .into_generator()
            .into_root_schema_for::<ExtensionManifestV1>(),
    )
    .expect("schema value");

    // assert
    assert_eq!(checked_in, generated);
}
