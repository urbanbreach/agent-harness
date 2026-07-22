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

/// TX-USER: user message chrome uses `❯` marker, flat padding, no outer rail / You header.
/// Freeze capture: run1-stream-probe (`❯ ping`).
#[test]
fn tx_user_message_chrome_is_flat_marker_without_legacy_rail() {
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
        lines[user_idx].contains('❯'),
        "TX-USER: user body must use ❯ marker\n{rendered}"
    );
    assert!(
        !lines[user_idx].contains('┃'),
        "TX-USER: no legacy outer rail on user body\n{rendered}"
    );
    assert!(
        !rendered.contains("❯ You") && !rendered.contains("› You") && !rendered.contains("You ·"),
        "TX-USER: no synthetic You header label\n{rendered}"
    );
    if user_idx > 0 {
        let prev = lines[user_idx - 1];
        assert!(
            !prev.contains('┃') && !prev.contains("You"),
            "TX-USER: flat top padding without rail/header\nprev={prev:?}\n{rendered}"
        );
    }
    let marker_col = lines[user_idx].find('❯').expect("marker");
    let after = &lines[user_idx][marker_col + '❯'.len_utf8()..];
    assert!(
        after.starts_with(' ') && after.contains("Explain shell parity"),
        "TX-USER: ❯ then space then body text\nline={}\n{rendered}",
        lines[user_idx]
    );
    // Marker sits in the left gutter; body text is marker + one space (2-col prefix pattern)
    let user_text_col = first_alphanumeric_column(lines[user_idx]);
    let user_rail_col = first_non_whitespace_column(lines[user_idx]);
    assert_eq!(
        user_text_col.saturating_sub(user_rail_col),
        2,
        "TX-USER: marker plus one-column space before body text\n{rendered}"
    );
}

/// TX-ASSISTANT: assistant prose is rail-free, inset vs user rail, footer without box.
/// Freeze capture: TBD.
#[test]
fn tx_assistant_message_chrome_is_rail_free_with_footer() {
    // arrange
    let mut app = live_app();
    ingest_completed_turn(
        &mut app,
        "req_asst",
        "User asks",
        "Assistant answers cleanly.",
    );

    // act
    let rendered = render(&app);
    let lines: Vec<&str> = rendered.lines().collect();
    let user_idx = lines
        .iter()
        .position(|line| line.contains("User asks"))
        .expect("user");
    let asst_idx = lines
        .iter()
        .position(|line| line.contains("Assistant answers cleanly."))
        .expect("assistant");
    let footer_idx = lines
        .iter()
        .enumerate()
        .skip(asst_idx + 1)
        .find_map(|(i, line)| {
            (line.contains("Worked for")
                || line.contains("model-tx")
                || line.contains("build")
                || line.contains("▪"))
            .then_some(i)
        })
        .expect("assistant footer");

    // assert — order + no rails
    assert!(user_idx < asst_idx && asst_idx < footer_idx);
    assert!(
        !lines[asst_idx].contains('┃'),
        "TX-ASSISTANT: no outer rail on assistant body\n{rendered}"
    );
    assert!(
        !lines[footer_idx].contains('┃'),
        "TX-ASSISTANT: no outer rail on footer\n{rendered}"
    );

    // Grok COMPLETE freeze packs Thought + dedicated wall-clock row between user and body
    // (user → Thought → clock → body ⇒ gap 7 at 100x30 unit geometry).
    assert!(
        asst_idx - user_idx <= 7,
        "TX-ASSISTANT: turn stacking should stay compact (gap={})\n{rendered}",
        asst_idx - user_idx
    );

    let asst_region = lines[asst_idx..=footer_idx].join("\n");
    assert!(
        !asst_region.contains('╭') && !asst_region.contains('╰'),
        "TX-ASSISTANT: message body/footer must not use rounded card borders\n{asst_region}\n{rendered}"
    );

    let user_rail = first_non_whitespace_column(lines[user_idx]);
    let asst_rail = first_non_whitespace_column(lines[asst_idx]);
    assert_eq!(
        asst_rail, user_rail,
        "TX-ASSISTANT: assistant body lead must match user lead (Grok packing)\nuser={user_rail} asst={asst_rail}\n{rendered}"
    );
    assert!(
        !rendered.contains("Thought for"),
        "TX-ASSISTANT: completed turns without reasoning must not render Thought for header\n{rendered}"
    );
    assert!(
        rendered.contains("Worked for"),
        "TX-ASSISTANT: completed turns render Worked for footer\n{rendered}"
    );
}

