use crate::coordinator_registry;
use crate::read_window::READ_DEFAULT_LIMIT;
use crate::test_support::{tool_context as fs_read_context, write_workspace_file};
use crate::UnwrapOrAbort;
use harness_core::config::ShellAllowlist;
use harness_core::edit::hashline::compute_line_hash;
use harness_core::tool::ToolError;
use serde_json::json;

#[tokio::test]
async fn read_tool_normalizes_zero_offset_and_limit_for_model_compatibility() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    write_workspace_file(
        temp.path(),
        "fixture.txt",
        "line one\nline two\nline three\n",
    );

    let registry = coordinator_registry(ShellAllowlist::default());
    let read = registry.get("read").unwrap_or_abort();

    // act
    let result = read
        .call(
            fs_read_context(temp.path(), "toolcall-read-zero-paging"),
            json!({
                "filePath": "fixture.txt",
                "offset": 0,
                "limit": 0
            }),
        )
        .await
        .unwrap_or_abort();

    // assert
    assert!(result.display_text.contains("<type>file</type>"));
    assert!(result.display_text.contains(&format!(
        "1#{}|line one\n2#{}|line two\n3#{}|line three",
        compute_line_hash("line one"),
        compute_line_hash("line two"),
        compute_line_hash("line three")
    )));

    let metadata = result.structured_json.unwrap_or_abort();
    assert_eq!(metadata["offset"], json!(1));
    assert_eq!(metadata["limit"], json!(READ_DEFAULT_LIMIT));
    assert_eq!(metadata["hashline_anchors"], json!(true));
}

#[tokio::test]
async fn read_tool_exposes_hashline_anchor_mode_for_model_workflows() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    write_workspace_file(temp.path(), "fixture.txt", "alpha\nbeta\n");

    let registry = coordinator_registry(ShellAllowlist::default());
    let read = registry.get("read").unwrap_or_abort();

    // act
    let result = read
        .call(
            fs_read_context(temp.path(), "toolcall-read-hashline"),
            json!({
                "filePath": "fixture.txt",
                "hashlineAnchors": true,
            }),
        )
        .await
        .unwrap_or_abort();

    // assert
    assert!(result.display_text.contains("1#"));
    assert!(result.display_text.contains("|alpha"));
    assert!(result.display_text.contains("2#"));
    assert!(result.display_text.contains("|beta"));

    let metadata = result.structured_json.unwrap_or_abort();
    assert_eq!(metadata["hashline_anchors"], json!(true));
    assert_eq!(metadata["anchors"][0]["line"], json!(1));
    assert_eq!(metadata["anchors"][1]["line"], json!(2));
}

#[tokio::test]
async fn read_tool_accepts_absolute_workspace_paths() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let source = write_workspace_file(temp.path(), "fixture.txt", "alpha\nbeta\n");

    let registry = coordinator_registry(ShellAllowlist::default());
    let read = registry.get("read").unwrap_or_abort();

    // act
    let result = read
        .call(
            fs_read_context(temp.path(), "toolcall-read-absolute"),
            json!({
                "filePath": source,
            }),
        )
        .await
        .unwrap_or_abort();

    // assert
    assert!(result.display_text.contains("|alpha"));
    assert!(result.display_text.contains("|beta"));
    let metadata = result.structured_json.unwrap_or_abort();
    assert!(metadata["resolved_path"]
        .as_str()
        .unwrap_or_abort()
        .ends_with("fixture.txt"));
}

#[tokio::test]
async fn read_tool_rejects_absolute_paths_outside_workspace() {
    // arrange
    let workspace = tempfile::tempdir().unwrap_or_abort();
    let outside = tempfile::tempdir().unwrap_or_abort();
    let source = outside.path().join("escape.txt");
    std::fs::write(&source, "blocked\n").unwrap_or_abort();

    let registry = coordinator_registry(ShellAllowlist::default());
    let read = registry.get("read").unwrap_or_abort();

    // act
    let error = read
        .call(
            fs_read_context(workspace.path(), "toolcall-read-absolute-escape"),
            json!({
                "filePath": source,
            }),
        )
        .await
        .expect_err("absolute path outside workspace should be rejected");

    // assert
    assert!(matches!(error, ToolError::PathEscapesWorkspace { .. }));
}
