//! Structural owners for SHELL-IDLE / TX-USER / TX-ASSISTANT / TX-TOOL / TX-DIFF.
//!
//! Contract: `docs/grok-build-tui-implementation-prompt.md` +
//! `crates/harness-tui/DESIGN.md` §8 / §14.
//!
//! Live transcript freezes are partial (`run1-stream-probe`). These tests lock
//! DESIGN-observable density: full-width body, bordered composer, no legacy
//! left rail / message cards, user `❯` chrome, rail-free assistant body/footer,
//! structured tool rows, and rail-free inline diffs.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration parity tests use fail-fast asserts"
)]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestFinishedEvent,
    ProviderRequestStartedEvent, ProviderStreamDeltaEvent, RunFailedEvent, TaskCancelledEvent,
    TaskScheduleState, TaskScheduledEvent, TaskTerminalScope, ToolCallFinishedEvent,
    ToolCallRequestedEvent, ToolCallStartedEvent, ToolCallStatus, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use harness_providers::CompletionUsage;
use harness_tui::app::{AppState, Focus, LaunchMetadata, RuntimeStateKind};
use harness_tui::layout::FrameLayoutPlan;
use harness_tui::render_test::render_to_string;
use harness_tui::ui;
use ratatui::layout::Rect;

const W: u16 = 120;
const H: u16 = 40;
const SCROLL_FREEZE_H: u16 = 32;

fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-tx-shell-{seq:04}"),
        seq,
        run_id: "run_tx_shell_parity".into(),
        mono_ms: seq,
        ts: Some("2026-03-19T05:54:00Z".to_string()),
        actor: EventActor::new(ActorKind::System, Some("tx-shell-parity".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_tx_shell_parity".to_string()),
        payload,
    }
}

fn live_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-tx").with_mode_label("Demo"),
    );
    app
}

fn render(app: &AppState) -> String {
    render_to_string(app, Rect::new(0, 0, W, H), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

fn render_at(app: &AppState, width: u16, height: u16) -> String {
    render_to_string(app, Rect::new(0, 0, width, height), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

fn first_inventory_line(screen: &str) -> Option<u32> {
    for line in screen.lines() {
        let trimmed = line.trim();
        let Some((num, rest)) = trimmed.split_once(". f") else {
            continue;
        };
        if !rest.ends_with(".txt") {
            continue;
        }
        if let Ok(n) = num.parse::<u32>() {
            return Some(n);
        }
    }
    None
}

fn first_non_whitespace_column(line: &str) -> usize {
    line.chars()
        .position(|ch| !ch.is_whitespace())
        .unwrap_or(line.chars().count())
}

fn first_alphanumeric_column(line: &str) -> usize {
    line.chars()
        .position(char::is_alphanumeric)
        .unwrap_or(line.chars().count())
}

fn ingest_completed_turn(app: &mut AppState, request_id: &str, user: &str, assistant: &str) {
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: user.to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "model-tx".to_string(),
            prompt_summary: user.to_string(),
            request_digest: "digest-tx".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: request_id.into(),
            delta: assistant.to_string(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        Some(request_id),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: request_id.into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-out".to_string()),
            usage: None,
            metadata: None,
        }),
    ));
}

fn completed_turn_events(
    first_seq: u64,
    request_id: &str,
    user: &str,
    assistant: &str,
) -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            first_seq,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: user.to_string(),
            }),
        ),
        envelope(
            first_seq + 1,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "model-tx".to_string(),
                prompt_summary: user.to_string(),
                request_digest: format!("digest-{request_id}"),
                metadata: None,
            }),
        ),
        envelope(
            first_seq + 2,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: assistant.to_string(),
            }),
        ),
        envelope(
            first_seq + 3,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.into(),
                finish_reason: "stop".to_string(),
                output_digest: Some(format!("digest-out-{request_id}")),
                usage: None,
                metadata: None,
            }),
        ),
    ]
}

fn app_with_deep_history() -> AppState {
    let mut app = live_app();
    let events = (0..6_u64)
        .flat_map(|turn| {
            let request_id = format!("req_history_{turn}");
            let user = format!("history prompt {turn}");
            let assistant =
                format!("history reply {turn}\nrow a\nrow b\nrow c\nrow d\nturn-{turn}-tail");
            completed_turn_events(turn * 4 + 1, &request_id, &user, &assistant)
        })
        .collect();
    app.replace_events(events);
    let _ = render(&app);
    app
}

fn submit_text(app: &mut AppState, text: &str) {
    for character in text.chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
}

