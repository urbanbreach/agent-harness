use super::*;
use crate::UnwrapOrAbort;

pub(super) fn mouse_click_toggles_transcript_tool_disclosure() {
    let mut app = AppState::new_live(None, false, None);
    app.advance_wall_clock_for_motion_evidence(std::time::Duration::ZERO);
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
            status: ToolCallStatus::Failed,
            output_summary: Some("exit code: 1\nstderr: nope".to_string()),
            output_digest: None,
            output_json: None,
            metadata: None,
        }),
    ));

    assert!(!tool_output_is_expanded(&app, "tc_shell_toggle"));

    for (elapsed_ms, expanded) in [
        (10, false),
        (10, true),
        (10, false),
        (10, false),
        (300, false),
        (10, true),
    ] {
        app.advance_wall_clock_for_motion_evidence(std::time::Duration::from_millis(elapsed_ms));
        let (column, row) = transcript_click_position(&app, "false");
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
        assert_eq!(tool_output_is_expanded(&app, "tc_shell_toggle"), expanded);
        assert!(matches!(
            app.selected_transcript_entry().and_then(|entry| entry.target),
            Some(TranscriptMouseTarget::Tool { ref tool_call_id })
                if tool_call_id == "tc_shell_toggle"
        ));
    }
}

pub(super) fn palette_turn_result_commands_override_failed_output_default() {
    let mut app = failed_tool_disclosure_app("req_palette_toggle", "tc_palette_toggle");
    assert!(!tool_output_is_expanded(&app, "tc_palette_toggle"));

    crate::app::palette_controller::dispatch_palette_command(
        &mut app,
        "harness.collapse_turn_results",
    );
    assert!(!tool_output_is_expanded(&app, "tc_palette_toggle"));

    crate::app::palette_controller::dispatch_palette_command(
        &mut app,
        "harness.expand_turn_results",
    );
    assert!(tool_output_is_expanded(&app, "tc_palette_toggle"));
}

pub(super) fn transcript_enter_opens_failed_tool_full_output() {
    for tool_id in [
        "fs.read",
        "grep",
        "glob",
        "list",
        "websearch",
        "webfetch",
        "fixture.inspect",
    ] {
        let mut app =
            failed_tool_disclosure_app_with_tool("req_failed_file", "tc_failed_file", tool_id);
        app.focus = Focus::Details;
        assert!(app.select_transcript_tool("tc_failed_file"));
        app.handle_key(key(KeyCode::Enter));
        if app.transcript_viewer().is_none() {
            app.handle_key(key(KeyCode::Enter));
        }
        let viewer = app.transcript_viewer().unwrap_or_abort();
        assert!(viewer.content().content().contains("nope"), "{tool_id}");
        assert!(!tool_output_is_expanded(&app, "tc_failed_file"));
    }

    let mut app = failed_tool_disclosure_app("req_enter_toggle", "tc_enter_toggle");
    app.focus = Focus::Details;
    assert!(!tool_output_is_expanded(&app, "tc_enter_toggle"));

    app.handle_key(key(KeyCode::Enter));
    assert!(app.transcript_viewer().is_some());
    assert!(!tool_output_is_expanded(&app, "tc_enter_toggle"));

    app.handle_key(key(KeyCode::Esc));
    assert!(app.transcript_viewer().is_none());
    assert!(!tool_output_is_expanded(&app, "tc_enter_toggle"));
}

