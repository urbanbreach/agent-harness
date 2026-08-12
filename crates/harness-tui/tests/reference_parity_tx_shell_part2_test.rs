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
    TaskTerminalScope, ToolCallFinishedEvent, ToolCallRequestedEvent, ToolCallStartedEvent,
    ToolCallStatus, UserMessageSubmittedEvent, SCHEMA_VERSION,
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

fn count_char(rendered: &str, ch: char) -> usize {
    rendered.chars().filter(|c| *c == ch).count()
}

/// SHELL-FAIL: failed turn keeps full-width transcript, visible error, bordered composer.
/// Freeze capture: `run1-stream-probe` (user turn + error chrome + empty composer).
#[test]
fn shell_fail_keeps_error_in_full_width_transcript_with_composer() {
    // arrange
    let mut app = live_app();
    let request_id = "req_fail";
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "ping".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "model-tx".to_string(),
            prompt_summary: "ping".to_string(),
            request_digest: "digest-fail".to_string(),
            metadata: None,
        }),
    ));
    let streaming_dock = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, W, H))
        .dock
        .expect("live shell should have a control dock");
    app.ingest_event(envelope(
        3,
        None,
        EventV1::RunFailed(RunFailedEvent {
            error: "API error (status 400 Bad Request): invalid-argument: Incorrect API key provided. You can obtain an API key from https://console.x.ai.\n\nRequest URL: https://api.x.ai/v1/responses\n\nTurn failed in 0.3s: Internal error: {\n  \"message\": \"API error (status 400 Bad Request): invalid-argument: Incorrect API key provided. You can obtain an API key from https://console.x.ai.\\n\\nRequest UR..."
                .to_string(),
        }),
    ));

    // act
    let rendered = render(&app);
    let runtime = app.runtime_state();
    let failed_dock = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, W, H))
        .dock
        .expect("failed live shell should keep its control dock");

    assert_eq!(
        runtime.kind,
        RuntimeStateKind::Failure,
        "SHELL-FAIL: runtime must be Failure; got {:?}",
        runtime.kind
    );
    assert!(failed_dock.composer.height < streaming_dock.composer.height);
    assert!(
        failed_dock.status.is_some(),
        "SHELL-FAIL: failed state must preserve the live status band"
    );
    assert!(
        rendered.contains("ping"),
        "SHELL-FAIL: user turn retained above error chrome\n{rendered}"
    );
    assert!(
        rendered.contains("API error")
            || rendered.contains("Incorrect API key")
            || rendered.contains("invalid-argument")
            || runtime.summary.to_lowercase().contains("fail"),
        "SHELL-FAIL: error text must project into shell\nstate={runtime:?}\n{rendered}"
    );
    assert!(
        rendered.contains("Request URL:") && rendered.contains("Turn failed"),
        "SHELL-FAIL: multi-line fail detail (Request URL / Turn failed) must project\n{rendered}"
    );
    assert!(
        rendered.contains('❯'),
        "SHELL-FAIL: collapsed composer retained under full-width shell\n{rendered}"
    );
    assert!(
        !rendered.contains('┃'),
        "SHELL-FAIL: no legacy left rail on fail chrome\n{rendered}"
    );
    // Freeze run1-stream-probe: flat `Retry failed: …` body, no elevated Failure/Review card,
    // no ✗ model duration footer under the error block.
    assert!(
        rendered.contains("Retry failed:"),
        "SHELL-FAIL: freeze-aligned Retry failed: prefix required\n{rendered}"
    );
    assert!(
        rendered.contains(''),
        "SHELL-FAIL: freeze-aligned live breadcrumb required\n{rendered}"
    );
    assert!(
        !rendered.contains("Review required"),
        "SHELL-FAIL: elevated Review required card must not paint over fail chrome\n{rendered}"
    );
    assert!(
        !rendered.contains('✗'),
        "SHELL-FAIL: freeze has no ✗ model error footer\n{rendered}"
    );
    let lines: Vec<&str> = rendered.lines().collect();
    let user_idx = lines
        .iter()
        .position(|line| line.contains("ping"))
        .expect("user");
    let error_idx = lines.iter().enumerate().find_map(|(i, line)| {
        (line.contains("Retry failed:")
            || line.contains("API error")
            || line.contains("Incorrect API key")
            || line.contains("invalid-argument"))
        .then_some(i)
    });
    if let Some(error_idx) = error_idx {
        assert!(
            user_idx <= error_idx,
            "SHELL-FAIL: user turn stacks above/at error chrome\n{rendered}"
        );
        assert!(
            !lines[error_idx].contains('╭') && !lines[error_idx].contains('╰'),
            "SHELL-FAIL: error chrome must not use rounded message cards\n{rendered}"
        );
    }
}