/// SHELL-IDLE: completed-turn live shell keeps full-width transcript + bordered composer.
/// Freeze capture: TBD (no `run*-idle` freeze yet).
#[test]
fn shell_idle_keeps_full_width_transcript_and_bordered_composer() {
    // arrange
    let mut app = live_app();
    ingest_completed_turn(
        &mut app,
        "req_idle",
        "idle user prompt",
        "idle assistant reply",
    );

    // act
    let rendered = render(&app);

    // assert — transcript body visible
    assert!(
        rendered.contains("idle user prompt") && rendered.contains("idle assistant reply"),
        "SHELL-IDLE: completed turn must remain visible in body\n{rendered}"
    );
    assert!(
        rendered.contains('❯'),
        "SHELL-IDLE: composer glyph required\n{rendered}"
    );

    // assert — bordered composer (rounded strip)
    let lines: Vec<&str> = rendered.lines().collect();
    let glyph_idx = lines
        .iter()
        .rposition(|line| line.contains('│') && line.contains('❯'))
        .or_else(|| lines.iter().rposition(|line| line.contains('❯')))
        .expect("composer glyph");
    let above = glyph_idx.checked_sub(1).map(|i| lines[i]).unwrap_or("");
    let below = lines.get(glyph_idx + 1).copied().unwrap_or("");
    assert!(
        above.contains('╭') || above.contains('─'),
        "SHELL-IDLE: composer top border required\nabove={above:?}\n{rendered}"
    );
    assert!(
        below.contains('╰') || below.contains('─'),
        "SHELL-IDLE: composer bottom border required\nbelow={below:?}\n{rendered}"
    );

    // assert — DESIGN §8: no legacy left rail as primary chrome
    assert!(
        !rendered.contains('┃'),
        "SHELL-IDLE: must not paint legacy left rail ┃\n{rendered}"
    );
}

/// SHELL-IDLE (empty live body): after startup dismiss without activities, body stays
/// card-free and composer remains bordered. Freeze: TBD.
#[test]
fn shell_idle_empty_body_is_card_free_with_bordered_composer() {
    // arrange — leave startup by typing then clearing (draft dismisses welcome)
    let mut app = live_app();
    app.composer.prompt_buffer = "x".to_string();
    app.composer.prompt_cursor = 1;
    // Clear draft but keep non-startup live shell (activities empty)
    app.composer.prompt_buffer.clear();
    app.composer.prompt_cursor = 0;

    // act
    let rendered = render(&app);

    // assert — bordered composer retained
    assert!(
        rendered.contains('❯'),
        "SHELL-IDLE empty: composer glyph required\n{rendered}"
    );
    let lines: Vec<&str> = rendered.lines().collect();
    let glyph_idx = lines
        .iter()
        .rposition(|line| line.contains('│') && line.contains('❯'))
        .or_else(|| lines.iter().rposition(|line| line.contains('❯')))
        .expect("composer glyph");
    let above = glyph_idx.checked_sub(1).map(|i| lines[i]).unwrap_or("");
    let below = lines.get(glyph_idx + 1).copied().unwrap_or("");
    assert!(
        above.contains('╭') || above.contains('─'),
        "SHELL-IDLE empty: composer top border required\nabove={above:?}\n{rendered}"
    );
    assert!(
        below.contains('╰') || below.contains('─'),
        "SHELL-IDLE empty: composer bottom border required\nbelow={below:?}\n{rendered}"
    );

    // assert — no legacy Session card / left rail as primary empty-body chrome
    assert!(
        !rendered.contains('┃'),
        "SHELL-IDLE empty: must not paint legacy left rail ┃\n{rendered}"
    );
    // Elevated "Session" card title is old Harness empty-state chrome (reskin leftover)
    assert!(
        !rendered.contains("Session"),
        "SHELL-IDLE empty: must not paint elevated Session card chrome\n{rendered}"
    );
}

/// SHELL-IDLE state machine: the focus owner stays on the composer and the
/// overlay z-stack is empty (manifest: `expected_focus_owner: composer`,
/// `expected_overlays_and_z_order: []`).
#[test]
fn shell_idle_focus_owner_is_composer_and_overlay_stack_is_empty() {
    // arrange
    let mut app = live_app();
    ingest_completed_turn(
        &mut app,
        "req_idle_state",
        "idle state prompt",
        "idle state reply",
    );

    // act
    let top_overlay = app.overlay_stack().top();

    // assert — focus belongs to the composer; no overlays are open
    assert_eq!(
        app.focus,
        Focus::Prompt,
        "SHELL-IDLE: focus owner must be the composer"
    );
    assert!(
        top_overlay.is_none(),
        "SHELL-IDLE: overlay z-stack must be empty, got {top_overlay:?}"
    );
    assert!(!app.palette_visible);
    assert!(!app.session_history_visible);
}

