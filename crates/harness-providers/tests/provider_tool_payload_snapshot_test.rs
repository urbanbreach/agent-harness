use std::error::Error;

use provider_tool_payload_snapshot_support::{fixture_path, generate_snapshot, Snapshot};

#[path = "support/provider_tool_payload_snapshot_support.rs"]
mod provider_tool_payload_snapshot_support;

#[tokio::test]
async fn provider_tool_payload_snapshots_match_real_registry_profiles() -> Result<(), Box<dyn Error>>
{
    // arrange
    let generated = generate_snapshot().await?;
    assert_snapshot_invariants(&generated);
    let generated_json = serde_json::to_string_pretty(&generated)?;
    assert!(!generated_json.contains("test-key"));

    let snapshot_path = fixture_path("provider_tool_payload_snapshots.v1.json");
    if std::env::var_os("UPDATE_PROVIDER_TOOL_PAYLOAD_SNAPSHOTS").is_some() {
        std::fs::write(&snapshot_path, format!("{generated_json}\n"))?;
        return Ok(());
    }

    let expected = std::fs::read_to_string(&snapshot_path)
        .unwrap_or_else(|err| panic!("read {}: {err}", snapshot_path.display()));
    let expected: Snapshot = serde_json::from_str(&expected)?;

    // act
    let comparison = (generated, expected);

    // assert
    assert_eq!(
        comparison.0, comparison.1,
        "provider tool payload snapshot mismatch"
    );
    Ok(())
}

fn assert_snapshot_invariants(snapshot: &Snapshot) {
    assert_eq!(snapshot.version, 1);
    let names = snapshot
        .profiles
        .iter()
        .map(|profile| profile.profile.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["default", "explore", "general", "mcp"]);

    assert_tool(snapshot, "default", "shell.run", "shell_run");
    assert_tool(snapshot, "default", "github.issue", "github_issue");
    assert_tool(snapshot, "default", "lsp.rename", "lsp_rename");
    assert_tool(snapshot, "default", "apply_patch", "apply_patch");
    assert_no_tool(snapshot, "default", "edit");
    assert_no_tool(snapshot, "default", "write");
    assert_tool(
        snapshot,
        "mcp",
        "mcp.docs.rs.tool.call",
        "mcp_docs_rs_tool_call",
    );

    for profile in &snapshot.profiles {
        assert_eq!(profile.openai.len(), 2);
        for tool in &profile.tools {
            assert_eq!(tool.parameters_type, "object");
            assert_eq!(tool.description_digest.len(), 64);
            assert_eq!(tool.parameters_digest.len(), 64);
        }
        let expected_names = profile
            .tools
            .iter()
            .map(|tool| tool.provider_function_name.clone())
            .collect::<Vec<_>>();
        for request in &profile.openai {
            assert_eq!(request.bearer_token, "<redacted>");
            assert_eq!(request.request_body_digest.len(), 64);
            assert_eq!(request.tool_function_names, expected_names);
        }
    }
}

#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn assert_no_tool(snapshot: &Snapshot, profile_name: &str, canonical_id: &str) {
    let profile = snapshot
        .profiles
        .iter()
        .find(|profile| profile.profile == profile_name)
        .unwrap_or_else(|| panic!("abort"));
    assert!(
        profile
            .tools
            .iter()
            .all(|tool| tool.canonical_id != canonical_id),
        "unexpected tool {canonical_id} in {profile_name}"
    );
}

#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn assert_tool(snapshot: &Snapshot, profile_name: &str, canonical_id: &str, function_name: &str) {
    let profile = snapshot
        .profiles
        .iter()
        .find(|profile| profile.profile == profile_name)
        .unwrap_or_else(|| panic!("abort"));
    let tool = profile
        .tools
        .iter()
        .find(|tool| tool.canonical_id == canonical_id)
        .unwrap_or_else(|| panic!("abort"));
    assert_eq!(tool.provider_function_name, function_name);
}
