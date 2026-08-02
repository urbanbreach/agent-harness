//! Plugin + ACP lifecycle product contract tests (T10 continuation).

use std::fs;

use harness_core::integrations::{
    run_mock_acp_agent_mode_product, run_multi_descriptor_discover_product,
    run_multi_plugin_lifecycle_product, AcpConnectionState, PROBE_ACP_AGENT_NAME,
    PROBE_EXTENSION_ALT_ID, PROBE_EXTENSION_PRIMARY_ID, PROBE_EXTENSION_TOOLS_ID,
    PROBE_PLUGIN_PRIMARY_ID, PROBE_PLUGIN_SECONDARY_ID,
};
use harness_core::UnwrapOrAbort;

#[test]
fn multi_plugin_lifecycle_product_meets_contract() {
    // arrange
    // act
    // assert
    // Given
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();

    // When
    let product = run_multi_plugin_lifecycle_product(&workspace);

    // Then
    assert!(
        product.meets_multi_plugin_contract(),
        "multi-plugin product contract failed: summary={:?} install={} activate={} deactivate={} remove={}",
        product.summary,
        product.last_install.one_line(),
        product.last_activate.one_line(),
        product.last_deactivate.one_line(),
        product.last_remove.one_line(),
    );
    assert!(product
        .first_line
        .as_deref()
        .is_some_and(|line| line.contains(PROBE_PLUGIN_PRIMARY_ID)
            || line.contains(PROBE_PLUGIN_SECONDARY_ID)));
    assert!(product
        .last_deactivate
        .one_line()
        .contains(PROBE_PLUGIN_SECONDARY_ID));
    assert!(product.last_remove.one_line().contains("failed"));
}

#[test]
fn multi_descriptor_discover_product_meets_contract() {
    // arrange
    // act
    // assert
    // Given
    let temp = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp.path().join("ws");
    fs::create_dir_all(&workspace).unwrap_or_abort();

    // When
    let product = run_multi_descriptor_discover_product(&workspace);

    // Then
    assert!(
        product.meets_multi_descriptor_contract(),
        "multi-descriptor product contract failed: discover={:?} primary={:?} load={} ids={:?}",
        product.discover,
        product.primary,
        product.last_load.one_line(),
        product.discovered_ids,
    );
    assert!(product
        .discovered_ids
        .iter()
        .any(|id| id == PROBE_EXTENSION_ALT_ID));
    assert!(product
        .discovered_ids
        .iter()
        .any(|id| id == PROBE_EXTENSION_TOOLS_ID));
    assert!(product
        .last_load
        .one_line()
        .contains(PROBE_EXTENSION_PRIMARY_ID));
}

#[test]
fn mock_acp_agent_mode_product_fail_then_success() {
    // arrange
    // act
    // assert
    // When
    let product = run_mock_acp_agent_mode_product();

    // Then
    assert!(
        product.meets_agent_mode_contract(),
        "ACP agent-mode product contract failed: fail_connect={} fail_bind={} last_connect={} last_bind={} summary={}",
        product.fail_connect.one_line(),
        product.fail_bind.one_line(),
        product.last_connect.one_line(),
        product.last_bind.one_line(),
        product.summary.one_line(),
    );
    assert!(
        product
            .fail_connect
            .one_line()
            .contains("probe-connect-denied")
            || product.fail_connect.one_line().contains("failed")
    );
    assert_eq!(
        product.session.as_ref().map(|s| s.agent_name.as_str()),
        Some(PROBE_ACP_AGENT_NAME)
    );
    assert!(matches!(product.state, AcpConnectionState::Connected));
}
