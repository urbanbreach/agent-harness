#[test]
fn doctor_cli_json_reports_native_tool_catalog_readiness() {
    // arrange
    let repo_root = repo_root();
    let config_path = repo_root.join("configs").join("harness.example.jsonc");

    // act
    let output = harness_command()
        .current_dir(&repo_root)
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "doctor",
            "--json",
        ])
        .output()
        .expect("run harness doctor --json with shipped example config");

    // assert
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("doctor json report");
    let tool_check = report["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .find(|check| check["name"] == "native_tool_catalog")
        .expect("native tool catalog check");

    assert_eq!(tool_check["status"], "pass");
    assert_eq!(
        tool_check["details"]["catalog_source"],
        "harness_tools::tool_catalog"
    );
    assert_eq!(
        tool_check["details"]["readiness"]["background_cancel"],
        true
    );
    assert_eq!(tool_check["details"]["readiness"]["team_list"], true);
    assert_eq!(
        tool_check["details"]["readiness"]["ast_grep_search"],
        true
    );
    assert_eq!(
        tool_check["details"]["readiness"]["ast_grep_replace"],
        "deferred_conditional_stretch"
    );
    assert_eq!(
        tool_check["details"]["readiness"]["team_projection"]["source"],
        "event_replay"
    );
    assert_eq!(
        tool_check["details"]["readiness"]["team_projection"]["no_network_probes"],
        true
    );
    assert!(
        tool_check["details"]["readiness"]["team_projection"]["active_team_count"].is_number()
    );
    let tools = tool_check["details"]["tools"]
        .as_array()
        .expect("catalog tools");
    for tool_id in [
        "session_list",
        "session_read",
        "session_search",
        "session_info",
        "background_cancel",
        "team_list",
        "ast_grep_search",
    ] {
        assert!(
            tools
                .iter()
                .any(|tool| tool["canonical_id"] == tool_id),
            "missing native tool catalog entry {tool_id}"
        );
    }

    let session_info = tools
        .iter()
        .find(|tool| tool["canonical_id"] == "session_info")
        .expect("session_info tool entry");
    assert_eq!(session_info["artifact_behavior"], "spills_large_output");
}