/// SHELL-IDLE error recovery: a failed tool call mid-run leaves the shell
/// idle-ready — failure visible in the body, bordered composer retained,
/// focus still on the composer, no overlays opened.
#[test]
fn shell_idle_stays_composer_ready_after_failed_tool_call() {
    // arrange — completed turn, then a tool call that fails
    let mut app = live_app();
    ingest_completed_turn(
        &mut app,
        "req_idle_recover",
        "recover prompt",
        "recover reply",
    );
    app.ingest_event(envelope(
        5,
        Some("req_idle_recover"),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_idle_fail".into(),
            tool_id: "fs.read".to_string(),
            args_summary: r#"{"path":"missing.txt"}"#.to_string(),
            args_digest: "digest-args-fail".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        6,
        Some("req_idle_recover"),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tc_idle_fail".into(),
        }),
    ));
    app.ingest_event(envelope(
        7,
        Some("req_idle_recover"),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_idle_fail".into(),
            status: ToolCallStatus::Failed,
            output_summary: Some("tool exploded mid-run".to_string()),
            output_digest: Some("digest-tool-fail".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));
    // act
    let rendered = render(&app);

    // assert — failure surfaced, composer bordered and focused, no overlays
    assert!(
        rendered.contains("tool exploded mid-run"),
        "SHELL-IDLE recovery: tool failure must stay visible in the body\n{rendered}"
    );
    assert!(
        rendered.contains('❯'),
        "SHELL-IDLE recovery: composer must stay ready\n{rendered}"
    );
    let lines: Vec<&str> = rendered.lines().collect();
    let glyph_idx = lines
        .iter()
        .rposition(|line| line.contains('│') && line.contains('❯'))
        .or_else(|| lines.iter().rposition(|line| line.contains('❯')))
        .expect("composer glyph");
    let above = glyph_idx.checked_sub(1).map(|i| lines[i]).unwrap_or("");
    let below = lines.get(glyph_idx + 1).copied().unwrap_or("");
    assert!(
        (above.contains('╭') || above.contains('─'))
            && (below.contains('╰') || below.contains('─')),
        "SHELL-IDLE recovery: bordered composer required\nabove={above:?} below={below:?}\n{rendered}"
    );
    assert_eq!(app.focus, Focus::Prompt);
    assert!(app.overlay_stack().top().is_none());
}

/// TX-USER: user messages use the elevated Grok band with a stable `❯` marker.
#[test]
fn tx_user_message_chrome_is_elevated_band_without_legacy_rail() {
    // arrange
    let mut app = live_app();
    ingest_completed_turn(
        &mut app,
        "req_user",
        "Explain shell parity",
        "Assistant acknowledges.",
    );

    // act
    let rendered = render(&app);
    let lines: Vec<&str> = rendered.lines().collect();
    let user_idx = lines
        .iter()
        .position(|line| line.contains("Explain shell parity"))
        .expect("user body");

    // assert
    assert!(
        lines[user_idx]
            .trim_start()
            .starts_with("❯ Explain shell parity"),
        "TX-USER: user body must use the stable compact marker\n{rendered}"
    );
    assert!(
        !lines[user_idx].contains('┃'),
        "TX-USER: no legacy outer rail on user body\n{rendered}"
    );
    assert!(
        !rendered.contains("You  "),
        "TX-USER: the rejected width-dependent label must not return\n{rendered}"
    );
    if user_idx > 0 {
        let prev = lines[user_idx - 1];
        assert!(
            !prev.contains('┃') && prev.trim().is_empty(),
            "TX-USER: the band must retain a blank top-padding row\nprev={prev:?}\n{rendered}"
        );
    }
    assert!(
        !lines[user_idx].contains('╭') && !lines[user_idx].contains('╮'),
        "TX-USER: elevated band must remain borderless\n{rendered}"
    );
}

#[test]
fn tx_user_long_message_collapses_after_three_visual_rows() {
    // arrange
    // Given: a submitted prompt that wraps beyond the reference three-row limit.
    let mut app = live_app();
    let long_prompt = format!(
        "{} TAIL_MUST_BE_COLLAPSED",
        "long user message segment ".repeat(24)
    );
    ingest_completed_turn(
        &mut app,
        "req_user_long",
        &long_prompt,
        "Assistant acknowledges.",
    );

    // When: the transcript is rendered at the primary reference width.
    let rendered = render(&app);

    // act
    // Then: disclosure is explicit and content beyond row three stays folded.
    // assert
    assert!(
        rendered.contains('…'),
        "TX-USER: folded prompt needs an ellipsis\n{rendered}"
    );
    assert!(
        !rendered.contains("TAIL_MUST_BE_COLLAPSED"),
        "TX-USER: content beyond the third visual row must stay folded\n{rendered}"
    );
}

