use std::path::PathBuf;

#[path = "support/deterministic_render_fixtures.rs"]
mod deterministic_render_fixtures;

use harness_core::event::{
    ActorKind, EditAppliedEvent, EventActor, EventEnvelopeV1, EventV1, PermissionRequestedEvent,
    ProviderRequestFinishedEvent, ProviderRequestStartedEvent, ProviderStreamDeltaEvent,
    RunFinishedEvent, RunStartedEvent, ToolCallFinishedEvent, ToolCallMetadata,
    ToolCallRequestedEvent, ToolCallStartedEvent, ToolCallStatus, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use harness_core::proj::{RunStatus, SessionCatalogEntry, SessionModeSource};
use harness_tui::app::{AppState, LaunchMetadata};
use harness_tui::render_test::render_to_string;
use harness_tui::{ui, FrameLayoutPlan};
use ratatui::layout::Rect;

#[test]
fn startup_shell_is_compose_first_without_pty() {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );
    app.prompt_buffer = "Explain deterministic TUI tests".to_string();
    app.prompt_cursor = app.prompt_buffer.len();

    let rendered = render_text(&app, 100, 24);

    insta::assert_snapshot!(rendered.as_str());

    assert!(rendered.contains("Explain deterministic TUI tests"));
    assert!(rendered.contains("Worker model-1 mock"));
    assert!(rendered.contains("ctrl+p commands"));
    assert!(!rendered.contains("Actions:"));
    assert!(!rendered.contains("Tabs"));
    assert!(!rendered.contains("Current runtime:"));
}

#[test]
fn live_transcript_and_operator_sidebar_render_without_pty() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/run_fixture")), false, None);
    for event in sidebar_render_events() {
        app.ingest_event(event);
    }

    let area = Rect::new(0, 0, 160, 30);
    let plan = FrameLayoutPlan::for_app(&app, area);
    assert!(
        plan.transcript.is_some(),
        "live shell keeps transcript primary"
    );
    assert!(
        plan.operator_sidebar.is_some(),
        "wide live shell keeps the operator sidebar persistent"
    );

    let rendered = render_text(&app, area.width, area.height);

    assert!(rendered.contains("Inspect deterministic sidebar"));
    assert!(rendered.contains("Assistant verified the rendered shell."));
    assert_markers_in_order(
        &rendered,
        &["▼ MCP", "▼ LSP", "▼ Modified Files", "src/ui_secondary.rs"],
    );
    assert!(rendered.contains("• websearch Connected"));
    assert!(rendered.contains("• rust"));
    assert!(!rendered.contains("Current runtime:"));
}

#[test]
fn tool_lifecycle_rows_stay_ordered_without_pty() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/run_tool_lifecycle")), false, None);
    for event in deterministic_render_fixtures::tool_lifecycle_events() {
        app.ingest_event(event);
    }

    let rendered = render_text(&app, 180, 36);

    assert_markers_in_order(
        &rendered,
        &[
            "Inspect tool activity",
            "Read src/ui.rs",
            "Loaded src/ui.rs",
            "Remove diff review surface",
            "Researcher Task",
            "audit tool lifecycle parity",
            "2 toolcalls",
            "cargo test -p harness-tui",
            "snapshot mismatch",
            "Tool summaries are now easier to scan, and edits stay inline.",
        ],
    );
    assert!(rendered.contains("Compat alias · read → fs.read"));
    assert!(rendered.contains("artifacts/tool-lifecycle-inline.diff"));
}

#[test]
fn permission_modal_preserves_draft_without_pty() {
    let mut app = AppState::new_live(None, false, None);
    app.prompt_buffer = "keep this draft".to_string();
    app.prompt_cursor = app.prompt_buffer.len();
    app.ingest_event(permission_requested_event(1, "perm_det", "tool_call_det"));

    let rendered = render_text(&app, 100, 28);

    assert!(rendered.contains("Permission required"));
    assert!(rendered.contains("Apply hashline edit to demo.txt"));
    assert!(rendered.contains("Allow once"));
    assert!(rendered.contains("Allow always"));
    assert!(rendered.contains("Reject"));
    assert_eq!(app.prompt_buffer, "keep this draft");
}

