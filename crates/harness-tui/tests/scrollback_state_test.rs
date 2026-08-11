//! Scrollback state parity contract tests (Todo 12 — clean-room parity program).
//!
//! Proof dimensions exercised:
//! - P1 (contract): scroll position invariants, follow mode lifecycle
//! - P2 (owner): folding expand/collapse via tool output state
//! - P5 (motion): content-arrival follow behavior
//! - P7 (lifecycle): selection snapshot round-trip through interaction state

use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestFinishedEvent,
    ProviderRequestStartedEvent, ToolCallFinishedEvent, ToolCallRequestedEvent,
    ToolCallStartedEvent, ToolCallStatus, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_tui::app::{AppState, LaunchMetadata};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn startup_app() -> AppState {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.set_launch_metadata(LaunchMetadata::from_model_ref("worker", "mock:model-1"));
    app
}

fn live_app_with_tool_call() -> AppState {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/scrollback_test")), false, None);
    for event in tool_call_events() {
        app.ingest_event(event);
    }
    app
}

fn tool_call_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_scrollback";
    vec![
        envelope(
            1,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: "Check scrollback".to_string(),
            }),
        ),
        envelope(
            2,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "Check scrollback".to_string(),
                request_digest: "digest-scrollback-req".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            3,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_bash_1".into(),
                tool_id: "bash".to_string(),
                args_summary: r#"{"command":"ls -la"}"#.to_string(),
                args_digest: "digest-tc_bash_1-args".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            4,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_bash_1".into(),
            }),
        ),
        envelope(
            5,
            Some(request_id),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_bash_1".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("total 42\ndrwxr-xr-x  5 user user 4096 .".to_string()),
                output_digest: Some("digest-tc_bash_1-output".to_string()),
                output_json: Some(serde_json::json!({
                    "exit_code": 0,
                    "stdout": "total 42\ndrwxr-xr-x  5 user user 4096 ."
                })),
                metadata: None,
            }),
        ),
        envelope(
            6,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_read_1".into(),
                tool_id: "read".to_string(),
                args_summary: r#"{"path":"README.md"}"#.to_string(),
                args_digest: "digest-tc_read_1-args".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            7,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_read_1".into(),
            }),
        ),
        envelope(
            8,
            Some(request_id),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_read_1".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("# agent-harness\nRust workspace".to_string()),
                output_digest: Some("digest-tc_read_1-output".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
        envelope(
            9,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-scrollback-resp".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
    ]
}

fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-scrollback-{seq:04}"),
        seq,
        run_id: "run_scrollback".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("scrollback-test".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_scrollback".to_string()),
        payload,
    }
}

// ===========================================================================
// P1: Scroll position contract
// ===========================================================================

#[test]
fn scroll_up_increases_offset_and_breaks_follow() {
    let mut app = startup_app();
    app.record_transcript_max_scroll(200);

    assert!(app.follow_mode_active(), "app starts in follow mode");
    assert_eq!(app.transcript_scroll_offset(), 0);

    app.scroll_page_up(20);

    assert!(
        !app.follow_mode_active(),
        "scroll up must break follow mode"
    );
    assert_eq!(
        app.transcript_scroll_offset(),
        20,
        "page up by viewport=20 moves offset by 20"
    );
}

#[test]
fn scroll_down_decreases_offset_and_reengages_at_bottom() {
    let mut app = startup_app();
    app.record_transcript_max_scroll(200);
    app.set_transcript_scroll_for_test(0);
    app.scroll_page_up(50);
    assert_eq!(app.transcript_scroll_offset(), 50);

    app.scroll_page_down(30);
    assert_eq!(
        app.transcript_scroll_offset(),
        20,
        "page down subtracts viewport from offset"
    );
    assert!(!app.follow_mode_active());

    app.scroll_page_down(30);
    assert_eq!(
        app.transcript_scroll_offset(),
        0,
        "scroll clamps to zero at bottom"
    );
    assert!(
        app.follow_mode_active(),
        "page down reaching scroll=0 re-engages follow mode"
    );

    app.scroll_page_down(30);
    assert!(
        app.follow_mode_active(),
        "a clamped downward gesture at scroll=0 re-engages follow mode"
    );
}