#[test]
fn tx_user_submit_page_flips_new_prompt_to_viewport_top() {
    // arrange
    // Given: enough completed history to fill more than one transcript viewport.
    let mut app = app_with_deep_history();

    // When: the user submits a new prompt.
    submit_text(&mut app, "new turn page flip");
    let rendered = render(&app);
    let new_prompt_row = rendered
        .lines()
        .position(|line| line.contains("new turn page flip"))
        .expect("submitted prompt row");

    // act
    // Then: the band starts at the first transcript row and its content follows the top pad.
    // assert
    assert_eq!(
        new_prompt_row, 4,
        "TX-USER: submitted prompt content must follow the pinned band top, row={new_prompt_row}\n{rendered}"
    );
    assert!(
        !rendered.contains("turn-5-tail"),
        "TX-USER: the immediately preceding turn tail must move above the viewport\n{rendered}"
    );
}

#[test]
fn tx_user_page_flip_keeps_prior_turn_reachable_by_scrolling_up() {
    // arrange
    // Given: a submitted prompt preserved at the top of a deep transcript.
    let mut app = app_with_deep_history();
    submit_text(&mut app, "reachability prompt");
    let preserved = render(&app);
    assert!(!preserved.contains("turn-5-tail"), "{preserved}");

    // When: the user scrolls far enough toward older transcript content.
    app.scroll_page_up(24);
    let scrolled = render(&app);

    // act
    // Then: the prior turn remains in scrollback instead of being removed by page flip.
    // assert
    assert!(
        scrolled.contains("turn-5-tail"),
        "TX-USER: prior turn must remain reachable above the page-flipped prompt\n{scrolled}"
    );
}

#[test]
fn tx_user_page_flip_targets_activated_prompt_before_later_queued_prompt() {
    // arrange
    // Given: deep history, one active turn, and two queued follow-up prompts.
    let mut app = app_with_deep_history();
    app.ingest_event(envelope(
        25,
        Some("req_active"),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_active".into(),
            text: "active prompt".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        26,
        Some("req_active"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_active".into(),
            provider_id: "mock".to_string(),
            model_id: "model-tx".to_string(),
            prompt_summary: "active prompt".to_string(),
            request_digest: "digest-active".to_string(),
            metadata: None,
        }),
    ));
    submit_text(&mut app, "queued prompt B");
    submit_text(&mut app, "queued prompt C");
    app.ingest_event(envelope(
        27,
        Some("req_b"),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_b".into(),
            text: "queued prompt B".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        28,
        Some("req_c"),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_c".into(),
            text: "queued prompt C".to_string(),
        }),
    ));
    for offset in 0_u64..10 {
        let request_id = format!("req_later_{offset}");
        let prompt = format!("later queued prompt {offset}");
        submit_text(&mut app, &prompt);
        app.ingest_event(envelope(
            29 + offset,
            Some(&request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.clone().into(),
                text: prompt,
            }),
        ));
    }
    app.ingest_event(envelope(
        39,
        Some("req_active"),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: "req_active".into(),
            finish_reason: "stop".to_string(),
            output_digest: None,
            usage: None,
            metadata: None,
        }),
    ));

    // When: B activates while C remains queued behind it.
    app.ingest_event(envelope(
        40,
        Some("req_b"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_b".into(),
            provider_id: "mock".to_string(),
            model_id: "model-tx".to_string(),
            prompt_summary: "queued prompt B".to_string(),
            request_digest: "digest-b".to_string(),
            metadata: None,
        }),
    ));
    let rendered = render(&app);
    let prompt_b_row = rendered
        .lines()
        .position(|line| line.contains("queued prompt B"))
        .unwrap_or_else(|| panic!("activated B must remain visible above queued C\n{rendered}"));
    let prompt_c_row = rendered
        .lines()
        .position(|line| line.contains("queued prompt C"))
        .unwrap_or_else(|| panic!("queued C should remain visible below activated B\n{rendered}"));

    // act
    // Then: page flip starts at B, not the latest queued row C.
    // assert
    assert!(
        prompt_b_row < prompt_c_row,
        "activated B must precede queued C, B={prompt_b_row}, C={prompt_c_row}\n{rendered}"
    );
    assert_eq!(
        prompt_b_row, 4,
        "later queued rows must not consume B's page flip before B itself overflows\n{rendered}"
    );
}

include!("support/reference_parity_tx_shell_test_part2_test.rs");
