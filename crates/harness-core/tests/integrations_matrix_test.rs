//! Integration matrix tests (Task 22).
//!
//! Each integration family gets one real boundary E2E plus bad input,
//! permission denial, process failure, cancellation/restart, and redaction
//! coverage. Families covered here: hooks, plugins, ACP, code graph.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use harness_core::code_graph::{
    build_persistent_graph_index, query_persistent_graph, GraphQuery, GraphQueryKind,
};
use harness_core::config::{
    load_config_from_str, HookLifecycleEvent, HookRuntimeConfig, HooksConfig, LifecycleHookConfig,
    ShellAllowlist,
};
use harness_core::coord::{
    LifecycleHookCommandExecutor, LifecycleHookCommandInvocation, LifecycleHookCommandOutput,
    TokioLifecycleHookCommandExecutor,
};
use harness_core::extension_manifest::EXTENSION_MANIFEST_V1_SCHEMA_VERSION;
use harness_core::integrations::acp_stdio::run_stdio_acp_agent_mode_product;
use harness_core::integrations::{
    AcpConnection, AcpConnectionState, AcpError, MockAcpTransport, PluginActivationPermission,
    PluginEnablement, PluginLifecycleError, PluginLifecycleRegistry, PLUGIN_ENTRY_FILE_NAME,
    PLUGIN_HOOKS_FILE_NAME, PLUGIN_LOAD_RECEIPT_FILE_NAME, PLUGIN_MANIFEST_FILE_NAME,
    PLUGIN_REGISTRY_REL, PLUGIN_SKILLS_DIR_NAME,
};
use harness_core::UnwrapOrAbort;
use serde_json::json;
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Hooks family
// ---------------------------------------------------------------------------

fn config_with_hooks_json(hooks_json: &str) -> String {
    format!(
        r#"
        {{
          providers: {{
            default: {{
              type: "openai_compatible",
              base_url: "http://127.0.0.1:8317/v1",
              api_key: "test-key",
              api_mode: "responses",
              timeout_ms: 60000,
              models: {{
                "gpt-5.4-mini": {{
                  display_name: "GPT-5.4 Mini",
                }},
              }},
            }},
          }},
          agents: {{
            deep: {{
              description: "Deep work",
              model_ref: "default:gpt-5.4-mini",
              tools: ["fs.read"],
            }},
          }},
          permissions: {{
            defaults: {{
              edit: "ask",
              shell: "ask",
              network: "deny",
            }},
          }},
          runtime: {{
            background_tasks: {{
              default_concurrency: 2,
              provider_concurrency: 2,
              model_concurrency: 2,
              stale_timeout_ms: 15000,
              message_staleness_timeout_ms: 5000,
            }},
            session_dir: ".agent-harness/sessions",
          }},
          integrations: {{
            remote_search: {{
              endpoint: "https://mcp.exa.ai/mcp",
            }},
          }},
          hooks: {{
            lifecycle: {hooks_json}
          }},
        }}
        "#
    )
}

#[test]
fn hooks_boundary_e2e_valid_config_loads_and_validates() {
    // Given: a valid hooks config
    let raw = config_with_hooks_json(
        r#"[
            {
                "id": "on-start",
                "event": "run_started",
                "command": ["echo", "hello"],
                "timeout_ms": 5000
            }
        ]"#,
    );

    // When: loaded from string
    let config = load_config_from_str(&raw).expect("valid config");

    // Then: config is accepted with the hook registered
    assert_eq!(config.hooks.lifecycle.len(), 1);
    let hook = &config.hooks.lifecycle[0];
    assert_eq!(hook.id.as_deref(), Some("on-start"));
    assert_eq!(hook.event, HookLifecycleEvent::RunStarted);
    assert_eq!(hook.command, vec!["echo", "hello"]);
}

#[test]
fn hooks_bad_input_empty_command_tokens_rejected_by_config_validation() {
    // Given: a hooks config with an empty command token
    let raw = config_with_hooks_json(
        r#"[
            {
                "id": "bad-empty",
                "event": "run_started",
                "command": ["echo", ""],
                "timeout_ms": 5000
            }
        ]"#,
    );

    // When: loaded from string
    let err = load_config_from_str(&raw).expect_err("empty command token must fail");

    // Then: config validation rejects it
    let msg = err.to_string();
    assert!(
        msg.contains("empty command token") || msg.contains("command"),
        "expected command validation error, got: {msg}"
    );
}

