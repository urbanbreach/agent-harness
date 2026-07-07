use super::*;
use crate::read_window::READ_DEFAULT_LIMIT;
use crate::test_support::{
    read_spilled_artifact, tool_context as fs_read_context, write_workspace_file,
};
use crate::UnwrapOrAbort;
use harness_core::edit::hashline::compute_line_hash;
use harness_core::tool::{Tool, ToolError, ToolResultContent};
use serde_json::json;

#[path = "tests/read_tool_support.rs"]
mod read_tool;

#[tokio::test]
async fn fs_read_supports_offset_and_limit_with_line_numbers() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    write_workspace_file(
        temp.path(),
        "fixture.txt",
        "line one\nline two\nline three\n",
    );

    let tool = FsReadTool::new(false);

    // act
    let result = tool
        .call(
            fs_read_context(temp.path(), "toolcall-offset-limit"),
            json!({
                "path": "fixture.txt",
                "offset": 2,
                "limit": 2
            }),
        )
        .await
        .unwrap_or_abort();

    // assert
    assert!(result.display_text.contains("<type>file</type>"));
    assert!(result
        .display_text
        .contains("<content>\n2: line two\n3: line three"));
    assert!(result
        .display_text
        .contains("(End of file - total 3 lines)"));
    assert!(result.artifacts.is_empty());

    let metadata = result.structured_json.unwrap_or_abort();
    assert_eq!(metadata["offset"], json!(2));
    assert_eq!(metadata["limit"], json!(2));
    assert_eq!(metadata["total_lines"], json!(3));
    assert_eq!(metadata["truncated"], json!(false));
}

#[tokio::test]
async fn fs_read_can_render_hashline_anchors() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    write_workspace_file(temp.path(), "fixture.txt", "alpha\nbeta\n");

    let tool = FsReadTool::new(false);

    // act
    let result = tool
        .call(
            fs_read_context(temp.path(), "toolcall-hashline-read"),
            json!({
                "path": "fixture.txt",
                "hashline_anchors": true,
            }),
        )
        .await
        .unwrap_or_abort();

    // assert
    assert!(result.display_text.contains("<type>file</type>"));
    assert!(result.display_text.contains(&format!(
        "1#{}|alpha\n2#{}|beta",
        compute_line_hash("alpha"),
        compute_line_hash("beta")
    )));

    let metadata = result.structured_json.unwrap_or_abort();
    assert_eq!(metadata["hashline_anchors"], json!(true));
    assert_eq!(metadata["anchors"][0]["line"], json!(1));
    assert_eq!(
        metadata["anchors"][0]["hash"],
        json!(compute_line_hash("alpha"))
    );
    assert_eq!(metadata["anchors"][0]["text"], json!("alpha"));
    assert_eq!(metadata["anchors"][1]["line"], json!(2));
    assert_eq!(
        metadata["anchors"][1]["hash"],
        json!(compute_line_hash("beta"))
    );
    assert_eq!(metadata["anchors"][1]["text"], json!("beta"));
}

#[tokio::test]
async fn fs_read_strips_crlf_line_endings() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    write_workspace_file(temp.path(), "fixture.txt", "alpha\r\nbeta\r\n");

    let tool = FsReadTool::new(false);

    // act
    let result = tool
        .call(
            fs_read_context(temp.path(), "toolcall-crlf-read"),
            json!({
                "path": "fixture.txt",
            }),
        )
        .await
        .unwrap_or_abort();

    // assert
    assert!(result.display_text.contains("<content>\n1: alpha\n2: beta"));
}

#[tokio::test]
async fn fs_read_normalizes_zero_offset_and_limit_to_defaults() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    write_workspace_file(
        temp.path(),
        "fixture.txt",
        "line one\nline two\nline three\n",
    );

    let tool = FsReadTool::new(false);

    // act
    let result = tool
        .call(
            fs_read_context(temp.path(), "toolcall-zero-paging"),
            json!({
                "path": "fixture.txt",
                "offset": 0,
                "limit": 0
            }),
        )
        .await
        .unwrap_or_abort();

    // assert
    assert!(result
        .display_text
        .contains("<content>\n1: line one\n2: line two\n3: line three"));

    let metadata = result.structured_json.unwrap_or_abort();
    assert_eq!(metadata["offset"], json!(1));
    assert_eq!(metadata["limit"], json!(READ_DEFAULT_LIMIT));
    assert_eq!(metadata["hashline_anchors"], json!(false));
}

#[tokio::test]
async fn fs_read_rejects_offset_beyond_end_of_file() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    write_workspace_file(temp.path(), "fixture.txt", "alpha\nbeta\ngamma\n");

    let tool = FsReadTool::new(false);

    // act
    let error = tool
        .call(
            fs_read_context(temp.path(), "toolcall-offset-eof"),
            json!({
                "path": "fixture.txt",
                "offset": 4,
            }),
        )
        .await
        .expect_err("fs.read should reject offsets beyond EOF");

    // assert
    match error {
        ToolError::Execution(message) => {
            assert_eq!(message, "Offset 4 is out of range for this file (3 lines)")
        }
        other => panic!("unexpected error variant: {other}"),
    }
}

