use super::*;

fn start_wait(app: &mut AppState, seq: u64, id: &str) {
    app.ingest_event(envelope(
        seq,
        "req_parent",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: id.into(),
            tool_id: "wait_commands_or_subagents".into(),
            args_summary: r#"{"task_ids":["child","command"],"mode":"wait_all"}"#.into(),
            args_digest: "digest".into(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        seq + 1,
        "req_parent",
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: id.into(),
        }),
    ));
}

fn finish_wait(app: &mut AppState, seq: u64, id: &str, results: [serde_json::Value; 2]) {
    app.ingest_event(envelope(
        seq,
        "req_parent",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: id.into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("=== Multi-wait (wait_all) ===".into()),
            output_digest: None,
            output_json: Some(serde_json::json!({"MultiResult": {
                "mode": "wait_all", "summary": "", "results": results
            }})),
            metadata: None,
        }),
    ));
}

fn result(id: &str, status: &str, output: &str) -> serde_json::Value {
    let command = if id == "child" {
        "[subagent:explore] Inspect files"
    } else {
        "cargo build"
    };
    serde_json::json!({
        "task_id": id, "command": command, "status": status,
        "exit_code": (status == "failed").then_some(1),
        "started": "", "duration_secs": 2.0, "output": output,
        "output_file": "", "truncated": false
    })
}

/// A parent with a running background subagent `child` and command `command`.
fn app_with_running_jobs(session_path: Option<std::path::PathBuf>) -> AppState {
    let mut app = AppState::new_live(session_path, false, None);
    for event in [
        run_started(1),
        agent_spawned(2, "parent", "default"),
        provider_started(3, "req_parent", "mock", "parent-model"),
        envelope(
            4,
            "req_parent",
            EventV1::NativeSubagentRegistered(Box::new(registration())),
        ),
        envelope(
            5,
            "req_child",
            EventV1::SubagentTransition(Box::new(transition(SubagentTransitionKind::Spawned))),
        ),
        envelope(
            6,
            "req_parent",
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "command".into(),
                state: TaskScheduleState::Started,
                queue_key: Some("command".into()),
                metadata: None,
            }),
        ),
    ] {
        app.ingest_event(event);
    }
    app
}

#[test]
fn wait_card_lists_awaited_jobs_and_a_later_wait_replaces_a_still_running_poll() {
    // Given: a background subagent and a background command, both running.
    let mut app = app_with_running_jobs(None);
    let running = || {
        [
            result("child", "running", ""),
            result("command", "running", "Compiling"),
        ]
    };

    // When: the parent waits, the wait returns with both still running, and it waits again.
    start_wait(&mut app, 7, "wait-1");
    let live = render_text(&app, 120, 40);
    finish_wait(&mut app, 9, "wait-1", running());
    start_wait(&mut app, 10, "wait-2");
    let polled = render_text(&app, 120, 40);

    // Then: the live card lists both jobs and the second wait replaces the first.
    assert!(live.contains("Waiting on 2 jobs"), "{live}");
    assert!(live.contains("[subagent] child"), "{live}");
    assert!(live.contains("[bash] command"), "{live}");
    assert_eq!(polled.matches("Waiting on 2 jobs").count(), 1, "{polled}");

    // When: the second wait settles with one success and one failure, then a third wait polls.
    finish_wait(
        &mut app,
        12,
        "wait-2",
        [
            result("child", "completed", "Found 3 files"),
            result("command", "failed", "error[E0425]: cannot find value"),
        ],
    );
    start_wait(&mut app, 13, "wait-3");
    finish_wait(&mut app, 15, "wait-3", running());
    let settled = render_text(&app, 120, 40);

    // Then: the settled card stays, failures sort first, and each row previews its output.
    assert!(
        settled.contains("2 jobs settled · 1 done · 1 failed"),
        "{settled}"
    );
    let failed = settled.find("exit 1 [bash] command").unwrap_or_abort();
    let done = settled.find("[subagent] child").unwrap_or_abort();
    assert!(failed < done, "{settled}");
    assert!(
        settled.contains("error[E0425]: cannot find value"),
        "{settled}"
    );
    assert!(settled.contains("Found 3 files"), "{settled}");
}

#[test]
fn live_wait_card_clock_ticks_between_events_and_its_subagent_rows_open_the_child() {
    // Given: a frame prepared for a running wait over a subagent and a command.
    let temp = tempfile::tempdir().unwrap_or_abort();
    let mut app = app_with_running_jobs(Some(temp.path().join("parent")));
    app.restart_motion_epoch_for_evidence();
    start_wait(&mut app, 7, "wait-1");
    app.set_frame_area(TEST_FRAME_AREA);

    // When: only time passes, then the subagent row is clicked.
    app.advance_wall_clock_for_motion_evidence(std::time::Duration::from_millis(1_500));
    app.set_frame_area(TEST_FRAME_AREA);
    let screen = render_text(&app, 140, 40);
    let (row, line) = screen
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("[subagent] child"))
        .unwrap_or_abort();
    let column = line[..line.find("[subagent]").unwrap_or_abort()]
        .chars()
        .count();
    let (column, row) = (
        u16::try_from(column).unwrap_or_abort(),
        u16::try_from(row).unwrap_or_abort(),
    );
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        app.handle_mouse(
            MouseEvent {
                kind,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            },
            TEST_FRAME_AREA,
            None,
            None,
            None,
        );
    }

    // Then: the prepared row shows the new elapsed time and the click opens the child.
    assert!(line.contains("1.5s"), "{line}");
    assert_eq!(app.current_session_id(), Some("child"));
}
