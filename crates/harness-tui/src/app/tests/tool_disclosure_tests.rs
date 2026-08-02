use super::*;
use crate::UnwrapOrAbort;

pub(super) fn mouse_click_toggles_transcript_tool_disclosure() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_tool_toggle",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_tool_toggle".into(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Toggle shell tool".to_string(),
            request_digest: "digest-tool-toggle".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_tool_toggle",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_shell_toggle".into(),
            tool_id: "shell.run".to_string(),
            args_summary: r#"{"cmd":"false"}"#.to_string(),
            args_digest: "digest-tool-toggle-args".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_tool_toggle",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_shell_toggle".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("exit code: 1\nstderr: nope".to_string()),
            output_digest: None,
            output_json: None,
            metadata: None,
        }),
    ));

    let (column, row) = transcript_click_position(&app, "$ false");
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    assert!(app
        .transcript_view
        .expanded_tool_outputs
        .contains("tc_shell_toggle"));

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    assert!(!app
        .transcript_view
        .expanded_tool_outputs
        .contains("tc_shell_toggle"));
}
pub(super) fn mouse_click_toggles_apply_patch_file_disclosure() {
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let artifacts_dir = run_dir.path().join("artifacts");
    fs::create_dir_all(&artifacts_dir).unwrap_or_abort();
    fs::write(
        artifacts_dir.join("apply-a.diff"),
        "@@ -1,1 +1,1 @@\n-old a\n+new a\n",
    )
    .unwrap_or_abort();

    let mut app = AppState::new_live(Some(run_dir.path().to_path_buf()), false, None);
    app.ingest_event(envelope(
        1,
        "req_patch_toggle",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_patch_toggle".into(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Toggle patch file".to_string(),
            request_digest: "digest-patch-toggle".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_patch_toggle",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_patch_toggle".into(),
            tool_id: "apply_patch".to_string(),
            args_summary: r#"{"patchText":"*** Begin Patch"}"#.to_string(),
            args_digest: "digest-patch-toggle-args".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_patch_toggle",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_patch_toggle".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("Success. Updated the following files".to_string()),
            output_digest: None,
            output_json: Some(serde_json::json!({
                "files": ["M notes/a.md", "M notes/b.md"],
                "edits": [
                    {
                        "edit_id": "apply-patch-a",
                        "path": "notes/a.md",
                        "summary": "apply patch update notes/a.md",
                        "deleted": false,
                        "diff_rel_path": "artifacts/apply-a.diff",
                        "diff_digest": "digest-apply-a"
                    },
                    {
                        "edit_id": "apply-patch-b",
                        "path": "notes/b.md",
                        "summary": "apply patch update notes/b.md",
                        "deleted": false
                    }
                ]
            })),
            metadata: None,
        }),
    ));

    assert!(!app.patch_file_output_expanded("tc_patch_toggle", "notes/a.md"));
    let (column, row) = transcript_click_position(&app, "Patch");
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    assert!(app
        .transcript_view
        .expanded_tool_outputs
        .contains("tc_patch_toggle"));
    let (column, row) = transcript_click_position(&app, "a.md · notes");
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    assert!(app.patch_file_output_expanded("tc_patch_toggle", "notes/a.md"));

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    assert!(!app.patch_file_output_expanded("tc_patch_toggle", "notes/a.md"));
}

pub(super) fn apply_patch_files_start_collapsed() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_patch_defaults",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_patch_defaults".into(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Seed patch defaults".to_string(),
            request_digest: "digest-patch-defaults".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_patch_defaults",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_patch_defaults".into(),
            tool_id: "apply_patch".to_string(),
            args_summary: r#"{"patchText":"*** Begin Patch"}"#.to_string(),
            args_digest: "digest-patch-defaults-args".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_patch_defaults",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_patch_defaults".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("Success. Updated the following files".to_string()),
            output_digest: None,
            output_json: Some(serde_json::json!({
                "files": ["M notes/a.md", "D notes/old.md"],
                "edits": [
                    {
                        "edit_id": "apply-patch-a",
                        "path": "notes/a.md",
                        "summary": "apply patch update notes/a.md",
                        "deleted": false
                    },
                    {
                        "edit_id": "apply-patch-old",
                        "path": "notes/old.md",
                        "summary": "apply patch delete notes/old.md",
                        "deleted": true
                    }
                ]
            })),
            metadata: None,
        }),
    ));

    assert!(!app.patch_file_output_expanded("tc_patch_defaults", "notes/a.md"));
    assert!(!app.patch_file_output_expanded("tc_patch_defaults", "notes/old.md"));
}