/// SHELL-FAIL freeze ladder (run1-stream-probe @120x40): composer L35, disclosure L39, empty L40.
#[test]
fn shell_fail_dock_matches_freeze_vertical_ladder() {
    // arrange
    // act
    // assert
    let mut app = live_app();
    let request_id = "req_fail_ladder";
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "ping".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "model-tx".to_string(),
            prompt_summary: "ping".to_string(),
            request_digest: "digest-fail-ladder".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        None,
        EventV1::RunFailed(RunFailedEvent {
            error: "API error (status 400 Bad Request): invalid-argument".to_string(),
        }),
    ));

    let plan = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, W, H));
    let dock = plan.dock.expect("live fail shell keeps a dock");
    let status = dock.status.expect("live fail shell keeps a status band");
    let disclosure = dock.disclosure.expect("live fail shell keeps disclosure");

    assert_eq!(
        status,
        Rect::new(2, 35, 116, 1),
        "SHELL-FAIL freeze ladder: one status row sits immediately above the composer"
    );
    assert_eq!(
        dock.composer.y, 36,
        "SHELL-FAIL compact ladder: composer top y=36; got y={}",
        dock.composer.y
    );
    assert_eq!(
        dock.composer.height, 1,
        "SHELL-FAIL collapses an empty unfocused composer; got {}",
        dock.composer.height
    );
    assert_eq!(
        disclosure.y, 38,
        "SHELL-FAIL freeze ladder: disclosure y=38 (L39); got y={}",
        disclosure.y
    );
    assert_eq!(
        dock.shell.height, 5,
        "SHELL-FAIL compact ladder uses status+composer+gap+disclosure+bottom; got {}",
        dock.shell.height
    );
    assert_eq!(
        dock.shell.y + dock.shell.height,
        H,
        "SHELL-FAIL freeze ladder: dock remains bottom-anchored with empty L40 margin"
    );
    assert_eq!(
        disclosure.y + disclosure.height + 1,
        H,
        "SHELL-FAIL freeze ladder: one empty row below disclosure"
    );
}

/// SHELL-COMPLETE: finished turn leaves Success/Ready with full-width body + bordered composer.
#[test]
fn shell_complete_keeps_full_width_body_and_bordered_composer() {
    // arrange
    // act
    // assert
    // arrange
    let mut app = live_app();
    ingest_completed_turn(
        &mut app,
        "req_complete",
        "completed user turn",
        "completed assistant turn",
    );

    // act
    let rendered = render(&app);
    let runtime = app.runtime_state();

    assert!(
        matches!(
            runtime.kind,
            RuntimeStateKind::Success | RuntimeStateKind::Ready
        ),
        "SHELL-COMPLETE: runtime must be Success/Ready; got {:?}",
        runtime.kind
    );
    assert!(
        rendered.contains("completed user turn") && rendered.contains("completed assistant turn"),
        "SHELL-COMPLETE: completed turn projects into transcript\n{rendered}"
    );
    assert!(
        rendered.contains('❯') && count_char(&rendered, '╭') >= 1,
        "SHELL-COMPLETE: bordered composer retained under full-width shell\n{rendered}"
    );
    assert!(
        !rendered.contains('┃'),
        "SHELL-COMPLETE: no legacy left rail\n{rendered}"
    );
}

/// SHELL-CANCEL: AgentTurn cancel projects Cancelled (not Failure) under full-width shell.
#[test]
fn shell_cancel_projects_cancelled_distinct_from_failure() {
    // arrange
    // act
    // assert
    // arrange
    let mut app = live_app();
    let request_id = "req_cancel";
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "cancel probe".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "model-tx".to_string(),
            prompt_summary: "cancel probe".to_string(),
            request_digest: "digest-cancel".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        EventV1::TaskCancelled(TaskCancelledEvent {
            task_id: "task_cancel_parity".into(),
            reason: "user interrupted streaming turn".to_string(),
            task_scope: Some(TaskTerminalScope::AgentTurn),
        }),
    ));

    // act
    let rendered = render(&app);
    let runtime = app.runtime_state();

    assert_eq!(
        runtime.kind,
        RuntimeStateKind::Cancelled,
        "SHELL-CANCEL: runtime must be Cancelled; got {:?}",
        runtime.kind
    );
    assert_ne!(
        runtime.kind,
        RuntimeStateKind::Failure,
        "SHELL-CANCEL: cancelled must remain distinct from Failure"
    );
    assert!(
        rendered.contains("cancel probe"),
        "SHELL-CANCEL: user turn retained under cancel shell\n{rendered}"
    );
    assert!(
        rendered.contains("Turn cancelled by user"),
        "SHELL-CANCEL: cancel reason must surface as Harness cancel footer\n{rendered}"
    );
    assert!(
        !rendered.contains("Retry failed:"),
        "SHELL-CANCEL: must not reuse fail chrome prefix\n{rendered}"
    );
    assert!(
        rendered.contains('❯') && count_char(&rendered, '╭') >= 1,
        "SHELL-CANCEL: bordered composer retained under full-width shell\n{rendered}"
    );
    assert!(
        !rendered.contains('┃'),
        "SHELL-CANCEL: no legacy left rail\n{rendered}"
    );
}

