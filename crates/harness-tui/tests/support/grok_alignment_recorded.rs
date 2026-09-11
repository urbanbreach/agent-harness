//! Recorded lifecycle geometry through the public ingestion/render path.
use super::{persist_frame, render, text};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, LiveEventEnvelope, LiveEventV1,
    PermissionDecision, PermissionRequestedEvent, PermissionResolvedEvent,
    ProviderRequestStartedEvent, RuntimeEvent, TaskCancelledEvent, TaskCompletedEvent,
    TaskCompletionMetadata, TaskLineageMetadata, TaskScheduleMetadata, TaskScheduleState,
    TaskScheduledEvent, TaskTerminalScope, ToolCallFinishedEvent, ToolCallRequestedEvent,
    ToolCallStartedEvent, ToolCallStatus, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_tui::app::AppState;
use harness_tui::UnwrapOrAbort;
use serde_json::{json, Value};
use std::time::Duration;

fn ingest(app: &mut AppState, next_seq: &mut u64, payload: EventV1) {
    let seq = *next_seq;
    *next_seq += 1;
    app.ingest_event(EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("alignment-{seq}"),
        seq,
        run_id: "alignment".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::Worker, Some("worker".into())),
        correlation_id: Some("turn".into()),
        causation_id: None,
        stream_key: None,
        payload,
    });
}

#[test]
fn all_tool_families_keep_grok_columns_through_streaming_and_disclosure() {
    let cases = [
        (
            "task",
            json!({"description":"Edit temporary tool test file", "subagent_type":"general", "prompt":"Inspect demo.txt", "session_id":"", "task_id":"", "run_in_background":false, "load_skills":[]}),
        ),
        (
            "todowrite",
            json!({"todos":[{"content":"Inspect demo.txt", "status":"in_progress", "priority":"medium"}]}),
        ),
        ("read", json!({"filePath":"demo.txt"})),
        (
            "write",
            json!({"filePath":"demo.txt", "content":"Initial tool test content.\n"}),
        ),
        (
            "edit",
            json!({"filePath":"demo.txt", "oldString":"old\n", "newString":"Initial tool test content.\n"}),
        ),
        (
            "apply_patch",
            json!({"patchText":"*** Begin Patch\n*** Add File: demo.txt\n+Initial tool test content.\n*** End Patch"}),
        ),
        (
            "bash",
            json!({"command":"printf ready", "description":"Check terminal output"}),
        ),
        ("grep", json!({"pattern":"ready", "path":"src"})),
        ("glob", json!({"pattern":"*.rs"})),
        ("list", json!({"path":"src"})),
        ("websearch", json!({"query":"terminal geometry"})),
        ("mcp.fixture.inspect", json!({"query":"terminal geometry"})),
        (
            "question",
            json!({"questions":[{"header":"Choice", "question":"Choose an option", "options":[{"label":"First", "description":"Continue"}]}]}),
        ),
    ];
    for (tool, args) in cases {
        for (width, height) in [(40, 24), (80, 24), (120, 40)] {
            for failed in [false, true] {
                verify_lifecycle(tool, &args, width, height, failed);
            }
        }
    }
}

