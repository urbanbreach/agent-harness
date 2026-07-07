use harness_tools::UnwrapOrAbort;
use std::fs;

mod common;

use common::{setup_workspace_fixture, test_context};
use harness_core::{config::ShellAllowlist, edit::hashline::compute_line_hash};
use harness_tools::coordinator_registry;
use serde_json::json;

#[tokio::test]
async fn read_truncation_names_full_artifact_and_next_window() {
    // arrange
    let workspace = setup_workspace_fixture();
    fs::write(
        workspace.workspace().join("fixture.txt"),
        "alpha\nbeta\ngamma\ndelta\n",
    )
    .unwrap_or_abort();
    let context = test_context(
        workspace.workspace(),
        "run-read-truncation-presentation",
        "toolcall-read-truncated",
    );
    let registry = coordinator_registry(ShellAllowlist::default());

    // act
    let result = registry
        .get("read")
        .unwrap_or_abort()
        .call(
            context,
            json!({
                "filePath": "fixture.txt",
                "limit": 2
            }),
        )
        .await
        .unwrap_or_abort();

    // assert
    assert_eq!(result.artifacts.len(), 1);
    assert!(result.display_text.contains("<type>file</type>"));
    assert!(result.display_text.contains("full output artifact:"));
    assert!(result.display_text.contains("Use offset=3 to continue"));

    let metadata = result.structured_json.unwrap_or_abort();
    assert_eq!(metadata["truncated"], json!(true));
    assert_eq!(metadata["next_offset"], json!(3));
    assert_eq!(
        metadata["output_artifact"]["path"],
        json!(result.artifacts[0].path)
    );
}

#[tokio::test]
async fn read_byte_cap_spills_many_lines_to_artifact() {
    // arrange
    let workspace = setup_workspace_fixture();
    let huge_line = "x".repeat(2000);
    let huge = (0..30)
        .map(|_| huge_line.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(workspace.workspace().join("huge.txt"), format!("{huge}\n")).unwrap_or_abort();
    let context = test_context(
        workspace.workspace(),
        "run-read-byte-cap",
        "toolcall-read-byte-cap",
    );
    let artifacts_dir = context.artifacts_dir.clone();
    let registry = coordinator_registry(ShellAllowlist::default());

    // act
    let result = registry
        .get("read")
        .unwrap_or_abort()
        .call(context, json!({"filePath": "huge.txt"}))
        .await
        .unwrap_or_abort();

    // assert
    assert_eq!(result.artifacts.len(), 1);
    assert!(result.display_text.contains("<type>file</type>"));
    assert!(result.display_text.contains("full output artifact:"));
    assert!(result.display_text.len() <= 55 * 1024);
    let metadata = result.structured_json.unwrap_or_abort();
    assert_eq!(metadata["truncated"], json!(true));
    assert_eq!(
        metadata["output_artifact"]["path"],
        json!(result.artifacts[0].path)
    );
    let relative_artifact = result.artifacts[0]
        .path
        .strip_prefix("artifacts/")
        .unwrap_or_abort();
    let artifact_text = fs::read_to_string(artifacts_dir.join(relative_artifact)).unwrap_or_abort();
    assert!(artifact_text.contains(&huge_line));
}

#[tokio::test]
async fn read_hashline_spill_artifact_hashes_source_line_for_truncated_visible_line() {
    // arrange
    let workspace = setup_workspace_fixture();
    let prefix = "x".repeat(2000);
    let long_line = format!("{prefix}tail");
    let visible_line = format!("{prefix}... (line truncated to 2000 chars)");
    fs::write(
        workspace.workspace().join("hashline-huge.txt"),
        format!("{long_line}\nsecond\n"),
    )
    .unwrap_or_abort();
    let context = test_context(
        workspace.workspace(),
        "run-read-hashline-spill",
        "toolcall-read-hashline-spill",
    );
    let artifacts_dir = context.artifacts_dir.clone();
    let registry = coordinator_registry(ShellAllowlist::default());

    // act
    let result = registry
        .get("read")
        .unwrap_or_abort()
        .call(
            context,
            json!({
                "filePath": "hashline-huge.txt",
                "hashline_anchors": true,
                "limit": 1,
            }),
        )
        .await
        .unwrap_or_abort();

    // assert
    assert_eq!(result.artifacts.len(), 1);
    let relative_artifact = result.artifacts[0]
        .path
        .strip_prefix("artifacts/")
        .unwrap_or_abort();
    let artifact_text = fs::read_to_string(artifacts_dir.join(relative_artifact)).unwrap_or_abort();
    assert!(artifact_text.contains(&format!(
        "1#{}|{visible_line}",
        compute_line_hash(&long_line)
    )));
    assert!(!artifact_text.contains(&format!(
        "1#{}|{visible_line}",
        compute_line_hash(&visible_line)
    )));
}