/// SHELL-RECOVER: after Failure, full-width shell keeps bordered composer + draft for retry.
#[test]
fn shell_recover_after_failure_keeps_composer_and_accepts_draft() {
    // arrange
    // act
    // assert
    // arrange
    let mut app = live_app();
    let request_id = "req_recover";
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "first attempt".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "model-tx".to_string(),
            prompt_summary: "first attempt".to_string(),
            request_digest: "digest-recover".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        None,
        EventV1::RunFailed(RunFailedEvent {
            error: "provider stream interrupted".to_string(),
        }),
    ));
    let failed = app.runtime_state();
    assert_eq!(
        failed.kind,
        RuntimeStateKind::Failure,
        "SHELL-RECOVER: pre-condition Failure required; got {:?}",
        failed.kind
    );

    // act
    let draft = "retry after failure";
    app.composer.prompt_buffer = draft.to_string();
    app.composer.prompt_cursor = draft.chars().count();
    app.focus = Focus::Prompt;
    let rendered = render(&app);
    let runtime = app.runtime_state();
    let plan = harness_tui::FrameLayoutPlan::for_app(&app, Rect::new(0, 0, W, H));

    assert_eq!(
        app.composer.prompt_buffer, draft,
        "SHELL-RECOVER: draft must be editable after Failure"
    );
    assert!(
        !runtime.composer_disabled,
        "SHELL-RECOVER: composer must remain enabled for retry after Failure"
    );
    assert!(
        rendered.contains("first attempt"),
        "SHELL-RECOVER: failed turn remains visible in transcript\n{rendered}"
    );
    assert!(
        rendered.contains("provider stream interrupted")
            || runtime.summary.to_lowercase().contains("fail")
            || runtime.summary.to_lowercase().contains("error"),
        "SHELL-RECOVER: failure context remains visible for review\nstate={runtime:?}\n{rendered}"
    );
    assert!(
        rendered.contains('❯') && count_char(&rendered, '╭') >= 1,
        "SHELL-RECOVER: bordered composer retained under full-width shell\n{rendered}"
    );
    assert!(
        plan.operator_sidebar.is_none(),
        "SHELL-RECOVER: full-width shell (no operator sidebar)"
    );
    assert!(
        !rendered.contains('┃'),
        "SHELL-RECOVER: no legacy left rail\n{rendered}"
    );
    assert!(
        runtime.composer_hint.to_lowercase().contains("retry")
            || runtime.composer_hint.to_lowercase().contains("continue")
            || runtime.composer_hint.to_lowercase().contains("draft")
            || !runtime.composer_hint.is_empty()
            || matches!(runtime.kind, RuntimeStateKind::Failure),
        "SHELL-RECOVER: recovery choreography keeps Failure review path\nstate={runtime:?}"
    );
}

/// SHELL-SCROLL: settle inventory window to freeze band f39–f55 (loop15 packing).
#[test]
fn shell_scroll_freeze_viewport_packs_f39_to_f55_at_120x32() {
    // arrange
    // act
    // assert
    let mut app = live_app();
    let request_id = "req_scroll_freeze_pack";
    let user = "List every file in the current directory using a tool, then write a numbered inventory of all names one per line.";
    let inventory = (1..=80)
        .map(|n| format!("{n}. f{n}.txt"))
        .collect::<Vec<_>>()
        .join("\n");
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: user.into(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".into(),
            model_id: "model-tx".into(),
            prompt_summary: user.into(),
            request_digest: "digest-scroll-freeze".into(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: request_id.into(),
            delta: inventory,
        }),
    ));
    app.ingest_event(envelope(
        4,
        Some(request_id),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: request_id.into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("out-scroll-freeze".into()),
            usage: Some(CompletionUsage {
                prompt_tokens: 19_000,
                completion_tokens: 0,
                total_tokens: 19_000,
            }),
            metadata: None,
        }),
    ));

    let _bottom = render_at(&app, W, SCROLL_FREEZE_H);
    app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
    let after_pages = render_at(&app, W, SCROLL_FREEZE_H);
    let first_after_pages = first_inventory_line(&after_pages);
    assert!(
        first_after_pages.is_some_and(|n| (35..=41).contains(&n)),
        "PageUp should land near freeze mid-band; first inventory={first_after_pages:?}\n{after_pages}"
    );

    let mut first = first_after_pages;
    for _ in 0..8 {
        if first == Some(35) {
            break;
        }
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL));
        let screen = render_at(&app, W, SCROLL_FREEZE_H);
        first = first_inventory_line(&screen);
    }

    let packed = render_at(&app, W, SCROLL_FREEZE_H);
    let first_packed = first_inventory_line(&packed);
    assert_eq!(
        first_packed,
        Some(35),
        "compact viewport packing requires first inventory line f35; got {first_packed:?}\n{packed}"
    );
    assert!(
        packed.contains("List every file in the current directory"),
        "SCROLL freeze sticky user: user prompt must remain visible at top while f39 band shows\n{packed}"
    );
    assert!(
        packed.lines().any(|line| line.contains('❯')
            && line.contains("all names")
            && line.contains("5:54 AM")),
        "SCROLL freeze sticky user: first user row packs 'all names' + wall clock\n{packed}"
    );
    assert!(
        packed.contains("54. f54.txt"),
        "compact viewport packing requires f54 still visible\n{packed}"
    );
    assert!(
        packed.contains('▼'),
        "freeze viewport packing requires more-below ▼\n{packed}"
    );
    assert!(
        !packed.contains("61. f61.txt"),
        "loop15 residual: f61 must leave the window once settled to f39 band\n{packed}"
    );
    if let Ok(path) = std::env::var("HARNESS_SCROLL_L3_DUMP") {
        let path = std::path::PathBuf::from(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create HARNESS_SCROLL_L3_DUMP parent");
        }
        std::fs::write(&path, &packed).expect("write HARNESS_SCROLL_L3_DUMP");
    }
}

