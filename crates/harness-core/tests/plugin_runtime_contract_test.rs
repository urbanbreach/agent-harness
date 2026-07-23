//! PluginRuntimeContract tests — lifecycle event recording, execution surface,
//! failure isolation, cancellation, transactional upgrade/rollback.

use std::fs;
use std::path::{Path, PathBuf};

use harness_core::extension_manifest::EXTENSION_MANIFEST_V1_SCHEMA_VERSION;
use harness_core::integrations::{
    FailingPlugin, HelloWorldPlugin, PluginActivationPermission, PluginExecutionSurface,
    PluginLifecycleEvent, PluginRuntimeContract, PluginRuntimeError, PLUGIN_MANIFEST_FILE_NAME,
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

#[test]
fn runtime_contract_records_lifecycle_events() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_plugin_package(&workspace, "plugins/demo", "demo.plugin");
    let mut contract = PluginRuntimeContract::new(&workspace);

    // act
    contract
        .install_from_package_root(&package)
        .unwrap_or_abort();
    contract
        .activate("demo.plugin", PluginActivationPermission::Granted)
        .unwrap_or_abort();
    contract.deactivate("demo.plugin").unwrap_or_abort();
    contract.remove("demo.plugin").unwrap_or_abort();

    // assert
    let events = contract.events();
    assert_eq!(events.len(), 4);
    assert!(matches!(&events[0], PluginLifecycleEvent::Installed { id } if id == "demo.plugin"));
    assert!(matches!(&events[1], PluginLifecycleEvent::Activated { id } if id == "demo.plugin"));
    assert!(matches!(&events[2], PluginLifecycleEvent::Deactivated { id } if id == "demo.plugin"));
    assert!(matches!(&events[3], PluginLifecycleEvent::Removed { id } if id == "demo.plugin"));
}

#[test]
fn runtime_contract_executes_plugin_via_compiled_surface() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_plugin_package(&workspace, "plugins/hello", "hello.plugin");
    let mut contract = PluginRuntimeContract::new(&workspace);
    contract
        .install_from_package_root(&package)
        .unwrap_or_abort();
    contract
        .activate("hello.plugin", PluginActivationPermission::Granted)
        .unwrap_or_abort();
    contract
        .register_execution_surface(Box::new(HelloWorldPlugin::new("hello.plugin")))
        .unwrap_or_abort();

    // act
    let result = contract
        .execute_plugin("hello.plugin", "op-1", "world")
        .unwrap_or_abort();

    // assert
    assert_eq!(result.plugin_id, "hello.plugin");
    assert_eq!(result.operation_id, "op-1");
    assert!(result.output.contains("hello from hello.plugin: world"));
    let events = contract.events();
    assert!(events.iter().any(|e| matches!(
        e,
        PluginLifecycleEvent::ExecutionStarted { id, operation_id } if id == "hello.plugin" && operation_id == "op-1"
    )));
    assert!(events.iter().any(|e| matches!(
        e,
        PluginLifecycleEvent::ExecutionFinished { id, operation_id, success } if id == "hello.plugin" && operation_id == "op-1" && *success
    )));
}

#[test]
fn runtime_contract_isolates_plugin_failures() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_plugin_package(&workspace, "plugins/fail", "fail.plugin");
    let mut contract = PluginRuntimeContract::new(&workspace);
    contract
        .install_from_package_root(&package)
        .unwrap_or_abort();
    contract
        .activate("fail.plugin", PluginActivationPermission::Granted)
        .unwrap_or_abort();
    contract
        .register_execution_surface(Box::new(FailingPlugin::new("fail.plugin")))
        .unwrap_or_abort();

    // act
    let result = contract.execute_plugin("fail.plugin", "op-fail", "input");

    // assert
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, PluginRuntimeError::ExecutionFailed { id, .. } if id == "fail.plugin"));
    let events = contract.events();
    assert!(events.iter().any(|e| matches!(
        e,
        PluginLifecycleEvent::ExecutionFinished { id, success, .. } if id == "fail.plugin" && !*success
    )));
}

#[test]
fn runtime_contract_cancels_operations() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_plugin_package(&workspace, "plugins/cancel", "cancel.plugin");
    let mut contract = PluginRuntimeContract::new(&workspace);
    contract
        .install_from_package_root(&package)
        .unwrap_or_abort();
    contract
        .activate("cancel.plugin", PluginActivationPermission::Granted)
        .unwrap_or_abort();
    contract
        .register_execution_surface(Box::new(HelloWorldPlugin::new("cancel.plugin")))
        .unwrap_or_abort();
    contract.cancel_operation("op-cancelled");

    // act
    let result = contract.execute_plugin("cancel.plugin", "op-cancelled", "input");

    // assert
    assert!(matches!(
        result.unwrap_err(),
        PluginRuntimeError::OperationCancelled { id, operation_id } if id == "cancel.plugin" && operation_id == "op-cancelled"
    ));
    let events = contract.events();
    assert!(events.iter().any(|e| matches!(
        e,
        PluginLifecycleEvent::Cancelled { id, operation_id } if id == "cancel.plugin" && operation_id == "op-cancelled"
    )));
}