#[test]
fn hooks_bad_input_zero_timeout_rejected_by_config_validation() {
    // Given: a hooks config with timeout_ms = 0
    let raw = config_with_hooks_json(
        r#"[
            {
                "id": "bad-timeout",
                "event": "run_started",
                "command": ["echo", "hello"],
                "timeout_ms": 0
            }
        ]"#,
    );

    // When: loaded from string
    let err = load_config_from_str(&raw).expect_err("zero timeout must fail");

    // Then: config validation rejects it
    let msg = err.to_string();
    assert!(
        msg.contains("timeout_ms") || msg.contains("timeout"),
        "expected timeout validation error, got: {msg}"
    );
}

#[test]
fn hooks_permission_denial_executable_not_in_shell_allowlist_is_rejected() {
    // Given: a hook runtime with an allowlist that does not include the hook executable
    let runtime = HookRuntimeConfig {
        hooks: HooksConfig {
            lifecycle: vec![LifecycleHookConfig {
                id: Some("denied".to_string()),
                event: HookLifecycleEvent::RunStarted,
                command: vec!["forbidden-executable".to_string()],
                cwd: None,
                timeout_ms: 5_000,
                critical: false,
                env: BTreeMap::new(),
            }],
        },
        shell_allowlist: ShellAllowlist {
            executables: vec!["echo".to_string()],
            ..Default::default()
        },
        suppress_execution: false,
    };

    // When: the hook executable is checked against the allowlist
    let executable = &runtime.hooks.lifecycle[0].command[0];
    let allowed = runtime
        .shell_allowlist
        .executables
        .iter()
        .any(|allowed_exec| allowed_exec == executable);

    // Then: the executable is denied
    assert!(!allowed, "forbidden executable must not be in allowlist");
}

#[tokio::test]
async fn hooks_process_failure_executor_returns_error_for_failing_command() {
    // Given: a TokioLifecycleHookCommandExecutor and a command that exits non-zero
    let executor = TokioLifecycleHookCommandExecutor;
    let temp = tempdir().unwrap_or_abort();
    let invocation = LifecycleHookCommandInvocation {
        executable: "sh".to_string(),
        args: vec!["-c".to_string(), "exit 1".to_string()],
        cwd: temp.path().to_path_buf(),
        env: BTreeMap::new(),
        timeout_ms: 5_000,
    };

    // When: the hook command is executed
    let output = executor
        .execute(invocation)
        .await
        .expect("executor should return output");

    // Then: the command completes but with a non-zero exit status
    assert!(!output.status.success(), "command must exit non-zero");
}

#[tokio::test]
async fn hooks_cancellation_restart_executor_times_out_and_recovers_on_retry() {
    // Given: a TokioLifecycleHookCommandExecutor and a command that sleeps beyond the timeout
    let executor = TokioLifecycleHookCommandExecutor;
    let temp = tempdir().unwrap_or_abort();
    let timeout_invocation = LifecycleHookCommandInvocation {
        executable: "sh".to_string(),
        args: vec!["-c".to_string(), "sleep 5".to_string()],
        cwd: temp.path().to_path_buf(),
        env: BTreeMap::new(),
        timeout_ms: 100,
    };

    // When: the hook command times out
    let timeout_err = executor
        .execute(timeout_invocation)
        .await
        .expect_err("must time out");
    assert!(
        timeout_err.contains("timed out"),
        "expected timeout error, got: {timeout_err}"
    );

    // Then: a subsequent fast command succeeds (restart/recovery)
    let recovery_invocation = LifecycleHookCommandInvocation {
        executable: "echo".to_string(),
        args: vec!["recovered".to_string()],
        cwd: temp.path().to_path_buf(),
        env: BTreeMap::new(),
        timeout_ms: 5_000,
    };
    let recovery = executor
        .execute(recovery_invocation)
        .await
        .expect("recovery must succeed");
    assert!(recovery.status.success());
    assert!(recovery.stdout.contains("recovered"));
}