#[tokio::test]
async fn fs_read_truncates_lines_longer_than_baseline_limit() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let prefix = "a".repeat(2000);
    let long_line = format!("{prefix}tail");
    let expected = format!("{prefix}... (line truncated to 2000 chars)");
    write_workspace_file(temp.path(), "fixture.txt", format!("{long_line}\n"));

    let tool = FsReadTool::new(false);

    // act
    let result = tool
        .call(
            fs_read_context(temp.path(), "toolcall-long-line"),
            json!({
                "path": "fixture.txt",
            }),
        )
        .await
        .unwrap_or_abort();

    // assert
    assert!(result.display_text.contains(&format!("1: {expected}")));
    assert!(!result.display_text.contains(&long_line));

    let metadata = result.structured_json.unwrap_or_abort();
    assert_eq!(metadata["metadata"]["preview"], json!(expected));
    assert_eq!(metadata["metadata"]["display"]["text"], json!(expected));
}

#[tokio::test]
async fn fs_read_adds_truncation_marker_and_spills_full_output_artifact() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    write_workspace_file(temp.path(), "fixture.txt", "alpha\nbeta\ngamma\ndelta\n");

    let context = fs_read_context(temp.path(), "toolcall-truncated");
    let tool = FsReadTool::new(false);

    // act
    let result = tool
        .call(
            context.clone(),
            json!({
                "path": "fixture.txt",
                "limit": 2
            }),
        )
        .await
        .unwrap_or_abort();

    // assert
    assert!(result.display_text.contains("1: alpha\n2: beta"));
    assert!(result.display_text.contains("Showing lines 1-2 of 4"));
    assert!(result.display_text.contains("Use offset=3 to continue"));
    assert!(result.display_text.contains("full output artifact:"));
    assert_eq!(result.artifacts.len(), 1);

    let metadata = result.structured_json.unwrap_or_abort();
    assert_eq!(metadata["truncated"], json!(true));
    assert_eq!(metadata["total_lines"], json!(4));

    let spilled = read_spilled_artifact(&context, &result.artifacts[0].path);
    assert!(spilled.contains("1: alpha"));
    assert!(spilled.contains("4: delta"));
}

#[tokio::test]
async fn fs_read_spill_artifact_hashes_source_line_when_visible_line_is_truncated() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let prefix = "a".repeat(2000);
    let long_line = format!("{prefix}tail");
    let visible_line = format!("{prefix}... (line truncated to 2000 chars)");
    write_workspace_file(temp.path(), "fixture.txt", format!("{long_line}\nsecond\n"));

    let context = fs_read_context(temp.path(), "toolcall-hashline-spill-long-line");
    let tool = FsReadTool::new(false);

    // act
    let result = tool
        .call(
            context.clone(),
            json!({
                "path": "fixture.txt",
                "hashline_anchors": true,
                "limit": 1,
            }),
        )
        .await
        .unwrap_or_abort();

    // assert
    assert_eq!(result.artifacts.len(), 1);
    let spilled = read_spilled_artifact(&context, &result.artifacts[0].path);
    assert!(spilled.contains(&format!(
        "1#{}|{visible_line}",
        compute_line_hash(&long_line)
    )));
    assert!(!spilled.contains(&format!(
        "1#{}|{visible_line}",
        compute_line_hash(&visible_line)
    )));
}

#[tokio::test]
async fn fs_read_rejects_binary_files() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    write_workspace_file(temp.path(), "fixture.bin", [0xff_u8, 0xfe, 0x00]);

    let tool = FsReadTool::new(false);

    // act
    let error = tool
        .call(
            fs_read_context(temp.path(), "toolcall-binary"),
            json!({
                "path": "fixture.bin"
            }),
        )
        .await
        .expect_err("fs.read should fail for binary");

    // assert
    match error {
        ToolError::Execution(message) => assert!(message.contains("Cannot read binary file:")),
        other => panic!("unexpected error variant: {other}"),
    }
}

#[tokio::test]
async fn fs_read_returns_image_attachment_content() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    write_workspace_file(
        temp.path(),
        "pixel.png",
        b"\x89PNG\r\n\x1a\nfixture-image-bytes",
    );

    let tool = FsReadTool::new(false);

    // act
    let result = tool
        .call(
            fs_read_context(temp.path(), "toolcall-image-read"),
            json!({
                "path": "pixel.png"
            }),
        )
        .await
        .unwrap_or_abort();

    // assert
    assert_eq!(result.display_text, "Image read successfully");
    assert_eq!(result.provider_content.len(), 2);
    assert_eq!(
        result.provider_content[0],
        ToolResultContent::text("Image read successfully")
    );
    let ToolResultContent::File { uri, mime, name } = &result.provider_content[1] else {
        panic!("expected image provider file content");
    };
    assert_eq!(mime, "image/png");
    assert_eq!(name.as_deref(), Some("pixel.png"));
    assert!(uri.starts_with("data:image/png;base64,"));

    let metadata = result.structured_json.unwrap_or_abort();
    assert_eq!(
        metadata["metadata"]["preview"],
        json!("Image read successfully")
    );
    assert_eq!(metadata["attachments"][0]["mime"], json!("image/png"));
}

#[tokio::test]
async fn fs_read_returns_pdf_attachment_content() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    write_workspace_file(temp.path(), "doc.pdf", b"%PDF-1.7\nfixture-pdf-bytes");

    let tool = FsReadTool::new(false);

    // act
    let result = tool
        .call(
            fs_read_context(temp.path(), "toolcall-pdf-read"),
            json!({
                "path": "doc.pdf"
            }),
        )
        .await
        .unwrap_or_abort();

    // assert
    assert_eq!(result.display_text, "PDF read successfully");
    assert_eq!(result.provider_content.len(), 2);
    let ToolResultContent::File { uri, mime, name } = &result.provider_content[1] else {
        panic!("expected PDF provider file content");
    };
    assert_eq!(mime, "application/pdf");
    assert_eq!(name.as_deref(), Some("doc.pdf"));
    assert!(uri.starts_with("data:application/pdf;base64,"));
}