#[test]
fn group_keyboard_and_mouse_toggle_the_same_disclosure_state() {
    // arrange
    let app = || {
        let mut app = failed_tool_disclosure_app("req_shared_group", "tc_shared_first");
        app.ingest_event(envelope(
            4,
            "req_shared_group",
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_shared_second".into(),
                tool_id: "shell.run".to_string(),
                args_summary: r#"{"cmd":"exit 2"}"#.to_string(),
                args_digest: "digest-tc-shared-second-args".to_string(),
                metadata: None,
            }),
        ));
        app.ingest_event(envelope(
            5,
            "req_shared_group",
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_shared_second".into(),
                status: ToolCallStatus::Failed,
                output_summary: Some("exit code: 2\nstderr: second failed".to_string()),
                output_digest: None,
                output_json: None,
                metadata: None,
            }),
        ));
        for index in 0..10 {
            let tool_call_id = format!("tc_shared_extra_{index}");
            let seq = 6 + index * 2;
            app.ingest_event(envelope(
                seq,
                "req_shared_group",
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: tool_call_id.clone().into(),
                    tool_id: "shell.run".to_string(),
                    args_summary: r#"{"cmd":"true"}"#.to_string(),
                    args_digest: format!("digest-{tool_call_id}"),
                    metadata: None,
                }),
            ));
            app.ingest_event(envelope(
                seq + 1,
                "req_shared_group",
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: tool_call_id.into(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("done".to_string()),
                    output_digest: None,
                    output_json: None,
                    metadata: None,
                }),
            ));
        }
        app
    };
    let mut mouse_app = app();
    mouse_app.advance_wall_clock_for_motion_evidence(std::time::Duration::ZERO);
    let mut keyboard_app = app();
    keyboard_app.focus = Focus::Details;

    // act
    for expanded in [false, true, true, false, false, true] {
        let title = if mouse_app.tool_group_expanded("tc_shared_first") {
            "Ran 12 commands"
        } else {
            "Ran 2 commands"
        };
        let (column, row) = transcript_click_position(&mouse_app, title);
        mouse_app.advance_wall_clock_for_motion_evidence(std::time::Duration::from_millis(10));
        mouse_app.handle_mouse(
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
        assert_eq!(mouse_app.tool_group_expanded("tc_shared_first"), expanded);
    }
    keyboard_app.handle_key(key(KeyCode::Enter));

    // Opening the fold reveals member headers with output still collapsed.
    assert_eq!(
        mouse_app.transcript_view.expanded_tool_groups,
        keyboard_app.transcript_view.expanded_tool_groups
    );
    assert!(mouse_app.tool_group_expanded("tc_shared_first"));
    assert!(mouse_app.transcript_view.expanded_tool_outputs.is_empty());
    assert!(keyboard_app
        .transcript_view
        .expanded_tool_outputs
        .is_empty());
    assert!(render_text(&mouse_app, 120, 40).contains("Ran 12 commands"));
}

fn tool_output_is_expanded(app: &AppState, tool_call_id: &str) -> bool {
    app.activities
        .iter()
        .flat_map(|activity| activity.tool_calls.iter())
        .find(|tool_call| tool_call.tool_call_id == tool_call_id)
        .is_some_and(|tool_call| app.tool_output_expanded(tool_call))
}

fn failed_tool_disclosure_app(request_id: &str, tool_call_id: &str) -> AppState {
    failed_tool_disclosure_app_with_tool(request_id, tool_call_id, "shell.run")
}

fn failed_tool_disclosure_app_with_tool(
    request_id: &str,
    tool_call_id: &str,
    tool_id: &str,
) -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        request_id,
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Toggle failed tool output".to_string(),
            request_digest: format!("digest-{request_id}"),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        request_id,
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: tool_call_id.into(),
            tool_id: tool_id.to_string(),
            args_summary: r#"{"cmd":"false","path":"missing.txt"}"#.to_string(),
            args_digest: format!("digest-{tool_call_id}-args"),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        request_id,
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: tool_call_id.into(),
            status: ToolCallStatus::Failed,
            output_summary: Some("exit code: 1\nstderr: nope".to_string()),
            output_digest: None,
            output_json: None,
            metadata: None,
        }),
    ));
    app
}

pub(super) fn explicit_tool_disclosure_survives_replay_replacement() {
    let events = shell_test_events(
        ToolCallStatus::Succeeded,
        serde_json::json!({
            "command": "printf replay",
            "status": 0,
            "success": true,
            "stdout": "replay output\n",
            "stderr": "",
            "truncated": false
        }),
    );
    let mut app = AppState::new_live(None, false, None);
    for event in events.iter().cloned() {
        app.ingest_event(event);
    }
    app.activate_transcript_mouse_target(TranscriptMouseTarget::Tool {
        tool_call_id: "tc_shell_panel".to_string(),
    });
    let expanded = app
        .activities
        .iter()
        .flat_map(|activity| activity.tool_calls.iter())
        .find(|tool_call| tool_call.tool_call_id == "tc_shell_panel")
        .expect("shell tool call");
    assert!(app.tool_output_expanded(expanded));

    app.replace_events(events);

    let replayed = app
        .activities
        .iter()
        .flat_map(|activity| activity.tool_calls.iter())
        .find(|tool_call| tool_call.tool_call_id == "tc_shell_panel")
        .expect("replayed shell tool call");
    assert!(app.tool_output_expanded(replayed));
}

