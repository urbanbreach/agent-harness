use serde_json::Value;

const TUI_SIGNOFF_MANIFEST: &str = include_str!("../../../docs/tui-signoff-manifest.v1.json");

#[test]
fn tui_signoff_manifest_covers_required_release_flows() {
    // arrange
    let manifest: Value =
        serde_json::from_str(TUI_SIGNOFF_MANIFEST).expect("parse TUI signoff manifest");
    assert_eq!(
        manifest["schema_version"],
        "harness-tui-signoff-manifest-v1"
    );
    assert_eq!(manifest["native_visual_policy"]["lane"], "signoff-native");
    assert_eq!(
        manifest["native_visual_policy"]["missing_env_status"],
        "documented_gap"
    );

    let flows = manifest["flows"]
        .as_array()
        .expect("manifest flows must be an array");
    let required_flow_ids = [
        "startup",
        "command_palette",
        "session_picker_resume",
        "permission_question",
        "provider_tool_failure",
        "diff_review",
    ];

    // act
    let flow_ids = flows
        .iter()
        .filter_map(|flow| flow["id"].as_str())
        .collect::<Vec<_>>();

    // assert
    for flow_id in required_flow_ids {
        assert!(
            flow_ids.contains(&flow_id),
            "missing signoff flow {flow_id}"
        );
        let flow = flows
            .iter()
            .find(|flow| flow["id"] == flow_id)
            .unwrap_or_else(|| panic!("missing signoff flow {flow_id}"));
        assert_non_empty_array(flow, "deterministic_tests");
        assert_non_empty_array(flow, "pty_stages");
        assert_non_empty_array(flow, "required_markers");
    }

    assert_flow_names_test(
        flows,
        "command_palette",
        "command_palette_renders_without_pty",
    );
    assert_flow_names_test(
        flows,
        "session_picker_resume",
        "startup_session_history_picker_renders_without_pty",
    );
    assert_flow_names_test(
        flows,
        "permission_question",
        "question_permission_prompt_renders_without_pty",
    );
    assert_flow_names_test(
        flows,
        "provider_tool_failure",
        "replay_failure_state_renders_without_pty",
    );
    assert_flow_names_test(
        flows,
        "diff_review",
        "diff_hunk_navigation_advances_and_retreats_between_hunks",
    );

    assert_eq!(
        manifest["reference_image_policy"],
        "not_required_for_this_prd"
    );
    assert!(
        flows
            .iter()
            .all(|flow| flow.get("reference_assets").is_none()),
        "reference-image comparison is outside this PRD and must not be required"
    );
}

fn assert_non_empty_array(flow: &Value, field: &str) {
    assert!(
        flow[field]
            .as_array()
            .is_some_and(|values| !values.is_empty()),
        "flow {} must declare a non-empty {field} array",
        flow["id"]
    );
}

fn assert_flow_names_test(flows: &[Value], flow_id: &str, test_name: &str) {
    let flow = flows
        .iter()
        .find(|flow| flow["id"] == flow_id)
        .unwrap_or_else(|| panic!("missing signoff flow {flow_id}"));
    let deterministic_tests = flow["deterministic_tests"]
        .as_array()
        .expect("deterministic_tests must be an array");
    assert!(
        deterministic_tests
            .iter()
            .any(|test| test.as_str().is_some_and(|test| test.contains(test_name))),
        "flow {flow_id} must name deterministic owner {test_name}"
    );
}
