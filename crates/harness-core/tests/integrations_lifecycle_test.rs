//! Plugin + ACP lifecycle foundation tests (T10).
//!
//! Happy / edge / adjacent coverage for descriptor-only plugin install and the
//! offline ACP connection state machine.

use std::fs;
use std::path::{Path, PathBuf};

use harness_core::extension_manifest::EXTENSION_MANIFEST_V1_SCHEMA_VERSION;
use harness_core::integrations::{
    run_mock_acp_agent_mode_product, run_multi_descriptor_discover_product,
    run_multi_plugin_lifecycle_product, AcpConnection, AcpConnectionState, AcpConnectionSummary,
    AcpError, MockAcpTransport, PluginActivationPermission, PluginEnablement, PluginLifecycleError,
    PluginLifecycleRegistry, PluginLifecycleSummary, PLUGIN_ENTRY_FILE_NAME,
    PLUGIN_HOOKS_FILE_NAME, PLUGIN_LOAD_RECEIPT_FILE_NAME, PLUGIN_MANIFEST_FILE_NAME,
    PLUGIN_SKILLS_DIR_NAME, PROBE_ACP_AGENT_NAME, PROBE_EXTENSION_ALT_ID,
    PROBE_EXTENSION_PRIMARY_ID, PROBE_EXTENSION_TOOLS_ID, PROBE_PLUGIN_PRIMARY_ID,
    PROBE_PLUGIN_SECONDARY_ID,
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
fn acp_connect_happy_path_reaches_connected() {
    // arrange
    // act
    // assert
    // Given
    let mut session = AcpConnection::new(MockAcpTransport::new());
    assert_eq!(session.state(), &AcpConnectionState::Disconnected);

    // When
    session.connect().expect("connect should succeed");

    // Then
    assert_eq!(session.state(), &AcpConnectionState::Connected);
    assert!(session.transport().connected);
}

#[test]
fn acp_connect_failure_ends_in_failed_not_connected() {
    // arrange
    // act
    // assert
    // Given
    let mut transport = MockAcpTransport::new();
    transport.fail_connect = true;
    transport.fail_connect_reason = "refused".to_string();
    let mut session = AcpConnection::new(transport);

    // When
    let err = session.connect().expect_err("connect must fail");

    // Then
    assert_eq!(err, AcpError::Transport("refused".to_string()));
    assert_eq!(
        session.state(),
        &AcpConnectionState::Failed {
            reason: "refused".to_string()
        }
    );
    assert!(!session.state().is_connected());
}

#[test]
fn acp_disconnect_from_connected_returns_to_disconnected() {
    // arrange
    // act
    // assert
    // Given
    let mut session = AcpConnection::new(MockAcpTransport::new());
    session.connect().expect("connect");

    // When
    session.disconnect().expect("disconnect");

    // Then
    assert_eq!(session.state(), &AcpConnectionState::Disconnected);
    assert!(!session.transport().connected);
}

#[test]
fn acp_reconnect_from_failed_can_recover() {
    // arrange
    // act
    // assert
    // Given: prior failed connect
    let mut transport = MockAcpTransport::new();
    transport.fail_connect = true;
    let mut session = AcpConnection::new(transport);
    let _ = session.connect();
    assert!(matches!(session.state(), AcpConnectionState::Failed { .. }));

    // When: clear failure and reconnect
    session.transport_mut().fail_connect = false;
    session.reconnect().expect("reconnect");

    // Then
    assert_eq!(session.state(), &AcpConnectionState::Connected);
}

#[test]
fn acp_disconnect_during_operation_is_not_success() {
    // arrange
    // act
    // assert
    // Given: connected session that will drop mid-operate
    let mut transport = MockAcpTransport::new();
    transport.disconnect_on_next_operate = true;
    let mut session = AcpConnection::new(transport);
    session.connect().expect("connect");

    // When
    let err = session
        .operate(b"ping")
        .expect_err("mid-op disconnect must not succeed");

    // Then: Failed/Disconnected, never Connected success
    assert!(matches!(err, AcpError::OperationAborted(_)));
    assert!(
        matches!(
            session.state(),
            AcpConnectionState::Disconnected | AcpConnectionState::Failed { .. }
        ),
        "expected Disconnected or Failed, got {}",
        session.state()
    );
    assert!(!session.state().is_connected());
}

#[test]
fn acp_transport_error_during_operation_marks_failed() {
    // arrange
    // act
    // assert
    // Given
    let mut transport = MockAcpTransport::new();
    transport.fail_on_next_operate = true;
    transport.fail_operate_reason = "io error".to_string();
    let mut session = AcpConnection::new(transport);
    session.connect().expect("connect");

    // When
    let err = session.operate(b"work").expect_err("operate must fail");

    // Then
    assert_eq!(err, AcpError::OperationAborted("io error".to_string()));
    assert_eq!(
        session.state(),
        &AcpConnectionState::Failed {
            reason: "io error".to_string()
        }
    );
}

#[test]
fn acp_operate_while_disconnected_is_rejected() {
    // arrange
    // act
    // assert
    // Given
    let mut session = AcpConnection::new(MockAcpTransport::new());

    // When
    let err = session.operate(b"x").expect_err("must reject");

    // Then
    assert!(matches!(err, AcpError::NotConnected { .. }));
    assert_eq!(session.state(), &AcpConnectionState::Disconnected);
}

#[test]
fn acp_connect_while_connected_is_rejected() {
    // arrange
    // act
    // assert
    // Given
    let mut session = AcpConnection::new(MockAcpTransport::new());
    session.connect().expect("connect");

    // When
    let err = session.connect().expect_err("double connect");

    // Then
    assert!(matches!(err, AcpError::InvalidConnectState { .. }));
    assert_eq!(session.state(), &AcpConnectionState::Connected);
}

#[test]
fn acp_bind_session_while_connected_assigns_session_id() {
    // arrange
    // act
    // assert
    // Given
    let mut session = AcpConnection::new(MockAcpTransport::new());
    session.connect().expect("connect");
    assert!(session.session().is_none());

    // When
    let bound = session.bind_session("build").expect("bind");

    // Then
    assert_eq!(bound.agent_name, "build");
    assert_eq!(bound.session_id, "acp-session-1");
    assert_eq!(
        session.session().map(|s| s.session_id.as_str()),
        Some("acp-session-1")
    );
}

#[test]
fn acp_bind_session_while_disconnected_is_rejected() {
    // arrange
    // act
    // assert
    // Given
    let mut session = AcpConnection::new(MockAcpTransport::new());

    // When
    let err = session.bind_session("build").expect_err("must reject");

    // Then
    assert!(matches!(err, AcpError::SessionBindNotConnected { .. }));
    assert!(session.session().is_none());
}

#[test]
fn acp_bind_session_rejects_empty_agent_name_and_double_bind() {
    // arrange
    // act
    // assert
    // Given
    let mut session = AcpConnection::new(MockAcpTransport::new());
    session.connect().expect("connect");

    // When / Then empty name
    let empty_err = session.bind_session("   ").expect_err("empty");
    assert!(matches!(empty_err, AcpError::EmptyAgentName));

    // When / Then double bind
    session.bind_session("build").expect("first bind");
    let second = session.bind_session("plan").expect_err("double bind");
    assert!(matches!(
        second,
        AcpError::SessionAlreadyBound {
            session_id
        } if session_id == "acp-session-1"
    ));
}

#[test]
fn acp_disconnect_clears_bound_session() {
    // arrange
    // act
    // assert
    // Given
    let mut session = AcpConnection::new(MockAcpTransport::new());
    session.connect().expect("connect");
    session.bind_session("build").expect("bind");
    assert!(session.session().is_some());

    // When
    session.disconnect().expect("disconnect");

    // Then
    assert_eq!(session.state(), &AcpConnectionState::Disconnected);
    assert!(session.session().is_none());
}

#[test]
fn acp_operator_diagnostics_cover_state_session_and_summary() {
    // arrange
    // act
    // assert
    // Given: disconnected → connected+bound → failed with session retained
    let mut session = AcpConnection::new(MockAcpTransport::new());
    assert_eq!(
        session.summary(),
        AcpConnectionSummary {
            state: "disconnected".to_string(),
            session_id: None,
            agent_name: None,
            bound: false,
        }
    );
    assert!(session.state().one_line().contains("ACP: disconnected"));
    assert!(!session.summary().is_bound());

    // When: connect + bind
    session.connect().expect("connect");
    session.bind_session("build").expect("bind");
    let bound_summary = session.summary();
    let session_line = session.session().expect("bound").one_line();

    // Then
    assert!(session.state().one_line().contains("ACP: connected"));
    assert!(bound_summary.is_bound());
    assert!(bound_summary.one_line().contains("state=connected"));
    assert!(bound_summary.one_line().contains("session=`acp-session-1`"));
    assert!(bound_summary.one_line().contains("agent=`build`"));
    assert!(session_line.contains("id=`acp-session-1`"));
    assert!(session_line.contains("agent=`build`"));

    // When: transport error marks failed but keeps session for inspection
    session.transport_mut().fail_on_next_operate = true;
    let _ = session.operate(b"ping").expect_err("operate fails");
    assert!(session.state().one_line().contains("ACP: failed"));
    assert!(session.summary().is_bound());
    assert!(session.summary().one_line().contains("state=failed"));
}
