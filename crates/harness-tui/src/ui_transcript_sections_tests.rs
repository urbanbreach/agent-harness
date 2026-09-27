use super::*;

fn successful_edit(id: &str, path: &str) -> crate::app::ToolCallEntry {
    let mut call = transcript_section_model_test_tool_call(id, "edit");
    call.canonical_tool_id = Some("edit".to_string());
    call.args_summary = serde_json::json!({
        "filePath": path,
        "oldString": "old\n",
        "newString": "new\n"
    })
    .to_string();
    call.status = ToolCallDisplayStatus::Succeeded;
    call
}

#[test]
fn live_completion_rail_requires_open_content_and_settles_for_replay_or_reduced_motion() {
    let base = std::time::Instant::now();
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let mut app = AppState::new_live(Some(run_dir.path().to_path_buf()), false, None);
    app.set_now_fn_for_test(std::sync::Arc::new(move || base));
    let mut activity =
        transcript_section_model_test_activity("completion", ActivityStatus::Streaming, "");
    let mut tool = transcript_section_model_test_tool_call("completion-edit", "bash");
    tool.args_summary = r#"{"command":"cargo check"}"#.to_string();
    tool.output_summary = Some("check completed".to_string());
    tool.status = ToolCallDisplayStatus::Running;
    activity.tool_calls.push(tool);
    app.activities = std::collections::VecDeque::from([activity]);

    app.sync_transcript_state(true);
    app.activities[0].tool_calls[0].status = ToolCallDisplayStatus::Succeeded;
    app.sync_transcript_state(true);
    assert_eq!(
        app.tool_finish_elapsed("completion-edit"),
        Some(std::time::Duration::ZERO)
    );

    let motion = |app: &AppState| {
        build_transcript_sections(app)[0]
            .assistant_tools()
            .next()
            .unwrap_or_abort()
            .rail_motion
    };
    assert_eq!(motion(&app), ToolRailMotion::Settled);
    app.toggle_tool_output_for_test("completion-edit");
    assert!(matches!(motion(&app), ToolRailMotion::FinishFlash { .. }));
    app.replay_mode = true;
    assert_eq!(motion(&app), ToolRailMotion::Settled);
    app.replay_mode = false;
    app.set_reduced_motion(true);
    assert_eq!(motion(&app), ToolRailMotion::Settled);
    app.set_reduced_motion(false);
    assert_eq!(motion(&app), ToolRailMotion::Settled);
}

#[test]
fn same_file_coalescing_accepts_only_trusted_successful_adjacent_edits() {
    // arrange
    let first = successful_edit("edit-1", "src/lib.rs");
    let second = successful_edit("edit-2", "src/lib.rs");
    assert!(safe_same_file_edit_pair(&first, &second));
    let mut failed = successful_edit("edit-3", "src/lib.rs");
    failed.status = ToolCallDisplayStatus::Failed;
    assert!(!safe_same_file_edit_pair(&second, &failed));
    assert!(!safe_same_file_edit_pair(
        &second,
        &successful_edit("edit-4", "src/main.rs")
    ));

    // act
    let mut first_write = successful_edit("write-1", "src/lib.rs");
    first_write.tool_id = "fs.write".to_string();
    first_write.canonical_tool_id = Some("fs.write".to_string());
    let mut duplicate_write = first_write.clone();
    duplicate_write.tool_call_id = "write-2".to_string();
    // assert
    assert!(safe_same_file_edit_pair(&first_write, &duplicate_write));
    duplicate_write.args_summary.push(' ');
    assert!(!safe_same_file_edit_pair(&first_write, &duplicate_write));
}

#[test]
fn live_duplicate_writes_coalesce_under_first_identity_and_expand_as_a_group() {
    // arrange
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let mut app = AppState::new_live(Some(run_dir.path().to_path_buf()), false, None);
    let mut activity =
        transcript_section_model_test_activity("request-live-diff", ActivityStatus::Streaming, "");
    let mut writes = Vec::new();
    for index in 0..3 {
        let mut write = successful_edit(&format!("write-{index}"), "demo.txt");
        write.tool_id = "fs.write".to_string();
        write.canonical_tool_id = Some("fs.write".to_string());
        write.args_summary =
            r#"{"path":"demo.txt","content":"consistency-diff-ok\n","oldContent":"old content\n"}"#
                .to_string();
        writes.push(write);
    }
    activity.tool_calls = writes;
    app.activities = std::collections::VecDeque::from([activity]);

    assert!(build_transcript_sections(&app)[0]
        .assistant_tools()
        .all(|tool| tool.details_visible()));
    app.toggle_tool_output_for_test("write-0");

    let collapsed = build_transcript_sections(&app);
    let collapsed_tools = collapsed[0].assistant_tools().collect::<Vec<_>>();
    assert_eq!(collapsed_tools.len(), 1);
    assert_eq!(collapsed_tools[0].tool_call_id, "write-0");
    assert_eq!(
        collapsed_tools[0].coalesced_tool_call_ids,
        ["write-0", "write-1", "write-2"]
    );
    assert_eq!(collapsed_tools[0].header.title, "Creating");
    assert_eq!(
        collapsed_tools[0].header.path_metadata.as_deref(),
        Some("demo.txt")
    );
    assert_eq!(collapsed_tools[0].header.subtitle, None);
    assert!(!collapsed_tools[0].details_visible());

    // act
    app.focus = crate::app::Focus::Details;
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Down,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert!(app.toggle_selected_transcript_fold());
    let expanded = build_transcript_sections(&app);
    let expanded_tools = expanded[0].assistant_tools().collect::<Vec<_>>();
    // assert
    assert!(expanded_tools[0].details_visible());
    assert_eq!(expanded_tools[0].detail_blocks.len(), 1);
    assert_eq!(expanded_tools[0].header.title, "Creating");
    assert_eq!(expanded_tools[0].header.subtitle, None);
}

#[test]
fn tool_lifecycle_upgrades_diff_highlight_without_replacing_identity_or_content() {
    // arrange
    // act
    let mut blocks = vec![TranscriptToolCallDetailBlock::StructuredDiff {
        before_source: None,
        diff_content: "--- src/lib.rs\n+++ src/lib.rs\n@@ -1 +1 @@\n-old\n+new\n".to_string(),
        fallback_path: Some("src/lib.rs".to_string()),
        force_stacked: false,
        plain_numbered: false,
        highlight_syntax: false,
        show_file_header: false,
    }];
    let before = blocks.clone();
    super::super::ui_transcript_tool_sections::set_diff_highlight_phase(&mut blocks, true);
    let TranscriptToolCallDetailBlock::StructuredDiff {
        before_source: None,
        diff_content: before_text,
        fallback_path: before_path,
        ..
    } = &before[0]
    else {
        panic!("structured before block");
    };
    let TranscriptToolCallDetailBlock::StructuredDiff {
        before_source: None,
        diff_content: after_text,
        fallback_path: after_path,
        highlight_syntax,
        ..
    } = &blocks[0]
    else {
        panic!("structured after block");
    };
    // assert
    assert!(*highlight_syntax);
    assert_eq!(before_text, after_text);
    assert_eq!(before_path, after_path);
}