fn verify_lifecycle(tool: &str, args: &Value, width: u16, height: u16, failed: bool) {
    let session = tempfile::tempdir().unwrap_or_abort();
    std::fs::create_dir(session.path().join("artifacts")).unwrap_or_abort();
    std::fs::write(
        session.path().join("artifacts/alignment.diff"),
        "--- /dev/null\n+++ b/demo.txt\n@@ -0,0 +1 @@\n+Initial tool test content.\n",
    )
    .unwrap_or_abort();
    let mut app = AppState::new_live(Some(session.path().to_path_buf()), false, None);
    let mut next_seq = 1;
    app.restart_motion_epoch_for_evidence();
    ingest(
        &mut app,
        &mut next_seq,
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "turn".into(),
            text: "Inspect the renderer".into(),
        }),
    );
    ingest(
        &mut app,
        &mut next_seq,
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "provider".into(),
            provider_id: "mock".into(),
            model_id: "model".into(),
            prompt_summary: "Inspect the renderer".into(),
            request_digest: "synthetic".into(),
            metadata: None,
        }),
    );
    app.ingest_runtime_event(RuntimeEvent::Live(Box::new(LiveEventEnvelope {
        event_id: "tool-input".into(),
        run_id: "alignment".into(),
        mono_ms: 2,
        ts: None,
        actor: EventActor::new(ActorKind::Worker, Some("worker".into())),
        correlation_id: Some("turn".into()),
        causation_id: None,
        stream_key: None,
        payload: LiveEventV1::ProviderToolInputDelta {
            request_id: "provider".into(),
            tool_call_id: "tool-call".into(),
            delta: "{".into(),
        },
    })));
    let name = tool.replace(['.', '_'], "-");
    capture(&app, width, height, &name, "streaming");
    ingest(
        &mut app,
        &mut next_seq,
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tool-call".into(),
            tool_id: tool.into(),
            args_summary: args.to_string(),
            args_digest: "synthetic".into(),
            metadata: None,
        }),
    );
    capture(&app, width, height, &name, "queued");
    if width == 120 {
        ingest(
            &mut app,
            &mut next_seq,
            EventV1::PermissionRequested(PermissionRequestedEvent {
                permission_id: "permission".into(),
                kind: tool.into(),
                tool_call_id: Some("tool-call".into()),
                summary: if tool == "question" {
                    args.to_string()
                } else {
                    "Allow the fixture tool".into()
                },
                request_digest: "synthetic".into(),
                timeout_ms: 30000,
                default_decision: PermissionDecision::Deny,
            }),
        );
        capture(&app, width, height, &name, "waiting");
        ingest(
            &mut app,
            &mut next_seq,
            EventV1::PermissionResolved(PermissionResolvedEvent {
                permission_id: "permission".into(),
                decision: PermissionDecision::Allow,
                reason: (tool == "question").then(|| "[[\"First\"]]".into()),
            }),
        );
    }
    ingest(
        &mut app,
        &mut next_seq,
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "scheduled-tool".into(),
            state: TaskScheduleState::Started,
            queue_key: None,
            metadata: Some(TaskScheduleMetadata {
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: Some("tool-call".into()),
                    ..Default::default()
                }),
            }),
        }),
    );
    ingest(
        &mut app,
        &mut next_seq,
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tool-call".into(),
        }),
    );
    capture(&app, width, height, &name, "running");
    app.advance_wall_clock_for_motion_evidence(Duration::from_millis(330));
    capture(&app, width, height, &name, "running-tick");
    if failed {
        ingest(
            &mut app,
            &mut next_seq,
            EventV1::TaskCancelled(TaskCancelledEvent {
                task_id: "scheduled-tool".into(),
                reason: "tool argument error: Invalid fixture selector".into(),
                task_scope: Some(TaskTerminalScope::ToolCall),
            }),
        );
    } else {
        ingest(
            &mut app,
            &mut next_seq,
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: "scheduled-tool".into(),
                result_summary: String::new(),
                result_digest: "synthetic".into(),
                metadata: Some(TaskCompletionMetadata {
                    task_scope: Some(TaskTerminalScope::ToolCall),
                    ..Default::default()
                }),
            }),
        );
    }
    ingest(
        &mut app,
        &mut next_seq,
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tool-call".into(),
            status: if failed {
                ToolCallStatus::Failed
            } else {
                ToolCallStatus::Succeeded
            },
            output_summary: Some(
                if failed {
                    "tool argument error: Invalid fixture selector"
                } else {
                    "ready"
                }
                .into(),
            ),
            output_digest: None,
            output_json: (!failed).then(|| match tool {
                "todowrite" => args.clone(),
                "read" => json!({"metadata":{"display":{"text":"ready", "lineStart":1}}}),
                "grep" => json!({"matches":["demo.txt:1:ready"],"total_count":1}),
                "glob" => json!({"paths":["demo.txt"],"total_count":1}),
                "apply_patch" => json!({"edits":[{"path":"demo.txt", "diff_rel_path":"artifacts/alignment.diff"}]}),
                "websearch" => json!({"content":"ready"}),
                _ => json!({"result":"ready"}),
            }),
            metadata: None,
        }),
    );
    let status = if failed { "failed" } else { "succeeded" };
    if tool == "task" && failed && width == 120 {
        let rendered = text(&render(&app, width, height));
        assert!(rendered.contains("· failed"), "{rendered}");
        assert!(!rendered.contains("· cancelled"), "{rendered}");
    }
    capture(&app, width, height, &name, status);
    app.toggle_tool_output_for_test("tool-call");
    capture(&app, width, height, &name, &format!("{status}-open"));
    app.focus = harness_tui::app::Focus::Details;
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    capture(&app, width, height, &name, &format!("{status}-selected"));
    app.focus = harness_tui::app::Focus::Prompt;
    app.toggle_tool_output_for_test("tool-call");
    capture(&app, width, height, &name, &format!("{status}-closed"));
}

fn capture(app: &AppState, width: u16, height: u16, tool: &str, state: &str) {
    let buffer = render(app, width, height);
    assert_eq!(app.canonical_projection_error(), None);
    let rendered = text(&buffer);
    // EntryRenderer places the bullet after one accent cell and two padding
    // cells. Harness's viewport has two outer cells; user and tool markers
    // must share the resulting column, regardless of the tool family/state.
    let marker_columns = buffer
        .content
        .chunks(usize::from(width))
        .filter_map(|row| {
            row.iter()
                .position(|cell| matches!(cell.symbol(), "◆" | "◈" | "›" | "⌄"))
        })
        .collect::<Vec<_>>();
    assert!(
        !marker_columns.is_empty(),
        "missing marker: {tool} {state}\n{rendered}"
    );
    let expected = 5; // Grok LayoutConfig: outer left 2 + accent 1 + block left 2.
    assert!(
        marker_columns.iter().all(|&col| col == expected),
        "misaligned markers: {tool} {state}: {marker_columns:?}\n{rendered}"
    );
    persist_frame(
        app,
        width,
        height,
        &format!("align-{tool}-{state}-{width}x{height}-motion-0ms"),
    );
}
