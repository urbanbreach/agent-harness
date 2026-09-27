use super::*;
use crate::app::{PermissionEntry, ToolCallDisplayStatus, ToolCallPresentationStatus};
use crate::ui::ui_transcript_test_helpers::transcript_section_model_test_tool_call;

fn section(tool_call: &crate::app::ToolCallEntry) -> TranscriptToolCallSection {
    build_transcript_tool_call_section(
        tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    )
}

#[test]
fn static_tool_headers_preserve_recorded_identity_and_ranges() {
    // Given: native and MCP calls with recorded, displayable metadata.
    let cases = [
        (
            "read",
            r#"{"path":"src/main.rs","offset":42,"limit":20}"#,
            "Read",
            Some("src/main.rs"),
            Some("(42-61)"),
        ),
        (
            "list",
            r#"{"path":"src"}"#,
            "List",
            Some("src"),
            Some("(3 entries)"),
        ),
        (
            "bash",
            r#"{"command":"cargo test","description":"unit tests"}"#,
            "Run unit tests",
            None,
            None,
        ),
        (
            "mcp.database.query",
            r#"{"sql":"select 1"}"#,
            "Database Query",
            None,
            None,
        ),
    ];
    for (id, args, title, path, subtitle) in cases {
        let mut tool = transcript_section_model_test_tool_call(id, id);
        tool.status = ToolCallDisplayStatus::Succeeded;
        tool.args_summary = args.to_string();
        if id == "list" {
            tool.output_json = Some(serde_json::json!({"entry_count": 3}));
        }

        // When: the stored call is projected without performing tool work.
        let header = section(&tool).header;

        // Then: identity and metadata occupy separate header fields.
        assert_eq!(
            (
                header.title.as_str(),
                header.path_metadata.as_deref(),
                header.subtitle.as_deref()
            ),
            (title, path, subtitle),
            "{id}"
        );
    }
}

#[test]
fn search_markers_use_ascii_catalog_when_requested() {
    // Given: an ASCII-mode app and search rows that normally use Unicode diamonds.
    let mut app = AppState::default();
    app.set_glyph_mode(crate::theme::GlyphMode::Ascii);
    let web = transcript_section_model_test_tool_call("web", "search.web");
    let code = transcript_section_model_test_tool_call("code", "search.code");

    // When: the search markers are projected.
    let markers = [&web, &code].map(|tool_call| {
        build_transcript_tool_call_section(tool_call, &app, None, false, false, false, false, None)
            .header
            .icon
    });

    // Then: both use the catalog's ASCII-safe marker.
    assert_eq!(markers, [Some("*"), Some("*")]);
}

#[test]
fn section_projects_waiting_and_correlated_cancelled_states() {
    // arrange
    let mut waiting = transcript_section_model_test_tool_call("waiting", "bash");
    waiting.status = ToolCallDisplayStatus::PendingPermission;
    assert_eq!(
        section(&waiting).header.presentation.status,
        ToolCallPresentationStatus::Waiting
    );

    // act
    let mut cancelled = transcript_section_model_test_tool_call("cancelled", "background_cancel");
    cancelled.status = ToolCallDisplayStatus::Succeeded;
    cancelled.args_summary = r#"{"request_id":"req-child"}"#.to_string();
    cancelled.output_json = Some(serde_json::json!({
        "request_id": "req-child",
        "status": "cancelled"
    }));
    // assert
    assert_eq!(
        section(&cancelled).header.presentation.status,
        ToolCallPresentationStatus::Cancelled
    );
}

#[test]
fn section_preserves_terminal_metadata_and_disclosure_modes() {
    // arrange
    let mut tool_call = transcript_section_model_test_tool_call("generic", "custom.tool");
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.output_summary = Some("result body".to_string());
    tool_call.output_json = Some(serde_json::json!({ "result_count": 3 }));
    tool_call.timing_elapsed_ms = Some(850);

    // act
    let collapsed = section(&tool_call);
    let preview = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        true,
        false,
        false,
        None,
    );
    let expanded = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        true,
        false,
        None,
    );

    // assert
    assert_eq!(collapsed.header.presentation.duration_ms, Some(850));
    assert_eq!(collapsed.header.presentation.result_count, Some(3));
    assert_eq!(
        collapsed.header.disclosure_state,
        Some(TranscriptToolCallDisclosureState::Collapsed)
    );
    assert!(preview.details_preview_visible);
    assert_eq!(
        expanded.header.disclosure_state,
        Some(TranscriptToolCallDisclosureState::Expanded)
    );
}

#[test]
fn resolved_question_renders_numbered_question_and_answer_pairs() {
    // arrange
    // Given: a completed native question call with one answer and one omission.
    let mut tool_call = transcript_section_model_test_tool_call("question", "question");
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.args_summary =
        r#"{"questions":[{"question":"Pick one"},{"question":"Pick two"}]}"#.to_string();
    tool_call.permissions.push(PermissionEntry {
        permission_id: "permission".to_string(),
        kind: "question".to_string(),
        tool_call_id: Some(tool_call.tool_call_id.clone()),
        summary: tool_call.args_summary.clone(),
        request_digest: "digest".to_string(),
        timeout_ms: 30_000,
        default_decision: harness_core::event::PermissionDecision::Deny,
        resolved_decision: Some(harness_core::event::PermissionDecision::Allow),
        resolution_reason: Some(r#"[["Alpha"],[]]"#.to_string()),
        first_seq: 2,
        last_seq: 3,
    });

    // When: the transcript section is projected.
    let rendered = section(&tool_call);

    // act
    // assert
    assert_eq!(rendered.header.title, "Asked 2 questions");
    assert_eq!(
        rendered.detail_blocks,
        vec![TranscriptToolCallDetailBlock::Message {
            text: "  1. Pick one\n     → Alpha\n  2. Pick two\n     → (no answer)".to_string(),
            tone: TranscriptToolCallDetailTone::Primary,
        }]
    );
}