#[test]
fn startup_session_history_picker_renders_without_pty() {
    let mut app = AppState::new_startup(startup_session_history_entries(), None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );

    app.execute_slash_command("resume", Some(String::new()));

    let rendered = render_text(&app, 100, 24);

    assert!(rendered.contains("Continue session"));
    assert!(rendered.contains("Search"));
    assert!(rendered.contains("alpha-run"));
    assert!(rendered.contains("continue ready"));
    assert!(!rendered.contains("beta-blocked"));
    assert!(!rendered.contains("run is still active"));
    assert!(!rendered.contains("provider unknown"));
}

#[test]
fn question_permission_prompt_renders_without_pty() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(question_permission_requested_event(
        1,
        "perm_question_det",
        "tool_call_question_det",
    ));

    let rendered = render_text(&app, 100, 28);

    assert!(rendered.contains("Pick one"));
    assert!(rendered.contains("Type your own answer"));
    assert!(rendered.contains("↑↓ select"));
    assert!(rendered.contains("enter submit"));
    assert!(rendered.contains("esc dismiss"));
}

#[test]
fn replay_shell_is_read_only_without_pty() {
    let mut app = AppState::new_replay(PathBuf::from("/tmp/replay_run"), replay_events());

    let rendered = render_text(&app, 100, 24);

    assert!(rendered.contains("Replay · read-only"));
    assert!(rendered.contains("Replay is read-only"));
    assert!(rendered.contains("r reload"));
    assert!(rendered.contains("q quit"));
    assert!(!rendered.contains("Type a prompt for the next turn"));

    app.execute_slash_command("clone", Some("preserved replay draft".to_string()));
    assert_eq!(app.prompt_buffer, "preserved replay draft");
    assert_eq!(
        app.status_banner.as_deref(),
        Some("session clone blocked: replay mode is read-only")
    );
}

fn render_text(app: &AppState, width: u16, height: u16) -> String {
    render_to_string(app, Rect::new(0, 0, width, height), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

fn assert_markers_in_order(screen: &str, markers: &[&str]) {
    let mut last = 0;
    for marker in markers {
        let offset = screen[last..]
            .find(marker)
            .unwrap_or_else(|| panic!("missing marker {marker:?}\n{screen}"));
        last += offset + marker.len();
    }
}

fn sidebar_render_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_sidebar_det";
    vec![
        envelope(
            1,
            Some(request_id),
            EventV1::RunStarted(RunStartedEvent {
                run_name: "deterministic-sidebar".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.to_string(),
                text: "Inspect deterministic sidebar".to_string(),
            }),
        ),
        envelope(
            3,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "Inspect deterministic sidebar".to_string(),
                request_digest: "digest-sidebar-det".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            4,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.to_string(),
                delta: "Assistant verified the rendered shell.".to_string(),
            }),
        ),
        tool_requested(
            5,
            "tool_call_search",
            "search.web",
            serde_json::json!({"query": "harness sidebar"}).to_string(),
        ),
        envelope(
            6,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tool_call_search".to_string(),
            }),
        ),
        tool_finished(
            7,
            "tool_call_search",
            "search.web",
            "Fetched harness sidebar examples",
        ),
        tool_requested(
            8,
            "tool_call_lsp",
            "code.lsp",
            serde_json::json!({
                "operation": "goto_definition",
                "path": "src/ui_secondary.rs"
            })
            .to_string(),
        ),
        envelope(
            9,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tool_call_lsp".to_string(),
            }),
        ),
        tool_finished(
            10,
            "tool_call_lsp",
            "code.lsp",
            "Found definition in src/ui_secondary.rs",
        ),
        envelope(
            11,
            Some(request_id),
            EventV1::EditApplied(EditAppliedEvent {
                edit_id: "edit_sidebar_det".to_string(),
                path: "src/ui_secondary.rs".to_string(),
                new_file_digest: "digest-edit-sidebar-det".to_string(),
                diff_rel_path: None,
                diff_digest: None,
            }),
        ),
        envelope(
            12,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-sidebar-output".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
    ]
}