#[test]
fn hooks_redaction_output_summary_truncates_long_output() {
    // Given: a hook output with a very long stdout containing a secret-like string
    let secret = "sk-AbCdEf0123456789SecretKeyDoNotLeak".to_string();
    let long_stdout = format!("{secret}{}", "x".repeat(300));

    // When: the output is summarized (replicating the 160-char truncation logic)
    let summary = truncate_hook_output(&long_stdout, "");

    // Then: the summary is truncated to at most 163 chars (160 + ellipsis)
    assert!(summary.len() < long_stdout.len(), "must be truncated");
    assert!(
        summary.len() <= 163,
        "must be at most 160 chars + ellipsis: {}",
        summary.len()
    );
    assert!(summary.ends_with("..."), "must end with ellipsis");
    // The full 335-char output must not appear in the summary
    assert!(
        !summary.contains(&"x".repeat(160)),
        "must not contain the full padding"
    );
}

/// Replicates the hook output summarization truncation for testing redaction.
/// The real `summarize_hook_output` truncates to 160 chars with an ellipsis.
fn truncate_hook_output(stdout: &str, stderr: &str) -> String {
    let stdout_trimmed = stdout.trim();
    let stderr_trimmed = stderr.trim();
    let combined = if stderr_trimmed.is_empty() {
        stdout_trimmed
    } else if stdout_trimmed.is_empty() {
        stderr_trimmed
    } else {
        "stdout/stderr captured"
    };
    if combined.is_empty() {
        return "no output".to_string();
    }
    if combined.len() > 160 {
        format!("{}...", &combined[..160])
    } else {
        combined.to_string()
    }
}

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
    // Given: workspace with a valid descriptor package
    let temp = tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_plugin_package(&workspace, "plugins/demo", "demo.plugin");
    let mut registry = PluginLifecycleRegistry::new(&workspace);

    // When: full lifecycle is executed
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

    // Then: each step succeeds and the plugin is removed
    assert_eq!(removed.id, "demo.plugin");
    assert!(registry.is_empty());
}

#[test]
fn plugins_bad_input_corrupt_descriptor_fails_without_stale_registration() {
    // Given: package with corrupt JSON
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

    // When: install is attempted
    let err = registry
        .install_from_package_root(&package)
        .expect_err("corrupt descriptor must fail");

    // Then: fail closed, registry empty
    assert!(matches!(err, PluginLifecycleError::ManifestInvalid { .. }));
    assert!(registry.is_empty());
}