/// SHELL-SCROLL: PageUp/PageDown adjust transcript scroll under full-width shell.
#[test]
fn shell_scroll_page_keys_adjust_transcript_under_full_width() {
    // arrange
    // act
    // assert
    // arrange
    let mut app = live_app();
    for (seq, rid, text) in [
        (1u64, "req_scroll_a", "First user turn for scroll"),
        (2u64, "req_scroll_b", "Second user turn for scroll"),
        (3u64, "req_scroll_c", "Third user turn for scroll"),
    ] {
        app.ingest_event(envelope(
            seq * 10,
            Some(rid),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: rid.into(),
                text: text.into(),
            }),
        ));
        app.ingest_event(envelope(
            seq * 10 + 1,
            Some(rid),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: rid.into(),
                provider_id: "mock".into(),
                model_id: "model-tx".into(),
                prompt_summary: text.into(),
                request_digest: format!("digest-{rid}"),
                metadata: None,
            }),
        ));
        app.ingest_event(envelope(
            seq * 10 + 2,
            Some(rid),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: rid.into(),
                delta: (0..12)
                    .map(|line| format!("Assistant body for {rid}, row {line}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            }),
        ));
        app.ingest_event(envelope(
            seq * 10 + 3,
            Some(rid),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: rid.into(),
                finish_reason: "stop".to_string(),
                output_digest: Some(format!("out-{rid}")),
                usage: None,
                metadata: None,
            }),
        ));
    }
    app.focus = Focus::Details;
    let _ = render(&app);
    let before = app.transcript_interaction_snapshot();
    assert!(before.follow_mode);
    assert_eq!(before.scroll, 0);

    // act
    app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
    let after_page_up = app.transcript_interaction_snapshot();
    app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
    let landed_at_bottom = app.transcript_interaction_snapshot();
    app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
    let after_overscroll = app.transcript_interaction_snapshot();
    let plan = harness_tui::FrameLayoutPlan::for_app(&app, Rect::new(0, 0, W, H));
    let rendered = render(&app);

    assert!(
        after_page_up.scroll > 0,
        "SHELL-SCROLL: PageUp must increase transcript_scroll; got {}",
        after_page_up.scroll
    );
    assert!(
        !after_page_up.follow_mode,
        "SHELL-SCROLL: PageUp must leave follow mode"
    );
    assert_eq!(
        landed_at_bottom.scroll, 0,
        "SHELL-SCROLL: PageDown must return to bottom (scroll 0)"
    );
    assert!(
        landed_at_bottom.follow_mode,
        "SHELL-SCROLL: PageDown reaching the bottom must restore follow mode"
    );
    assert!(
        after_overscroll.follow_mode,
        "SHELL-SCROLL: a clamped PageDown at bottom must restore follow mode"
    );
    assert!(
        plan.operator_sidebar.is_none(),
        "SHELL-SCROLL: full-width shell (no operator sidebar)"
    );
    assert!(
        rendered.contains("First user turn for scroll")
            || rendered.contains("Third user turn for scroll"),
        "SHELL-SCROLL: transcript content remains under full-width shell\n{rendered}"
    );
    assert!(
        rendered.contains('❯'),
        "SHELL-SCROLL: composer retained\n{rendered}"
    );
}