fn tool_requested(
    seq: u64,
    tool_call_id: &str,
    tool_id: &str,
    args_summary: String,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some("req_sidebar_det"),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: tool_call_id.to_string(),
            tool_id: tool_id.to_string(),
            args_summary,
            args_digest: format!("digest-{tool_call_id}"),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some(tool_id.to_string()),
                ..Default::default()
            }),
        }),
    )
}

fn tool_finished(
    seq: u64,
    tool_call_id: &str,
    tool_id: &str,
    output_summary: &str,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some("req_sidebar_det"),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: tool_call_id.to_string(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some(output_summary.to_string()),
            output_digest: Some(format!("digest-{tool_call_id}-output")),
            output_json: None,
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some(tool_id.to_string()),
                ..Default::default()
            }),
        }),
    )
}

fn permission_requested_event(
    seq: u64,
    permission_id: &str,
    tool_call_id: &str,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(tool_call_id),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: permission_id.to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some(tool_call_id.to_string()),
            summary: "Apply hashline edit to demo.txt".to_string(),
            request_digest: "digest-perm-det".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    )
}

fn question_permission_requested_event(
    seq: u64,
    permission_id: &str,
    tool_call_id: &str,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(tool_call_id),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: permission_id.to_string(),
            kind: "question".to_string(),
            tool_call_id: Some(tool_call_id.to_string()),
            summary: serde_json::json!({
                "questions": [{
                    "question": "Pick one",
                    "header": "Choice",
                    "options": [{"label": "A", "description": "Option A"}],
                    "multiple": false,
                }]
            })
            .to_string(),
            request_digest: "digest-question-perm-det".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    )
}

fn replay_events() -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            1,
            Some("run_fixture"),
            EventV1::RunStarted(RunStartedEvent {
                run_name: "replay-fixture".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            Some("run_fixture"),
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ]
}

fn startup_session_history_entries() -> Vec<harness_tui::app::SessionHistoryEntry> {
    vec![
        harness_tui::app::SessionHistoryEntry {
            run_dir: PathBuf::from("/tmp/sessions/run_resume"),
            catalog: SessionCatalogEntry {
                run_id: "run_resume".to_string(),
                run_name: Some("alpha-run".to_string()),
                status: Some(RunStatus::Finished),
                last_updated_at: Some("2026-03-08T12:34:56Z".to_string()),
                workspace_root: Some("/tmp/workspace".to_string()),
                profile_preset: Some("deep".to_string()),
                provider_model: Some("openai/gpt-5.4-mini".to_string()),
                mode_source: SessionModeSource::InteractiveLive,
                is_resumable: true,
                resume_disabled_reason: None,
                artifact_count: 2,
                child_session_count: 1,
                parent_session_id: None,
            },
        },
        harness_tui::app::SessionHistoryEntry {
            run_dir: PathBuf::from("/tmp/sessions/run_blocked"),
            catalog: SessionCatalogEntry {
                run_id: "run_blocked".to_string(),
                run_name: Some("beta-blocked".to_string()),
                status: Some(RunStatus::Running),
                last_updated_at: Some("2026-03-07T03:21:00Z".to_string()),
                workspace_root: Some("/tmp/workspace".to_string()),
                profile_preset: Some("ops".to_string()),
                provider_model: Some("anthropic/claude-3.7".to_string()),
                mode_source: SessionModeSource::InteractiveLive,
                is_resumable: false,
                resume_disabled_reason: Some("run is still active".to_string()),
                artifact_count: 1,
                child_session_count: 0,
                parent_session_id: Some("run_parent".to_string()),
            },
        },
    ]
}

fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-det-{seq:04}"),
        seq,
        run_id: "run_fixture".to_string(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("deterministic-render".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_fixture".to_string()),
        payload,
    }
}