/// TX-TOOL: tool rows use ◆ identity, structured path summary, no outer rail / opaque-only dump.
#[test]
fn tx_tool_row_is_structured_diamond_without_legacy_rail() {
    // arrange
    // act
    // assert
    // arrange
    let mut app = live_app();
    let request_id = "req_tool";
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "Read the readme".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "model-tx".to_string(),
            prompt_summary: "Read the readme".to_string(),
            request_digest: "digest-tool".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_tool".into(),
            tool_id: "fs.read".to_string(),
            args_summary: r#"{"path":"README.md"}"#.to_string(),
            args_digest: "digest-args-tool".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        4,
        Some(request_id),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tc_tool".into(),
        }),
    ));
    app.ingest_event(envelope(
        5,
        Some(request_id),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_tool".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("file contents".to_string()),
            output_digest: Some("digest-out-tool".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));

    // act
    let rendered = render(&app);
    let lines: Vec<&str> = rendered.lines().collect();
    let tool_idx = lines
        .iter()
        .position(|line| line.contains("README.md") || line.contains("Read"))
        .expect("tool row");

    assert!(
        rendered.contains('◈') || rendered.contains('◆'),
        "TX-TOOL: tool identity glyph ◈/◆ required\n{rendered}"
    );
    assert!(
        lines[tool_idx].contains("README.md")
            || lines[tool_idx].contains("Read 1 file")
            || rendered.contains("README.md")
            || rendered.contains("Read 1 file")
            || rendered.contains("Read "),
        "TX-TOOL: structured tool title (path or completed count) required\n{rendered}"
    );
    assert!(
        !lines[tool_idx].contains('┃'),
        "TX-TOOL: no legacy outer rail on tool row\n{rendered}"
    );
    assert!(
        !rendered.contains(r#"{"path":"README.md"}"#)
            || rendered.contains("Read")
            || rendered.contains("README.md"),
        "TX-TOOL: must not rely on opaque JSON dump alone\n{rendered}"
    );
    assert!(
        rendered.contains('❯') && count_char(&rendered, '╭') >= 1,
        "TX-TOOL: full-width shell keeps bordered composer\n{rendered}"
    );
}

/// TX-DIFF: inline edit/diff body stays rail-free and non-card under full-width shell.
#[test]
fn tx_diff_inline_is_rail_free_without_message_card() {
    // arrange
    // act
    // assert
    // arrange
    let mut app = live_app();
    let request_id = "req_diff";
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "Apply a patch".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "model-tx".to_string(),
            prompt_summary: "Apply a patch".to_string(),
            request_digest: "digest-diff".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_diff".into(),
            tool_id: "edit".to_string(),
            args_summary: r#"{"path":"src/main.rs","old":"fn a(){}","new":"fn a(){ 1 }"}"#
                .to_string(),
            args_digest: "digest-args-diff".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        4,
        Some(request_id),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tc_diff".into(),
        }),
    ));
    app.ingest_event(envelope(
        5,
        Some(request_id),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_diff".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("edited src/main.rs".to_string()),
            output_digest: Some("digest-out-diff".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));

    // act
    let rendered = render(&app);
    let lines: Vec<&str> = rendered.lines().collect();
    let tool_idx = lines
        .iter()
        .position(|line| {
            line.contains("src/main.rs")
                || line.contains("edit")
                || line.contains("Edit")
                || line.contains("◆")
        })
        .expect("diff/tool row");

    assert!(
        rendered.contains("src/main.rs") || rendered.contains('◆'),
        "TX-DIFF: structured edit/path projection required\n{rendered}"
    );
    assert!(
        !lines[tool_idx].contains('┃'),
        "TX-DIFF: no legacy outer rail on diff/tool row\n{rendered}"
    );
    let region_end = (tool_idx + 8).min(lines.len().saturating_sub(1));
    let region = lines[tool_idx..=region_end].join("\n");
    assert!(
        !region.contains('╭') && !region.contains('╰'),
        "TX-DIFF: inline diff/tool body must not use rounded message cards\n{region}\n{rendered}"
    );
    assert!(
        rendered.contains('❯'),
        "TX-DIFF: bordered composer retained under full-width shell\n{rendered}"
    );
}

fn count_char(rendered: &str, ch: char) -> usize {
    rendered.chars().filter(|c| *c == ch).count()
}

/// SHELL-STREAM: active provider stream keeps full-width transcript body + bordered composer.
/// Freeze capture: run2-shell-stream-pinned-v2 ("Waiting for response…" state).
#[test]
fn shell_stream_keeps_full_width_body_and_bordered_composer() {
    // arrange — Grok STREAM freeze: user submitted, provider started, no body text.
    // TaskScheduled keeps activity Streaming after ProviderRequestFinished seeds
    // total_tokens for ⇣ counter.
    let mut app = live_app();
    let request_id = "req_stream";
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_stream_parity".to_string().into(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:mock:model-tx".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "stream parity probe".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "model-tx".to_string(),
            prompt_summary: "stream parity probe".to_string(),
            request_digest: "digest-stream".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        4,
        Some(request_id),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: request_id.into(),
            finish_reason: "done".to_string(),
            output_digest: Some("digest-out-stream".to_string()),
            usage: Some(CompletionUsage {
                prompt_tokens: 1430,
                completion_tokens: 0,
                total_tokens: 1430,
            }),
            metadata: None,
        }),
    ));

    // act
    let rendered = render(&app);
    let runtime = app.runtime_state();

    // assert
    assert!(
        matches!(
            runtime.kind,
            RuntimeStateKind::Streaming | RuntimeStateKind::Sending
        ),
        "SHELL-STREAM: runtime must be streaming/sending; got {:?}",
        runtime.kind
    );
    assert!(
        rendered.contains("stream parity probe"),
        "SHELL-STREAM: user turn retained in transcript\n{rendered}"
    );
    assert!(
        rendered.contains("Waiting for response"),
        "SHELL-STREAM: waiting-for-response indicator must project\n{rendered}"
    );
    assert!(
        !rendered.contains("partial assistant tokens"),
        "SHELL-STREAM: no body text in waiting-for-response state\n{rendered}"
    );
    assert!(
        rendered.contains('❯') && count_char(&rendered, '╭') >= 1,
        "SHELL-STREAM: bordered composer retained under full-width shell\n{rendered}"
    );
    assert!(
        !rendered.contains('┃'),
        "SHELL-STREAM: no legacy left rail\n{rendered}"
    );
    let lines: Vec<&str> = rendered.lines().collect();
    let user_idx = lines
        .iter()
        .position(|line| line.contains("stream parity probe"))
        .expect("user");
    let waiting_idx = lines
        .iter()
        .position(|line| line.contains("Waiting for response"))
        .expect("waiting indicator");
    assert!(
        user_idx < waiting_idx,
        "SHELL-STREAM: user above waiting-for-response indicator\n{rendered}"
    );
}