#[test]
fn scroll_half_page_uses_half_viewport() {
    let mut app = startup_app();
    app.record_transcript_max_scroll(500);

    app.scroll_half_page_up(40);
    assert_eq!(
        app.transcript_scroll_offset(),
        20,
        "half page up with viewport=40 moves 20"
    );

    app.scroll_half_page_down(40);
    assert_eq!(
        app.transcript_scroll_offset(),
        0,
        "half page down returns toward bottom"
    );
}

#[test]
fn goto_top_sets_offset_to_max_scroll() {
    let mut app = startup_app();
    app.record_transcript_max_scroll(150);

    app.scroll_goto_top();

    assert_eq!(
        app.transcript_scroll_offset(),
        150,
        "goto top moves offset to max scroll"
    );
    assert!(!app.follow_mode_active(), "goto top breaks follow mode");
}

#[test]
fn goto_bottom_resets_offset_and_engages_follow() {
    let mut app = startup_app();
    app.record_transcript_max_scroll(150);
    app.scroll_goto_top();
    assert_eq!(app.transcript_scroll_offset(), 150);

    app.scroll_goto_bottom();

    assert_eq!(app.transcript_scroll_offset(), 0);
    assert!(app.follow_mode_active(), "goto bottom re-engages follow");
}

// ===========================================================================
// P5: Follow mode motion contract
// ===========================================================================

#[test]
fn follow_mode_pins_scroll_on_content_arrival() {
    let mut app = startup_app();
    app.record_transcript_max_scroll(100);
    assert!(app.follow_mode_active());

    app.follow_mode_content_arrived();
    assert_eq!(
        app.transcript_scroll_offset(),
        0,
        "follow mode keeps scroll at bottom (offset 0)"
    );

    app.record_transcript_max_scroll(200);
    app.follow_mode_content_arrived();
    assert_eq!(
        app.transcript_scroll_offset(),
        0,
        "repeated content arrival stays pinned"
    );
}

#[test]
fn follow_mode_does_not_move_scroll_when_disengaged() {
    let mut app = startup_app();
    app.record_transcript_max_scroll(100);
    app.scroll_page_up(10);
    assert!(!app.follow_mode_active());
    assert_eq!(app.transcript_scroll_offset(), 10);

    app.record_transcript_max_scroll(200);
    app.follow_mode_content_arrived();
    assert_eq!(
        app.transcript_scroll_offset(),
        110,
        "detached content anchor stays fixed while new rows grow below it"
    );
}

#[test]
fn follow_mode_round_trip_up_and_back_down() {
    let mut app = startup_app();
    app.record_transcript_max_scroll(300);

    app.scroll_page_up(30);
    assert!(!app.follow_mode_active());

    app.scroll_page_down(30);
    assert!(app.follow_mode_active());

    app.scroll_page_down(30);
    assert!(app.follow_mode_active());

    app.follow_mode_content_arrived();
    assert_eq!(app.transcript_scroll_offset(), 0);
}

// ===========================================================================
// P2: Folding (expand/collapse) contract
// ===========================================================================

#[test]
fn toggle_tool_output_expands_and_collapses() {
    let mut app = live_app_with_tool_call();

    assert!(
        !app.is_tool_output_expanded_for_test("tc_bash_1"),
        "tool starts collapsed"
    );

    app.toggle_tool_output_for_test("tc_bash_1");
    assert!(
        app.is_tool_output_expanded_for_test("tc_bash_1"),
        "toggle expands"
    );

    app.toggle_tool_output_for_test("tc_bash_1");
    assert!(
        !app.is_tool_output_expanded_for_test("tc_bash_1"),
        "second toggle collapses"
    );
}