#[test]
fn runtime_contract_upgrades_plugin() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package_v1 = write_plugin_package(&workspace, "plugins/upg-v1", "upg.plugin");
    let package_v2 = write_plugin_package(&workspace, "plugins/upg-v2", "upg.plugin");
    let mut contract = PluginRuntimeContract::new(&workspace);
    contract
        .install_from_package_root(&package_v1)
        .unwrap_or_abort();
    contract
        .activate("upg.plugin", PluginActivationPermission::Granted)
        .unwrap_or_abort();

    // act
    contract
        .upgrade_plugin(
            "upg.plugin",
            &package_v2,
            PluginActivationPermission::Granted,
        )
        .unwrap_or_abort();

    // assert
    assert!(contract.is_enabled("upg.plugin"));
    let events = contract.events();
    assert!(events
        .iter()
        .any(|e| matches!(e, PluginLifecycleEvent::Upgraded { id } if id == "upg.plugin")));
    assert!(events
        .iter()
        .any(|e| matches!(e, PluginLifecycleEvent::Deactivated { id } if id == "upg.plugin")));
    assert!(events
        .iter()
        .any(|e| matches!(e, PluginLifecycleEvent::Removed { id } if id == "upg.plugin")));
    assert!(events
        .iter()
        .any(|e| matches!(e, PluginLifecycleEvent::Installed { id } if id == "upg.plugin")));
    assert!(events
        .iter()
        .any(|e| matches!(e, PluginLifecycleEvent::Activated { id } if id == "upg.plugin")));
}

#[test]
fn runtime_contract_denies_execution_for_disabled_plugin() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_plugin_package(&workspace, "plugins/disabled", "disabled.plugin");
    let mut contract = PluginRuntimeContract::new(&workspace);
    contract
        .install_from_package_root(&package)
        .unwrap_or_abort();
    contract
        .register_execution_surface(Box::new(HelloWorldPlugin::new("disabled.plugin")))
        .unwrap_or_abort();

    // act
    let result = contract.execute_plugin("disabled.plugin", "op-1", "input");

    // assert
    assert!(matches!(
        result.unwrap_err(),
        PluginRuntimeError::NotEnabledForExecution { id } if id == "disabled.plugin"
    ));
}

#[test]
fn runtime_contract_upgrade_rolls_back_on_install_failure() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package = write_plugin_package(&workspace, "plugins/upg-fail", "upgfail.plugin");
    let mut contract = PluginRuntimeContract::new(&workspace);
    contract
        .install_from_package_root(&package)
        .unwrap_or_abort();
    contract
        .activate("upgfail.plugin", PluginActivationPermission::Granted)
        .unwrap_or_abort();
    let events_before = contract.events().len();

    // act — upgrade with invalid package root
    let bad_path = workspace.join("nonexistent");
    let result = contract.upgrade_plugin(
        "upgfail.plugin",
        &bad_path,
        PluginActivationPermission::Granted,
    );

    // assert — old plugin restored
    assert!(result.is_err());
    assert!(contract.get("upgfail.plugin").is_some());
    assert!(contract.is_enabled("upgfail.plugin"));

    // assert — failed upgrade events truncated, restoration events recorded
    let events = contract.events();
    assert_eq!(events.len(), events_before + 2);
    assert!(matches!(
        &events[events_before],
        PluginLifecycleEvent::Installed { id } if id == "upgfail.plugin"
    ));
    assert!(matches!(
        &events[events_before + 1],
        PluginLifecycleEvent::Activated { id } if id == "upgfail.plugin"
    ));
}

#[test]
fn runtime_contract_upgrade_preserves_disabled_state() {
    // arrange — plugin installed but NOT activated (disabled)
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();
    let package_v1 = write_plugin_package(&workspace, "plugins/dis-upg-v1", "dis.plugin");
    let package_v2 = write_plugin_package(&workspace, "plugins/dis-upg-v2", "dis.plugin");
    let mut contract = PluginRuntimeContract::new(&workspace);
    contract
        .install_from_package_root(&package_v1)
        .unwrap_or_abort();
    assert!(!contract.is_enabled("dis.plugin"));

    // act — upgrade without activating
    contract
        .upgrade_plugin(
            "dis.plugin",
            &package_v2,
            PluginActivationPermission::Granted,
        )
        .unwrap_or_abort();

    // assert — plugin remains disabled after upgrade
    assert!(!contract.is_enabled("dis.plugin"));
    let events = contract.events();
    assert!(events
        .iter()
        .any(|e| matches!(e, PluginLifecycleEvent::Upgraded { id } if id == "dis.plugin")));
    assert!(!events
        .iter()
        .any(|e| matches!(e, PluginLifecycleEvent::Activated { id } if id == "dis.plugin")));
}
