use harness::UnwrapOrAbort;
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
            config_path.to_str().unwrap_or_abort(),
            "doctor",
            "--json",
        ])
        .output()
        .unwrap_or_abort();

    // assert
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_abort();
    let tool_check = report["checks"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .find(|check| check["name"] == "native_tool_catalog")
        .unwrap_or_abort();

    assert_eq!(tool_check["status"], "pass");
    assert_eq!(
        tool_check["details"]["catalog_source"],
        "harness_tools::tool_catalog"
    );
    assert_eq!(
        tool_check["details"]["readiness"]["background_cancel"],
        true
    );
    assert_eq!(
        tool_check["details"]["readiness"]["ast_grep_search"],
        true
    );
    assert_eq!(
        tool_check["details"]["readiness"]["ast_grep_replace"],
        "shipped_edit_safe"
    );

    let tools = tool_check["details"]["tools"]
        .as_array()
        .unwrap_or_abort();
    for tool_id in [
        "session_list",
        "session_read",
        "session_search",
        "session_info",
        "background_cancel",
        "ast_grep_search",
        "ast_grep_replace",
    ] {
        assert!(
            tools.iter().any(|tool| tool["canonical_id"] == tool_id),
            "missing native tool catalog entry {tool_id}"
        );
    }

    let session_info = tools
        .iter()
        .find(|tool| tool["canonical_id"] == "session_info")
        .unwrap_or_abort();
    assert_eq!(session_info["artifact_behavior"], serde_json::json!("spills_large_output"));
}
