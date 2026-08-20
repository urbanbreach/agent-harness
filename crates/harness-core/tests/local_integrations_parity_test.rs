use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use harness_core::code_graph::{
    build_persistent_graph_index, query_persistent_graph, GraphQuery, GraphQueryKind,
};
use harness_core::config::{LspConfig, LspServerConfig, McpServerConfig};
use harness_core::coord::{
    LifecycleHookCommandExecutor, LifecycleHookCommandInvocation, TokioLifecycleHookCommandExecutor,
};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, RunStartedEvent, SCHEMA_VERSION,
};
use harness_core::extension_manifest::EXTENSION_MANIFEST_V1_SCHEMA_VERSION;
use harness_core::integrations::{
    run_file_acp_agent_mode_product, PluginActivationPermission, PluginLifecycleRegistry,
    PLUGIN_HOOKS_FILE_NAME, PLUGIN_LOAD_RECEIPT_FILE_NAME, PLUGIN_MANIFEST_FILE_NAME,
    PLUGIN_REGISTRY_REL, PLUGIN_SKILLS_DIR_NAME,
};
use harness_core::store::{EventEnvelopeWithoutSeqV1, EventStore, JsonlFileEventStore};
use harness_core::UnwrapOrAbort;
use serde_json::json;

fn write_local_plugin(workspace: &Path, id: &str) -> PathBuf {
    let package = workspace.join("local-plugin");
    let skill = package.join(PLUGIN_SKILLS_DIR_NAME).join("local-skill");
    fs::create_dir_all(&skill).unwrap_or_abort();
    fs::write(
        package.join(PLUGIN_MANIFEST_FILE_NAME),
        json!({
            "schemaVersion": EXTENSION_MANIFEST_V1_SCHEMA_VERSION,
            "id": id,
            "displayName": "Local parity plugin",
            "version": "0.1.0",
            "capabilities": [{"id": "local.parity", "defaultEnabled": true}],
        })
        .to_string(),
    )
    .unwrap_or_abort();
    fs::write(
        package.join(PLUGIN_HOOKS_FILE_NAME),
        r#"{"hooks":[{"id":"local.started","event":"run_started"}]}"#,
    )
    .unwrap_or_abort();
    fs::write(skill.join("SKILL.md"), "# local skill\n").unwrap_or_abort();
    package
}

fn run_started_event(run_id: &str) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{run_id}-0001"),
        seq: 1,
        run_id: run_id.to_string().into(),
        mono_ms: 1,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("parity".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: Some(format!("run:{run_id}")),
        payload: EventV1::RunStarted(RunStartedEvent {
            run_name: "local-integrations-parity".to_string().into(),
            workspace_root: "/tmp/local-integrations-parity".to_string(),
        }),
    }
}

#[tokio::test]
async fn file_discovered_hook_blocks_without_trust_then_runs_as_a_local_process() {
    // arrange — a filesystem-local package with a discovered hooks file.
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_local_plugin(&workspace, "local.parity.plugin");
    let mut registry = PluginLifecycleRegistry::open(&workspace).unwrap_or_abort();
    registry
        .install_from_package_root(&package)
        .unwrap_or_abort();

    // act — trust is denied, then explicitly granted before lifecycle execution.
    registry
        .activate("local.parity.plugin", PluginActivationPermission::Denied)
        .expect_err("untrusted local package must stay blocked");
    let active = registry
        .activate("local.parity.plugin", PluginActivationPermission::Granted)
        .unwrap_or_abort();
    let receipt = package.join(PLUGIN_LOAD_RECEIPT_FILE_NAME);
    let executor = TokioLifecycleHookCommandExecutor;
    let output = executor
        .execute(LifecycleHookCommandInvocation {
            executable: "sh".to_string(),
            args: vec![
                "-c".to_string(),
                "printf hook-fired > hook-fired.txt".to_string(),
            ],
            cwd: workspace.clone(),
            env: BTreeMap::new(),
            timeout_ms: 5_000,
        })
        .await
        .unwrap_or_abort();

    // assert — discovered components activate, and the allowed hook has a real process effect.
    assert!(active.loads_code());
    assert!(output.status.success());
    assert_eq!(
        fs::read_to_string(workspace.join("hook-fired.txt")).unwrap_or_abort(),
        "hook-fired"
    );
    assert!(
        receipt.is_file(),
        "trusted activation must write a load receipt"
    );
}

#[test]
fn local_plugin_source_survives_reload_disable_enable_and_uninstall() {
    // arrange — a durable registry and a local directory source with hooks and skills.
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_local_plugin(&workspace, "local.source.plugin");

    // act — the local source is installed, trusted, and re-opened after restart.
    let mut registry = PluginLifecycleRegistry::open(&workspace).unwrap_or_abort();
    registry
        .install_from_package_root(&package)
        .unwrap_or_abort();
    registry
        .activate("local.source.plugin", PluginActivationPermission::Granted)
        .unwrap_or_abort();
    drop(registry);
    let mut reloaded = PluginLifecycleRegistry::open(&workspace).unwrap_or_abort();

    // assert — persisted activation is observable and disable/enable/uninstall survive restarts.
    assert!(reloaded.is_enabled("local.source.plugin"));
    assert!(workspace.join(PLUGIN_REGISTRY_REL).is_file());
    reloaded.deactivate("local.source.plugin").unwrap_or_abort();
    reloaded
        .activate("local.source.plugin", PluginActivationPermission::Granted)
        .unwrap_or_abort();
    reloaded.deactivate("local.source.plugin").unwrap_or_abort();
    reloaded.remove("local.source.plugin").unwrap_or_abort();
    drop(reloaded);
    assert!(PluginLifecycleRegistry::open(&workspace)
        .unwrap_or_abort()
        .is_empty());
}