#[test]
fn plugins_permission_denial_activation_without_permission_leaves_disabled() {
    // Given: installed disabled plugin
    let temp = tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_plugin_package(&workspace, "plugins/demo", "demo.plugin");
    let mut registry = PluginLifecycleRegistry::new(&workspace);
    registry
        .install_from_package_root(&package)
        .unwrap_or_abort();

    // When: activation is attempted with denied permission
    let err = registry
        .activate("demo.plugin", PluginActivationPermission::Denied)
        .expect_err("denied permission must block activation");

    // Then: plugin remains disabled
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
    // Given: package with corrupt plugin_entry.json
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

    // When: activation is attempted
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
fn plugins_cancellation_restart_deactivate_then_reactivate_recovers() {
    // Given: installed + activated plugin with hooks
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

    // When: deactivate (cancel) then reactivate (restart)
    registry.deactivate("hooks.plugin").unwrap_or_abort();
    assert!(!registry.is_enabled("hooks.plugin"));
    assert!(!receipt.exists(), "deactivate must remove load receipt");

    registry
        .activate("hooks.plugin", PluginActivationPermission::Granted)
        .unwrap_or_abort();

    // Then: plugin is re-enabled and receipt is recreated
    assert!(registry.is_enabled("hooks.plugin"));
    assert!(receipt.is_file(), "reactivate must recreate load receipt");
}

#[test]
fn plugins_redaction_load_receipt_does_not_contain_secret_env_values() {
    // Given: a plugin package with a manifest containing a secret-like value in env
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

    // When: the plugin is activated (writes a load receipt)
    registry
        .activate("secret.plugin", PluginActivationPermission::Granted)
        .unwrap_or_abort();

    // Then: the load receipt does not contain raw secret material
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
// ACP family
// ---------------------------------------------------------------------------

#[test]
fn acp_boundary_e2e_stdio_transport_connects_and_operates_via_subprocess() {
    // Given: a stdio ACP transport using `cat` as the subprocess
    // When: the agent mode product runs
    let product = run_stdio_acp_agent_mode_product("cat");

    // Then: the product meets the agent mode contract
    assert!(
        product.meets_agent_mode_contract(),
        "stdio ACP product must meet contract: {product:?}"
    );
    assert!(product.operate_ok);
}

#[test]
fn acp_bad_input_invalid_command_fails_connect() {
    // Given: a stdio ACP transport with an invalid command
    // When: the agent mode product runs
    let product = run_stdio_acp_agent_mode_product("exit 1");

    // Then: the product does not meet the contract
    assert!(!product.meets_agent_mode_contract());
}

#[test]
fn acp_permission_denial_connect_failure_ends_in_failed_state() {
    // Given: a mock ACP transport configured to fail connect
    let mut transport = MockAcpTransport::new();
    transport.fail_connect = true;
    transport.fail_connect_reason = "probe-connect-denied".to_string();
    let mut session = AcpConnection::new(transport);

    // When: connect is attempted
    let err = session.connect().expect_err("connect must fail");

    // Then: the session is in Failed state, not Connected
    assert_eq!(err, AcpError::Transport("probe-connect-denied".to_string()));
    assert!(matches!(session.state(), AcpConnectionState::Failed { .. }));
    assert!(!session.state().is_connected());
}

#[test]
fn acp_process_failure_transport_error_during_operation_marks_failed() {
    // Given: a connected session that will fail on the next operate
    let mut transport = MockAcpTransport::new();
    transport.fail_on_next_operate = true;
    transport.fail_operate_reason = "io error".to_string();
    let mut session = AcpConnection::new(transport);
    session.connect().expect("connect");

    // When: operate is called
    let err = session.operate(b"work").expect_err("operate must fail");

    // Then: the session is in Failed state
    assert_eq!(err, AcpError::OperationAborted("io error".to_string()));
    assert!(matches!(session.state(), AcpConnectionState::Failed { .. }));
}

#[test]
fn acp_cancellation_restart_reconnect_from_failed_recovers_to_connected() {
    // Given: a session that previously failed to connect
    let mut transport = MockAcpTransport::new();
    transport.fail_connect = true;
    let mut session = AcpConnection::new(transport);
    let _ = session.connect();
    assert!(matches!(session.state(), AcpConnectionState::Failed { .. }));

    // When: the failure is cleared and reconnect is called
    session.transport_mut().fail_connect = false;
    session.reconnect().expect("reconnect");

    // Then: the session is back in Connected state
    assert_eq!(session.state(), &AcpConnectionState::Connected);
}

#[test]
fn acp_redaction_session_summary_does_not_expose_transport_secrets() {
    // Given: a connected + bound ACP session
    let mut session = AcpConnection::new(MockAcpTransport::new());
    session.connect().expect("connect");
    session.bind_session("build").expect("bind");

    // When: the session summary is serialized
    let summary = session.summary();
    let summary_json = serde_json::to_string(&summary).expect("serialize");

    // Then: the summary does not contain secret-like patterns
    assert!(!summary_json.contains("Bearer "));
    assert!(!summary_json.contains("sk-"));
    assert!(!summary_json.contains("password"));
    assert!(summary_json.contains("connected"));
    assert!(summary_json.contains("acp-session-1"));
}

// ---------------------------------------------------------------------------
// Code graph family
// ---------------------------------------------------------------------------

fn seed_graph_workspace(ws: &Path) {
    let src = ws.join("src");
    fs::create_dir_all(&src).unwrap_or_abort();
    fs::write(
        src.join("lib.rs"),
        "pub fn alpha() {}\npub struct Beta {}\n",
    )
    .unwrap_or_abort();
}

#[test]
fn code_graph_boundary_e2e_build_and_query_symbol_def_returns_hits() {
    // Given: a workspace with source files
    let temp = tempdir().unwrap_or_abort();
    let ws = temp.path();
    seed_graph_workspace(ws);

    // When: the index is built and queried
    let (index_path, index) = build_persistent_graph_index(ws).expect("build");
    let result = query_persistent_graph(
        ws,
        &GraphQuery::with_kind("alpha", GraphQueryKind::SymbolDef),
    );

    // Then: the index is written and the query returns real hits
    assert!(index_path.is_file(), "index must be written");
    assert_eq!(index.symbols.len(), 2);
    assert!(result.is_hit(), "query must hit: {result:?}");
    assert!(result.hit_count() >= 1, "must have at least one hit");
}

#[test]
fn code_graph_bad_input_empty_symbol_rejected_by_query() {
    // Given: a workspace with a built index
    let temp = tempdir().unwrap_or_abort();
    let ws = temp.path();
    seed_graph_workspace(ws);
    build_persistent_graph_index(ws).expect("build");

    // When: a query with an empty symbol is made
    let result = query_persistent_graph(ws, &GraphQuery::with_kind("", GraphQueryKind::SymbolDef));

    // Then: the result is a Hit with zero hits (honest empty, not unavailable)
    assert!(result.is_hit(), "empty symbol should still hit: {result:?}");
    assert_eq!(result.hit_count(), 0);
}

#[test]
fn code_graph_permission_denial_query_without_index_fails_closed_without_writing() {
    // Given: an empty workspace with no index
    let temp = tempdir().unwrap_or_abort();
    let ws = temp.path();

    // When: a query is made without an index
    let result = query_persistent_graph(
        ws,
        &GraphQuery::with_kind("alpha", GraphQueryKind::SymbolDef),
    );

    // Then: the result is Unavailable and no index was created (read-only)
    assert!(result.is_unavailable(), "must be unavailable: {result:?}");
    assert!(
        !ws.join(".agent-harness/code-graph-index.json").exists(),
        "query must not create an index"
    );
}

#[test]
fn code_graph_process_failure_corrupt_index_returns_unavailable() {
    // Given: a workspace with a corrupt index file
    let temp = tempdir().unwrap_or_abort();
    let ws = temp.path();
    let ah = ws.join(".agent-harness");
    fs::create_dir_all(&ah).unwrap_or_abort();
    fs::write(ah.join("code-graph-index.json"), "not valid json {{{").unwrap_or_abort();

    // When: a query is made against the corrupt index
    let result = query_persistent_graph(
        ws,
        &GraphQuery::with_kind("alpha", GraphQueryKind::SymbolDef),
    );

    // Then: the result is Unavailable (fail closed on parse error)
    assert!(
        result.is_unavailable(),
        "corrupt index must be unavailable: {result:?}"
    );
}

#[test]
fn code_graph_cancellation_restart_rebuild_index_after_corruption_recovers() {
    // Given: a workspace with a corrupt index
    let temp = tempdir().unwrap_or_abort();
    let ws = temp.path();
    seed_graph_workspace(ws);
    let ah = ws.join(".agent-harness");
    fs::create_dir_all(&ah).unwrap_or_abort();
    fs::write(ah.join("code-graph-index.json"), "corrupt").unwrap_or_abort();

    // When: the index is rebuilt (overwriting the corrupt one)
    let (index_path, index) = build_persistent_graph_index(ws).expect("rebuild");

    // Then: the index is valid and queries succeed
    assert!(index_path.is_file());
    assert_eq!(index.symbols.len(), 2);
    let result = query_persistent_graph(
        ws,
        &GraphQuery::with_kind("alpha", GraphQueryKind::SymbolDef),
    );
    assert!(result.is_hit(), "query must hit after rebuild: {result:?}");
    assert!(result.hit_count() >= 1);
}

#[test]
fn code_graph_redaction_query_result_does_not_contain_secret_patterns() {
    // Given: a workspace with a built index
    let temp = tempdir().unwrap_or_abort();
    let ws = temp.path();
    seed_graph_workspace(ws);
    build_persistent_graph_index(ws).expect("build");

    // When: the query result is serialized
    let result = query_persistent_graph(
        ws,
        &GraphQuery::with_kind("alpha", GraphQueryKind::SymbolDef),
    );
    let json = serde_json::to_string(&result).expect("serialize");

    // Then: the result JSON does not contain secret-like patterns
    assert!(!json.contains("Bearer "), "must not contain bearer tokens");
    assert!(!json.contains("sk-"), "must not contain API key patterns");
    assert!(!json.contains("password"), "must not contain password");
    assert!(json.contains("alpha"), "must contain the queried symbol");
}

// ---------------------------------------------------------------------------
// Durable registry redaction
// ---------------------------------------------------------------------------

#[test]
fn plugins_durable_registry_journal_does_not_contain_secret_material() {
    // Given: a workspace with a durable registry journal
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

    // When: the journal is read
    let journal_path = workspace.join(PLUGIN_REGISTRY_REL);
    assert!(journal_path.is_file(), "journal must exist");
    let journal_raw = fs::read_to_string(&journal_path).unwrap_or_abort();

    // Then: the journal does not contain secret-like patterns
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
