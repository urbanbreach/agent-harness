use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionDecision as EventPermissionDecision,
    PermissionRequestedEvent, RunFailedEvent, RunStartedEvent, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use harness_core::perm::PermissionDecision;
use harness_tui::app::{AppState, UiIntent};
use harness_tui::render_test::render_to_string;
use harness_tui::{ui, FrameLayoutPlan};
use ratatui::layout::Rect;
use serde_json::json;

#[test]
fn permission_modal_renders_typed_subjects_for_non_edit_tools() {
    for (kind, summary, expected) in [
        (
            "read",
            json!({"path": "README.md"}).to_string(),
            "Read README.md",
        ),
        ("list", json!({"dir": "src"}).to_string(), "List src"),
        (
            "glob",
            json!({"pattern": "**/*.rs"}).to_string(),
            "Glob \"**/*.rs\"",
        ),
        (
            "grep",
            json!({"pattern": "PermissionRequested"}).to_string(),
            "Grep \"PermissionRequested\"",
        ),
        (
            "webfetch",
            json!({"url": "https://example.com/docs"}).to_string(),
            "Fetch https://example.com/docs",
        ),
        (
            "bash",
            json!({"command": "cargo test -p harness-tui"}).to_string(),
            "Run `cargo test -p harness-tui`",
        ),
        ("task", "{}".to_string(), "Run task"),
    ] {
        let mut app = AppState::new_live(None, false, None);
        app.ingest_event(permission_event(1, kind, &summary));

        let rendered = render_text(&app, 120, 34);

        assert!(rendered.contains(expected), "{kind}: {rendered}");
    }
}

#[test]
fn permission_modal_edges_render_wildcard_digest_and_missing_diff_safely() {
    let mut wildcard = AppState::new_live(None, false, None);
    wildcard.ingest_event(permission_event(
        1,
        "bash",
        &json!({"command": "make deploy", "selectors": ["*"]}).to_string(),
    ));

    wildcard.handle_key(key(KeyCode::Right));
    wildcard.handle_key(key(KeyCode::Enter));
    let rendered = render_text(&wildcard, 120, 34);

    assert!(rendered.contains("Allow always"), "{rendered}");
    assert!(
        rendered.contains("run-scoped durable grant for *"),
        "{rendered}"
    );
    assert!(!rendered.contains("Diff preview"), "{rendered}");

    let mut digest = AppState::new_live(None, false, None);
    digest.ingest_event(permission_event(1, "edit_fs", "{not valid json"));
    let rendered = render_text(&digest, 120, 34);

    assert!(rendered.contains("Review edit fs"), "{rendered}");
    assert!(!rendered.contains("Diff preview"), "{rendered}");

    digest.handle_key(key(KeyCode::Right));
    digest.handle_key(key(KeyCode::Enter));
    let rendered = render_text(&digest, 120, 34);

    assert!(rendered.contains("digest:digest-edge"), "{rendered}");
    assert!(!rendered.contains("Diff preview"), "{rendered}");
}

#[test]
fn permission_modal_precedes_error_palette_slash_and_escape_keeps_request_pending() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| intents.lock().expect("lock intents").push(intent))
    };
    let mut app = AppState::new_live(None, false, Some(sink));
    for event in failed_turn_events() {
        app.ingest_event(event);
    }
    open_palette_command(&mut app, "error");
    app.overlay_state.slash_visible = true;
    app.handle_key(ctrl('p'));
    app.ingest_event(permission_event(
        10,
        "read",
        &json!({"path": "src/lib.rs"}).to_string(),
    ));

    let rendered = render_text(&app, 120, 34);
    assert!(rendered.contains("Permission required"), "{rendered}");
    assert!(rendered.contains("Read src/lib.rs"), "{rendered}");
    assert!(!rendered.contains("Error details"), "{rendered}");

    app.handle_key(key(KeyCode::Esc));

    let intents = intents.lock().expect("lock intents");
    assert!(matches!(
        intents.as_slice(),
        [UiIntent::ResolvePermission {
            decision: PermissionDecision::Deny,
            grant_scope: None,
            ..
        }]
    ));
    assert!(app.active_permission().is_some());
    assert!(render_text(&app, 120, 34).contains("Permission required"));
}

#[test]
fn empty_error_child_and_timeline_surfaces_render_safe_states() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/g012_empty_edges")), false, None);

    open_palette_command(&mut app, "error");
    let no_error = render_text(&app, 120, 34);
    assert!(no_error.contains("no failed turn to inspect"), "{no_error}");

    open_palette_command(&mut app, "child");
    let no_children = render_text(&app, 120, 34);
    assert!(no_children.contains("Child sessions"), "{no_children}");
    assert!(no_children.contains("No child sessions"), "{no_children}");

    app.handle_key(key(KeyCode::Esc));
    app.handle_key(ctrl('x'));
    app.handle_key(key(KeyCode::Char('g')));
    let timeline = render_text(&app, 120, 34);
    assert!(timeline.contains("Message timeline"), "{timeline}");
    assert!(timeline.contains("No saved sessions"), "{timeline}");
}

fn open_palette_command(app: &mut AppState, filter: &str) {
    app.handle_key(ctrl('p'));
    for ch in filter.chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
}

fn render_text(app: &AppState, width: u16, height: u16) -> String {
    render_to_string(app, Rect::new(0, 0, width, height), |app, frame, _area| {
        let _plan = FrameLayoutPlan::for_app(app, frame.area());
        ui::render_app(frame, app)
    })
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(ch: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL)
}

fn permission_event(seq: u64, kind: &str, summary: &str) -> EventEnvelopeV1 {
    event(
        seq,
        Some("tc_g012_edge_permission"),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: format!("perm_g012_edge_{seq}"),
            kind: kind.to_string(),
            tool_call_id: Some("tc_g012_edge_permission".to_string()),
            summary: summary.to_string(),
            request_digest: format!("digest-edge-{seq:04}-permission"),
            timeout_ms: 30_000,
            default_decision: EventPermissionDecision::Deny,
        }),
    )
}

fn failed_turn_events() -> Vec<EventEnvelopeV1> {
    vec![
        event(
            1,
            Some("req_g012_edge_failed"),
            EventV1::RunStarted(RunStartedEvent {
                run_name: "g012-edge-error".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        event(
            2,
            Some("req_g012_edge_failed"),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_g012_edge_failed".to_string(),
                text: "Retry edge".to_string(),
            }),
        ),
        event(
            3,
            Some("req_g012_edge_failed"),
            EventV1::RunFailed(RunFailedEvent {
                error: "transport_failure: network unreachable".to_string(),
            }),
        ),
    ]
}

fn event(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt_g012_edge_{seq:04}"),
        seq,
        run_id: "run_g012_edge".to_string(),
        mono_ms: seq,
        ts: Some(format!("2026-06-14T12:{:02}:00Z", seq.min(59))),
        actor: EventActor::new(ActorKind::System, Some("g012-edge-test".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_g012_edge".to_string()),
        payload,
    }
}