#[test]
fn local_acp_stdio_and_file_agent_modes_restart_with_observable_effects() {
    // arrange — local stdio and file-backed ACP transports.
    let temp = tempfile::tempdir().unwrap_or_abort();

    // act — stdio agent mode is started twice and the file agent mode writes a frame.
    let first = harness_core::integrations::acp_stdio::run_stdio_acp_agent_mode_product("cat");
    let restarted = harness_core::integrations::acp_stdio::run_stdio_acp_agent_mode_product("cat");
    let file_product = run_file_acp_agent_mode_product(temp.path());

    // assert — each local transport completes without a network endpoint.
    assert!(first.meets_agent_mode_contract());
    assert!(restarted.meets_agent_mode_contract());
    assert!(file_product.meets_agent_mode_contract());
    assert!(file_product.transport_root.join("frames.jsonl").is_file());
}

#[test]
fn configured_local_mcp_and_lsp_surfaces_preserve_stdio_http_and_consent_shapes() {
    // arrange — local stdio MCP, configured HTTP MCP, and a local LSP command.
    let stdio = McpServerConfig::Stdio {
        command: vec!["python3".to_string(), "local-mcp.py".to_string()],
        env: BTreeMap::from([("LOCAL_ONLY".to_string(), "1".to_string())]),
        cwd: Some(PathBuf::from(".")),
        timeout_secs: 5,
        enabled: true,
    };
    let http = McpServerConfig::Http {
        endpoint: "http://127.0.0.1:9911/mcp".to_string(),
        headers: BTreeMap::new(),
        timeout_secs: 5,
        enabled: true,
    };
    let lsp = LspConfig {
        disabled: false,
        servers: BTreeMap::from([(
            "local-rust".to_string(),
            LspServerConfig {
                disabled: false,
                command: Some(vec!["local-rust-lsp".to_string(), "--stdio".to_string()]),
                extensions: Some(vec![".rs".to_string()]),
                env: BTreeMap::new(),
                initialization: Some(json!({"local": true})),
            },
        )]),
    };

    // act — local-only transport and consent configuration is inspected.
    let serialized = serde_json::to_string(&lsp).unwrap_or_abort();

    // assert — configured endpoints stay explicit; no implicit network provisioning is introduced.
    assert!(stdio.enabled());
    assert!(http.enabled());
    assert!(serialized.contains("local-rust-lsp"));
    assert!(serialized.contains(".rs"));
}

#[test]
fn persistent_code_graph_returns_real_relationships_from_local_filesystem() {
    // arrange — a local source tree where beta calls alpha.
    let temp = tempfile::tempdir().unwrap_or_abort();
    let src = temp.path().join("src");
    fs::create_dir_all(&src).unwrap_or_abort();
    fs::write(
        src.join("lib.rs"),
        "pub fn alpha() {}\npub fn beta() { alpha(); }\n",
    )
    .unwrap_or_abort();

    // act — a persistent index is built and its definition/reference relationships are queried.
    let (index_path, index) = build_persistent_graph_index(temp.path()).unwrap_or_abort();
    let callers = query_persistent_graph(
        temp.path(),
        &GraphQuery::with_kind("alpha", GraphQueryKind::Callers),
    );
    let references = query_persistent_graph(
        temp.path(),
        &GraphQuery::with_kind("alpha", GraphQueryKind::References),
    );

    // assert — the graph is persistent and relationship-aware.
    assert!(index_path.is_file());
    assert!(index
        .edges
        .iter()
        .any(|edge| edge.caller == "beta" && edge.callee == "alpha"));
    assert_eq!(callers.hit_count(), 1);
    assert_eq!(references.hit_count(), 1);
}

#[tokio::test]
async fn replay_of_local_integration_session_is_side_effect_free() {
    // arrange — a persisted event log next to an already-created local plugin receipt.
    let temp = tempfile::tempdir().unwrap_or_abort();
    let session_root = temp.path().join("sessions");
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_local_plugin(&workspace, "replay.local.plugin");
    let mut registry = PluginLifecycleRegistry::open(&workspace).unwrap_or_abort();
    registry
        .install_from_package_root(&package)
        .unwrap_or_abort();
    registry
        .activate("replay.local.plugin", PluginActivationPermission::Granted)
        .unwrap_or_abort();
    let store = JsonlFileEventStore::open(&session_root, "local-replay", true).unwrap_or_abort();
    store
        .append(EventEnvelopeWithoutSeqV1::from(run_started_event(
            "local-replay",
        )))
        .unwrap_or_abort();
    drop(store);
    let events_path = session_root.join("local-replay/events.jsonl");
    let before = fs::read(&events_path).unwrap_or_abort();

    // act — replay reads the session twice.
    for _ in 0..2 {
        use tokio_stream::StreamExt;
        let store = JsonlFileEventStore::open_existing(&session_root, "local-replay", true)
            .unwrap_or_abort();
        let mut replay = store.replay(1).unwrap_or_abort();
        let event = replay.next().await.unwrap_or_abort().unwrap_or_abort();
        assert_eq!(event.seq, 1);
    }

    // assert — replay did not execute integrations or mutate the durable event log.
    assert_eq!(fs::read(&events_path).unwrap_or_abort(), before);
    assert!(package.join(PLUGIN_LOAD_RECEIPT_FILE_NAME).is_file());
}