pub(super) fn context_group_disclosure_preserves_detached_anchor() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_group_anchor",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_group_anchor".into(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Preserve grouped tool anchor".to_string(),
            request_digest: "digest-group-anchor".to_string(),
            metadata: None,
        }),
    ));
    for (seq, tool_call_id, tool_id, args_summary) in [
        (2, "tc_group_read", "read", r#"{"path":"README.md"}"#),
        (4, "tc_group_skill", "skill", r#"{"name":"frontend"}"#),
    ] {
        app.ingest_event(envelope(
            seq,
            "req_group_anchor",
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: tool_call_id.into(),
                tool_id: tool_id.to_string(),
                args_summary: args_summary.to_string(),
                args_digest: format!("digest-{tool_call_id}-args"),
                metadata: None,
            }),
        ));
        app.ingest_event(envelope(
            seq + 1,
            "req_group_anchor",
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: tool_call_id.into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("loaded context".to_string()),
                output_digest: None,
                output_json: None,
                metadata: None,
            }),
        ));
    }
    let body = (1..=80)
        .map(|line| format!("stable transcript line {line}  "))
        .collect::<Vec<_>>()
        .join("\n");
    app.ingest_event(envelope(
        6,
        "req_group_anchor",
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "req_group_anchor".into(),
            delta: body,
        }),
    ));
    app.ingest_event(envelope(
        7,
        "req_group_anchor",
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: "req_group_anchor".into(),
            finish_reason: "done".to_string(),
            output_digest: Some("digest-group-anchor-finished".to_string()),
            usage: None,
            metadata: None,
        }),
    ));
    let _ = render_text(&app, TEST_FRAME_AREA.width, TEST_FRAME_AREA.height);
    app.scroll_transcript_up(12);
    let _ = render_text(&app, TEST_FRAME_AREA.width, TEST_FRAME_AREA.height);
    let anchor_before = app.transcript_view.measured_anchor.get();
    let top_before = app.transcript_view.measured_viewport().top();
    assert!(anchor_before.is_some());

    // act
    app.activate_transcript_mouse_target(TranscriptMouseTarget::ToolGroup {
        tool_call_ids: vec!["tc_group_read".to_string(), "tc_group_skill".to_string()],
    });
    let _ = render_text(&app, TEST_FRAME_AREA.width, TEST_FRAME_AREA.height);

    // assert
    assert!(app.tool_group_expanded("tc_group_read"));
    assert!(app.transcript_view.expanded_tool_outputs.is_empty());
    assert_eq!(app.transcript_view.measured_anchor.get(), anchor_before);
    assert!(app.transcript_view.measured_viewport().top() > top_before);
    app.toggle_tool_output("tc_group_read");
    let target = ui::transcript_navigation_entries(&app, TEST_FRAME_AREA)
        .into_iter()
        .find_map(|entry| {
            entry
                .target
                .filter(|target| matches!(target, TranscriptMouseTarget::ToolGroup { .. }))
        })
        .unwrap_or_abort();
    app.activate_transcript_mouse_target(target);
    assert!(
        !app.tool_group_expanded("tc_group_read"),
        "opening the first member must preserve the group's collapse target"
    );
    assert!(tool_output_is_expanded(&app, "tc_group_read"));
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

    assert!(app.patch_file_output_expanded("tc_patch_toggle", "notes/a.md"));
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
    assert!(!app.patch_file_output_expanded("tc_patch_toggle", "notes/a.md"));

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
}

pub(super) fn apply_patch_default_expansion_skips_deleted_files() {
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

    assert!(app.patch_file_output_expanded("tc_patch_defaults", "notes/a.md"));
    assert!(!app.patch_file_output_expanded("tc_patch_defaults", "notes/old.md"));
}
