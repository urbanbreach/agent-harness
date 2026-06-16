use super::*;

pub(super) fn terminal_panel_is_hidden_by_default_and_toggles_from_keybinding() {
    let mut app = AppState::new_live(None, false, None);
    assert!(!app.terminal_panel_visible());
    assert!(
        crate::layout::FrameLayoutPlan::for_app(&app, TEST_FRAME_AREA)
            .terminal_panel
            .is_none()
    );

    app.handle_key(key(KeyCode::Char('4')));

    assert!(app.terminal_panel_visible());
    assert_eq!(
        app.focus,
        Focus::Prompt,
        "toggle should not steal composer focus"
    );
    assert!(
        crate::layout::FrameLayoutPlan::for_app(&app, TEST_FRAME_AREA)
            .terminal_panel
            .is_some()
    );

    app.handle_key(key(KeyCode::Char('4')));

    assert!(!app.terminal_panel_visible());
}

pub(super) fn terminal_panel_stays_hidden_for_live_bash_until_explicit_toggle() {
    let mut app = AppState::new_live(None, false, None);
    for event in shell_test_events(
        ToolCallStatus::Succeeded,
        serde_json::json!({
            "command": "pwd",
            "status": 0,
            "success": true,
            "stdout": "/home/urbanbreach/code/accela/agent-harness\n",
            "stderr": "",
            "truncated": false
        }),
    ) {
        app.ingest_event(event);
    }

    assert!(!app.terminal_panel_visible());
    assert_eq!(
        app.focus,
        Focus::Prompt,
        "shell output should stay inline without stealing composer focus"
    );
    assert!(
        crate::layout::FrameLayoutPlan::for_app(&app, TEST_FRAME_AREA)
            .terminal_panel
            .is_none(),
        "live shell commands should not create a duplicate terminal panel above the composer"
    );

    app.handle_key(key(KeyCode::Char('4')));
    assert!(app.terminal_panel_visible());

    app.handle_key(key(KeyCode::Char('4')));
    assert!(!app.terminal_panel_visible());

    for event in shell_test_events(
        ToolCallStatus::Succeeded,
        serde_json::json!({
            "command": "pwd",
            "status": 0,
            "success": true,
            "stdout": "/srv/samba/code/accela/agent-harness\n",
            "stderr": "",
            "truncated": false
        }),
    ) {
        let mut event = event;
        event.seq += 10;
        app.ingest_event(event);
    }

    assert!(
        !app.terminal_panel_visible(),
        "later shell commands should also remain inline unless the user toggles the panel"
    );
}

pub(super) fn terminal_panel_extracts_successful_bash_command_output() {
    let mut app = AppState::new_live(None, false, None);
    for event in shell_test_events(
        ToolCallStatus::Succeeded,
        serde_json::json!({
            "command": "cargo test -p harness-tui",
            "workdir": ".",
            "status": 0,
            "success": true,
            "stdout": "ok\nall tests passed\n",
            "stderr": "",
            "truncated": false
        }),
    ) {
        app.ingest_event(event);
    }

    let entries = app.terminal_panel_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].command, "cargo test -p harness-tui");
    assert_eq!(entries[0].stdout.as_deref(), Some("ok\nall tests passed\n"));
    assert_eq!(entries[0].stderr, None);
    assert_eq!(entries[0].exit_code, Some(0));
    assert_eq!(entries[0].duration_ms, Some(250));

    assert!(!app.terminal_panel_visible());
    app.handle_key(key(KeyCode::Char('4')));
    assert!(app.terminal_panel_visible());
    let debug = render_debug(&app, 140, 40);
    assert!(debug.contains("Terminal"));
    assert!(debug.contains("$ cargo test -p harness-tui"));
    assert!(debug.contains("stdout> ok"));
    assert!(debug.contains("exit 0"));
}

pub(super) fn terminal_panel_renders_failed_command_stderr_and_exit_status() {
    let mut app = AppState::new_live(None, false, None);
    for event in shell_test_events(
        ToolCallStatus::Failed,
        serde_json::json!({
            "command": "cargo test -p harness-tui",
            "status": 101,
            "success": false,
            "stdout": "",
            "stderr": "test failed\nassertion failed\n",
            "truncated": true,
            "output_artifact": {"path": "artifacts/toolcalls/tc_shell_panel/shell.output.txt"}
        }),
    ) {
        app.ingest_event(event);
    }
    assert!(!app.terminal_panel_visible());
    app.handle_key(key(KeyCode::Char('4')));
    assert!(app.terminal_panel_visible());

    let debug = render_debug(&app, 140, 40);
    assert!(debug.contains("failed"));
    assert!(debug.contains("exit 101"));
    assert!(debug.contains("stderr> test failed"));
    assert!(debug.contains("output truncated"));
}

pub(super) fn terminal_panel_extracts_shell_run_direct_command_schema() {
    let mut app = AppState::new_live(None, false, None);
    for event in shell_run_test_events(
        ToolCallStatus::Succeeded,
        serde_json::json!({
            "cmd": "bash",
            "args": ["-lc", "printf shell-run"],
            "cwd": ".",
            "status": 0,
            "success": true,
            "stdout": "shell-run",
            "stderr": "",
            "truncated": false
        }),
    ) {
        app.ingest_event(event);
    }

    assert!(!app.terminal_panel_visible());
    let entries = app.terminal_panel_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].command, "bash -lc printf shell-run");
    assert_eq!(entries[0].cwd.as_deref(), Some("."));
    assert_eq!(entries[0].stdout.as_deref(), Some("shell-run"));
    assert_eq!(entries[0].duration_ms, Some(42));
}

pub(super) fn terminal_panel_replay_reconstructs_from_events_without_execution() {
    let mut replay = AppState::new_replay(
        PathBuf::from("/tmp/terminal-panel-replay"),
        shell_test_events(
            ToolCallStatus::Succeeded,
            serde_json::json!({
                "command": "printf replay",
                "status": 0,
                "success": true,
                "stdout": "replay\n",
                "stderr": "",
                "truncated": false
            }),
        ),
    );

    assert_eq!(replay.terminal_panel_entries().len(), 1);
    assert_eq!(replay.terminal_panel_entries()[0].command, "printf replay");
    replay.handle_key(key(KeyCode::Char('4')));

    let debug = render_debug(&replay, 140, 40);
    assert!(debug.contains("Replay · read-only"));
    assert!(debug.contains("$ printf replay"));
    assert!(debug.contains("stdout> replay"));
}

pub(super) fn terminal_panel_focus_scrolls_independently_from_transcript() {
    let mut app = AppState::new_live(None, false, None);
    app.handle_key(key(KeyCode::Char('4')));
    app.focus = Focus::Terminal;
    app.last_terminal_panel_max_scroll.set(20);

    app.handle_key(key(KeyCode::PageUp));
    assert_eq!(app.terminal_panel_scroll(), 10);
    assert!(!app.terminal_panel_follow());
    assert_eq!(app.transcript_scroll, 0);

    app.handle_key(key(KeyCode::End));
    assert_eq!(app.terminal_panel_scroll(), 0);
    assert!(app.terminal_panel_follow());
}
