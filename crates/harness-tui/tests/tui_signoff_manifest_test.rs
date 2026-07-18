use harness_tui::UnwrapOrAbort;
use serde_json::Value;

const TUI_SIGNOFF_MANIFEST: &str = include_str!("../../../docs/tui-signoff-manifest.v1.json");

#[test]
#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn tui_signoff_manifest_covers_required_release_flows() {
    // arrange
    let manifest: Value = serde_json::from_str(TUI_SIGNOFF_MANIFEST).unwrap_or_abort();
    assert_eq!(
        manifest["schema_version"],
        "harness-tui-signoff-manifest-v1"
    );
    assert_eq!(manifest["native_visual_policy"]["lane"], "signoff-native");
    assert_eq!(
        manifest["native_visual_policy"]["missing_env_status"],
        "documented_gap"
    );

    let flows = manifest["flows"].as_array().unwrap_or_abort();
    let required_flow_ids = [
        "shell_topology",
        "startup",
        "command_palette",
        "session_picker_resume",
        "permission_question",
        "provider_tool_failure",
        "diff_review",
        "file_mention",
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
            .unwrap_or_else(|| {
                let _ = flow_id;
                panic!("abort");
            });
        assert_non_empty_array(flow, "deterministic_tests");
        assert_non_empty_array(flow, "pty_stages");
        assert_non_empty_array(flow, "required_markers");
    }

    assert_flow_names_test(
        flows,
        "shell_topology",
        "shell_topology_contract_test",
    );
    assert_flow_names_test(
        flows,
        "startup",
        "startup_shell_is_compose_first_without_pty",
    );
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
    assert_flow_names_test(
        flows,
        "file_mention",
        "file_mention_at_opens_picker_and_inserts_real_workspace_file",
    );

    assert_flow_pty_owner(
        flows,
        "permission_question",
        "pty_permission_overlay_resolves_and_preserves_draft",
    );
    assert_flow_pty_owner(
        flows,
        "command_palette",
        "pty_status_dialog_opens_without_sidebar_copy",
    );
    assert_flow_pty_owner(
        flows,
        "shell_topology",
        "pty_status_dialog_opens_without_sidebar_copy",
    );
    assert_flow_pty_owner(
        flows,
        "startup",
        "pty_smoke_starts_accepts_input_resizes_and_exits",
    );

    for flow in flows {
        let pty_stages = flow["pty_stages"].as_array().unwrap_or_abort();
        for stage in pty_stages {
            let stage = stage.as_str().unwrap_or_abort();
            assert!(
                !stage.contains("happy_path_pty")
                    && !stage.contains("pty_onboarding_auth_drives_required_screens"),
                "flow {} must not claim phantom PTY stage: {stage}",
                flow["id"]
            );
            assert!(
                stage.contains("pty_smoke_starts_accepts_input_resizes_and_exits")
                    || stage.contains("pty_connect_auth_drives_provider_connection")
                    || stage.contains("pty_permission_overlay_resolves_and_preserves_draft")
                    || stage.contains("pty_status_dialog_opens_without_sidebar_copy")
                    || stage.contains("pty_draft_esc_esc_clears_composer")
                    || stage.contains("harness_tui_pty_e2e"),
                "flow {} must name a real PTY owner: {stage}",
                flow["id"]
            );
        }
    }

    assert_eq!(manifest["reference_image_policy"], "not_required");
    assert!(
        flows
            .iter()
            .all(|flow| flow.get("reference_assets").is_none()),
        "reference-image comparison must not be required"
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

#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn assert_flow_names_test(flows: &[Value], flow_id: &str, test_name: &str) {
    let flow = flows
        .iter()
        .find(|flow| flow["id"] == flow_id)
        .unwrap_or_else(|| {
            let _ = flow_id;
            panic!("abort");
        });
    let deterministic_tests = flow["deterministic_tests"].as_array().unwrap_or_abort();
    assert!(
        deterministic_tests
            .iter()
            .any(|test| test.as_str().is_some_and(|test| test.contains(test_name))),
        "flow {flow_id} must name deterministic owner {test_name}"
    );
}

#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn assert_flow_pty_owner(flows: &[Value], flow_id: &str, pty_test: &str) {
    let flow = flows
        .iter()
        .find(|flow| flow["id"] == flow_id)
        .unwrap_or_else(|| {
            let _ = flow_id;
            panic!("abort");
        });
    let pty_stages = flow["pty_stages"].as_array().unwrap_or_abort();
    assert!(
        pty_stages
            .iter()
            .any(|stage| stage.as_str().is_some_and(|stage| stage.contains(pty_test))),
        "flow {flow_id} must name PTY owner {pty_test}"
    );
}