#[test]
fn expand_all_and_collapse_all_tool_outputs() {
    let mut app = live_app_with_tool_call();

    app.expand_all_tool_outputs_for_test();
    let expanded = app.expanded_tool_output_ids_for_test();
    assert!(
        expanded.contains(&"tc_bash_1".to_string()),
        "expand_all includes bash tool"
    );
    assert!(
        expanded.contains(&"tc_read_1".to_string()),
        "expand_all includes read tool"
    );

    app.collapse_all_tool_outputs_for_test();
    let collapsed = app.expanded_tool_output_ids_for_test();
    assert!(
        !collapsed.contains(&"tc_bash_1".to_string()),
        "collapse_all removes bash tool"
    );
    assert!(
        !collapsed.contains(&"tc_read_1".to_string()),
        "collapse_all removes read tool"
    );
}

#[test]
fn fold_state_appears_in_interaction_snapshot() {
    let mut app = live_app_with_tool_call();
    app.toggle_tool_output_for_test("tc_bash_1");

    let snapshot = app.transcript_interaction_snapshot();
    assert!(
        snapshot
            .expanded_tool_call_ids
            .contains(&"tc_bash_1".to_string()),
        "snapshot captures expanded tool ids"
    );
    assert!(
        !snapshot
            .expanded_tool_call_ids
            .contains(&"tc_read_1".to_string()),
        "snapshot only includes actually-expanded ids"
    );
}

// ===========================================================================
// P7: Selection and interaction snapshot lifecycle
// ===========================================================================

#[test]
fn interaction_snapshot_captures_scroll_and_follow_state() {
    let mut app = startup_app();
    app.record_transcript_max_scroll(400);
    app.scroll_page_up(25);

    let snapshot = app.transcript_interaction_snapshot();
    assert_eq!(snapshot.scroll, 25);
    assert!(!snapshot.follow_mode);
    assert_eq!(snapshot.selected_activity_index, 0);
    assert!(snapshot.show_tool_details);
}

#[test]
fn interaction_snapshot_at_follow_bottom() {
    let app = startup_app();

    let snapshot = app.transcript_interaction_snapshot();
    assert_eq!(snapshot.scroll, 0);
    assert!(snapshot.follow_mode, "default state is follow");
}

#[test]
fn selected_activity_index_tracks_in_snapshot() {
    let mut app = live_app_with_tool_call();
    app.set_selected_activity_index_for_test(0);

    let snapshot = app.transcript_interaction_snapshot();
    assert_eq!(snapshot.selected_activity_index, 0);
    assert!(
        !snapshot.follow_mode,
        "set_selected_activity_index disables follow"
    );
}

// ===========================================================================
// Layout measurement contracts (P1 surface positioning / line wrapping)
// ===========================================================================

#[test]
fn viewport_scroll_bounds_never_exceed_max_scroll() {
    let mut app = startup_app();
    app.record_transcript_max_scroll(50);

    app.scroll_page_up(100);
    assert_eq!(
        app.transcript_scroll_offset(),
        50,
        "measured scroll state clamps to the rendered-row maximum"
    );

    app.scroll_goto_top();
    assert_eq!(
        app.transcript_scroll_offset(),
        50,
        "goto_top snaps to max_scroll exactly"
    );
}

#[test]
fn scroll_arithmetic_is_saturating() {
    let mut app = startup_app();
    app.record_transcript_max_scroll(10);

    app.scroll_page_down(100);
    assert_eq!(
        app.transcript_scroll_offset(),
        0,
        "scroll down saturates at zero (no underflow)"
    );
    assert!(app.follow_mode_active());
}

#[test]
fn page_scroll_with_minimum_viewport() {
    let mut app = startup_app();
    app.record_transcript_max_scroll(100);

    app.scroll_page_up(0);
    assert_eq!(
        app.transcript_scroll_offset(),
        1,
        "viewport=0 is treated as 1 (minimum)"
    );
}

#[test]
fn half_page_odd_viewport_rounds_up() {
    let mut app = startup_app();
    app.record_transcript_max_scroll(100);

    app.scroll_half_page_up(7);
    assert_eq!(
        app.transcript_scroll_offset(),
        4,
        "half of 7 rounds up to 4"
    );
}
