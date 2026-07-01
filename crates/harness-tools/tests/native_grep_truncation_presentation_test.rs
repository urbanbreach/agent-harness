use std::fs;

mod common;

use common::{setup_workspace_fixture, test_context};
use harness_core::config::ShellAllowlist;
use harness_tools::coordinator_registry;
use serde_json::json;

#[tokio::test]
async fn grep_truncation_names_full_artifact_and_narrowing_guidance() {
    // arrange: more grep matches than the model-facing inline limit can return.
    let workspace = setup_workspace_fixture();
    let fixture = (1..=105)
        .map(|line| format!("MATCH result-{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        workspace.workspace().join("large.txt"),
        format!("{fixture}\n"),
    )
    .expect("write grep fixture");
    let context = test_context(
        workspace.workspace(),
        "run-grep-truncation-presentation",
        "toolcall-grep-truncated",
    );
    let artifacts_dir = context.artifacts_dir.clone();
    let registry = coordinator_registry(ShellAllowlist::default());

    // act: grep truncates inline presentation at the default limit.
    let result = registry
        .get("grep")
        .expect("grep tool")
        .call(
            context,
            json!({
                "pattern": "MATCH",
                "path": "large.txt"
            }),
        )
        .await
        .expect("grep should succeed");

    // assert: inline metadata stays bounded and points to the full rendered output.
    assert_eq!(result.artifacts.len(), 1);
    assert!(result.display_text.contains("full output artifact:"));
    assert!(result.display_text.contains("narrow"));
    assert!(result.display_text.contains("rerun grep"));
    assert!(!result.display_text.contains("result-105"));

    let metadata = result.structured_json.expect("structured grep metadata");
    assert_eq!(metadata["total_count"], json!(105));
    assert_eq!(metadata["returned_count"], json!(100));
    assert_eq!(metadata["truncated_count"], json!(5));
    assert_eq!(metadata["truncated"], json!(true));
    assert_eq!(
        metadata["matches"].as_array().expect("matches array").len(),
        100
    );
    assert_eq!(
        metadata["output_artifact"]["path"],
        json!(result.artifacts[0].path)
    );

    let relative_artifact = result.artifacts[0]
        .path
        .strip_prefix("artifacts/")
        .expect("artifact path prefix");
    let artifact_text =
        fs::read_to_string(artifacts_dir.join(relative_artifact)).expect("read grep artifact");
    assert!(artifact_text.contains("large.txt:105: MATCH result-105"));
}

#[tokio::test]
async fn grep_byte_cap_spills_single_huge_match_to_artifact() {
    // arrange: one matching line larger than the inline byte cap.
    let workspace = setup_workspace_fixture();
    let body = "x".repeat(60 * 1024);
    let sentinel = "HUGE_TAIL";
    let huge = format!("MATCH {body}{sentinel}");
    fs::write(workspace.workspace().join("huge.txt"), format!("{huge}\n"))
        .expect("write huge grep fixture");
    let context = test_context(
        workspace.workspace(),
        "run-grep-byte-cap",
        "toolcall-grep-byte-cap",
    );
    let artifacts_dir = context.artifacts_dir.clone();
    let registry = coordinator_registry(ShellAllowlist::default());

    // act
    let result = registry
        .get("grep")
        .expect("grep tool")
        .call(context, json!({"pattern": "MATCH", "path": "huge.txt"}))
        .await
        .expect("grep should succeed");

    // assert
    assert_eq!(result.artifacts.len(), 1);
    assert!(result.display_text.contains("full output artifact:"));
    assert!(result.display_text.len() <= 55 * 1024);
    let metadata = result.structured_json.expect("structured grep metadata");
    assert_eq!(metadata["truncated"], json!(true));
    assert_eq!(
        metadata["output_artifact"]["path"],
        json!(result.artifacts[0].path)
    );
    let relative_artifact = result.artifacts[0]
        .path
        .strip_prefix("artifacts/")
        .expect("artifact path prefix");
    let artifact_text =
        fs::read_to_string(artifacts_dir.join(relative_artifact)).expect("read grep artifact");
    assert!(artifact_text.contains("huge.txt:1: MATCH"));
    assert!(artifact_text.contains(&sentinel));
    assert!(artifact_text.len() > 60 * 1024);
    assert!(!result.display_text.contains(&sentinel));
}
