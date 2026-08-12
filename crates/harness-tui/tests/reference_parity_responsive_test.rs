//! Compact viewport parity (RESP-60x20 / RESP-79x24 / RESP-80x24 / RESP-100x30 /
//! RESP-120x40 / RESP-120x50 / RESP-WIDE).
//!
//! Reference freeze (run1-resp-*-pinned-v1) shows real HOME idle shell at each
//! viewport: breadcrumb + empty transcript body + bordered composer (empty
//! prompt) + idle footer (Shift+Tab:mode | Ctrl+x:shortcuts). No welcome panel.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration parity tests use fail-fast asserts"
)]

use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionDecision, PermissionRequestedEvent,
    ProviderRequestStartedEvent, ProviderStreamDeltaEvent, ToolCallRequestedEvent,
    UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_tui::app::{AppState, Focus, LaunchMetadata};
use harness_tui::render_test::render_to_string;
use harness_tui::ui;
use ratatui::layout::Rect;

fn idle_shell_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );
    app.focus = Focus::Prompt;
    app
}

fn streaming_shell_app() -> AppState {
    let mut app = idle_shell_app();
    for (seq, payload) in [
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_responsive_stream".into(),
            text: "Explain responsive hierarchy".to_string(),
        }),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_responsive_stream".into(),
            provider_id: "mock".to_string(),
            model_id: "model-1".to_string(),
            prompt_summary: "Explain responsive hierarchy".to_string(),
            request_digest: "digest-responsive-stream".to_string(),
            metadata: None,
        }),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "req_responsive_stream".into(),
            delta: "Streaming remains recognizable at every viewport.".to_string(),
        }),
    ]
    .into_iter()
    .enumerate()
    {
        app.ingest_event(EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt-responsive-stream-{seq:04}"),
            seq: u64::try_from(seq + 1).expect("responsive event sequence fits u64"),
            run_id: "run_responsive_stream".into(),
            mono_ms: u64::try_from(seq + 1).expect("responsive monotonic time fits u64"),
            ts: Some("2026-08-09T12:00:00Z".to_string()),
            actor: EventActor::new(ActorKind::System, Some("responsive-test".to_string())),
            correlation_id: Some("req_responsive_stream".to_string()),
            causation_id: None,
            stream_key: Some("run:run_responsive_stream".to_string()),
            payload,
        });
    }
    app
}

fn permission_shell_app() -> AppState {
    let mut app = streaming_shell_app();
    for (seq, payload) in [
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "call_responsive_edit".into(),
            tool_id: "edit".to_string(),
            args_summary: r#"{"path":"src/responsive.rs"}"#.to_string(),
            args_digest: "digest-responsive-edit".to_string(),
            metadata: None,
        }),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_responsive_edit".to_string(),
            kind: "edit".to_string(),
            tool_call_id: Some("call_responsive_edit".into()),
            summary: "Allow editing src/responsive.rs?".to_string(),
            request_digest: "digest-responsive-permission".to_string(),
            timeout_ms: 30_000,
            default_decision: PermissionDecision::Deny,
        }),
    ]
    .into_iter()
    .enumerate()
    {
        let seq = seq + 4;
        app.ingest_event(EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt-responsive-permission-{seq:04}"),
            seq: u64::try_from(seq).expect("responsive event sequence fits u64"),
            run_id: "run_responsive_stream".into(),
            mono_ms: u64::try_from(seq).expect("responsive monotonic time fits u64"),
            ts: Some("2026-08-09T12:00:00Z".to_string()),
            actor: EventActor::new(ActorKind::System, Some("responsive-test".to_string())),
            correlation_id: Some("req_responsive_stream".to_string()),
            causation_id: None,
            stream_key: Some("run:run_responsive_stream".to_string()),
            payload,
        });
    }
    app
}

fn render_at(app: &AppState, width: u16, height: u16) -> String {
    render_to_string(app, Rect::new(0, 0, width, height), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

fn count_char(rendered: &str, ch: char) -> usize {
    rendered.chars().filter(|c| *c == ch).count()
}

#[test]
fn streaming_state_remains_recognizable_at_all_reference_viewports() {
    let app = streaming_shell_app();

    for (width, height) in [
        (120_u16, 50_u16),
        (120, 40),
        (100, 30),
        (80, 24),
        (79, 24),
        (60, 20),
        (140, 40),
    ] {
        let rendered = render_at(&app, width, height);

        assert!(
            rendered.contains("Streaming remains recognizable")
                || rendered.contains("Responding")
                || rendered.contains("Waiting for response"),
            "{width}x{height}: streaming state must remain visible\n{rendered}"
        );
        assert!(
            rendered.contains('❯'),
            "{width}x{height}: composer must remain usable while streaming\n{rendered}"
        );
        assert!(
            rendered
                .lines()
                .all(|line| unicode_width::UnicodeWidthStr::width(line) <= usize::from(width)),
            "{width}x{height}: no rendered row may overflow the viewport\n{rendered}"
        );
    }
}

#[test]
fn permission_controls_remain_accessible_at_all_reference_viewports() {
    let app = permission_shell_app();

    for (width, height) in [
        (120_u16, 50_u16),
        (120, 40),
        (100, 30),
        (80, 24),
        (79, 24),
        (60, 20),
        (140, 40),
    ] {
        let rendered = render_at(&app, width, height);

        assert!(
            rendered.contains("Yes") && rendered.contains("No, reject"),
            "{width}x{height}: primary permission controls must remain accessible\n{rendered}"
        );
        assert!(
            rendered.contains('●') && rendered.contains('○'),
            "{width}x{height}: permission focus grammar must remain recognizable\n{rendered}"
        );
        assert!(
            rendered.contains('❯'),
            "{width}x{height}: permission surface must preserve the composer\n{rendered}"
        );
        assert!(
            rendered
                .lines()
                .all(|line| unicode_width::UnicodeWidthStr::width(line) <= usize::from(width)),
            "{width}x{height}: no permission row may overflow the viewport\n{rendered}"
        );
    }
}

/// RESP-80x24 idle shell: breadcrumb + empty body + bordered composer + idle footer.
#[test]
fn resp_80x24_idle_shell_keeps_bordered_composer() {
    // arrange
    let app = idle_shell_app();

    // act
    let rendered = render_at(&app, 80, 24);

    // assert
    assert!(
        rendered.contains('❯'),
        "RESP-80x24: composer glyph required\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '╭'),
        1,
        "RESP-80x24: exactly one bordered box (composer)\n{rendered}"
    );
    assert!(
        !rendered.contains("Shift+Tab:mode") && !rendered.contains("Ctrl+x:shortcuts"),
        "RESP-80x24: idle footer must collapse\n{rendered}"
    );
    assert!(
        !rendered.contains("New worktree") && !rendered.contains("New session"),
        "RESP-80x24: welcome actions must not appear in idle shell\n{rendered}"
    );
    assert!(
        !rendered.contains("Enter:send"),
        "RESP-80x24: idle shell must not show draft footer\n{rendered}"
    );
}

/// RESP-79x24 idle shell: same anatomy as 80x24 at narrow boundary width.
#[test]
fn resp_79x24_idle_shell_keeps_bordered_composer() {
    // arrange
    let app = idle_shell_app();

    // act
    let rendered = render_at(&app, 79, 24);

    // assert
    assert!(
        rendered.contains('❯'),
        "RESP-79x24: composer glyph required\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '╭'),
        1,
        "RESP-79x24: exactly one bordered box (composer)\n{rendered}"
    );
    assert!(
        !rendered.contains("Shift+Tab:mode") && !rendered.contains("Ctrl+x:shortcuts"),
        "RESP-79x24: idle footer must collapse\n{rendered}"
    );
    assert!(
        !rendered.contains("Enter:send"),
        "RESP-79x24: idle shell must not show draft footer\n{rendered}"
    );
}

/// RESP-100x30 idle shell: breadcrumb + empty body + bordered composer + idle footer.
#[test]
fn resp_100x30_idle_shell_keeps_bordered_composer() {
    // arrange
    let app = idle_shell_app();

    // act
    let rendered = render_at(&app, 100, 30);

    // assert
    assert!(
        rendered.contains('❯'),
        "RESP-100x30: composer glyph required\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '╭'),
        1,
        "RESP-100x30: exactly one bordered box (composer)\n{rendered}"
    );
    assert!(
        !rendered.contains("Shift+Tab:mode") && !rendered.contains("Ctrl+x:shortcuts"),
        "RESP-100x30: idle footer must collapse\n{rendered}"
    );
    assert!(
        !rendered.contains("Enter:send"),
        "RESP-100x30: idle shell must not show draft footer\n{rendered}"
    );
}

/// RESP-120x40 idle shell: same anatomy with extra vertical gap.
#[test]
fn resp_120x40_idle_shell_keeps_bordered_composer() {
    // arrange
    let app = idle_shell_app();

    // act
    let rendered = render_at(&app, 120, 40);

    // assert
    assert!(
        rendered.contains('❯'),
        "RESP-120x40: composer glyph required\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '╭'),
        1,
        "RESP-120x40: exactly one bordered box (composer)\n{rendered}"
    );
    assert!(
        !rendered.contains("Shift+Tab:mode") && !rendered.contains("Ctrl+x:shortcuts"),
        "RESP-120x40: idle footer must collapse\n{rendered}"
    );
    assert!(
        !rendered.contains("Enter:send"),
        "RESP-120x40: idle shell must not show draft footer\n{rendered}"
    );
}

/// RESP-60x20 idle shell (freeze run1-resp-60x20-pinned-v1): compact idle shell.
#[test]
fn resp_60x20_idle_shell_keeps_bordered_composer() {
    // arrange
    let app = idle_shell_app();

    // act
    let rendered = render_at(&app, 60, 20);

    // assert
    assert!(
        rendered.contains('❯'),
        "RESP-60x20: composer glyph required\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '╭'),
        1,
        "RESP-60x20: exactly one bordered box (composer)\n{rendered}"
    );
    assert!(
        !rendered.contains("Shift+Tab:mode") && !rendered.contains("Ctrl+x:shortcuts"),
        "RESP-60x20: idle footer must collapse\n{rendered}"
    );
    assert!(
        !rendered.contains("Enter:send"),
        "RESP-60x20: idle shell must not show draft footer\n{rendered}"
    );
}

/// RESP-120x50 idle shell (freeze run1-resp-120x50-pinned-v1): tall idle shell.
#[test]
fn resp_120x50_idle_shell_keeps_bordered_composer() {
    // arrange
    let app = idle_shell_app();

    // act
    let rendered = render_at(&app, 120, 50);

    // assert
    assert!(
        rendered.contains('❯'),
        "RESP-120x50: composer glyph required\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '╭'),
        1,
        "RESP-120x50: exactly one bordered box (composer)\n{rendered}"
    );
    assert!(
        !rendered.contains("Shift+Tab:mode") && !rendered.contains("Ctrl+x:shortcuts"),
        "RESP-120x50: idle footer must collapse\n{rendered}"
    );
    assert!(
        !rendered.contains("Enter:send"),
        "RESP-120x50: idle shell must not show draft footer\n{rendered}"
    );
}

/// RESP-WIDE idle shell (freeze run1-resp-140x40-pinned-v1): wide idle shell.
#[test]
fn resp_wide_140x40_idle_shell_keeps_bordered_composer() {
    // arrange
    let app = idle_shell_app();

    // act
    let rendered = render_at(&app, 140, 40);

    // assert
    assert!(
        rendered.contains('❯'),
        "RESP-WIDE: composer glyph required\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '╭'),
        1,
        "RESP-WIDE: exactly one bordered box (composer)\n{rendered}"
    );
    assert!(
        !rendered.contains("Shift+Tab:mode") && !rendered.contains("Ctrl+x:shortcuts"),
        "RESP-WIDE: idle footer must collapse\n{rendered}"
    );
    assert!(
        !rendered.contains("Enter:send"),
        "RESP-WIDE: idle shell must not show draft footer\n{rendered}"
    );
}

// ---------------------------------------------------------------------------
// Boundary column tests: 59/60/61, 79/80/81, 99/100/101, 119/120/121
// ---------------------------------------------------------------------------

/// Boundary 59/60/61 columns: composer border and footer survive the dense cutoff.
#[test]
fn boundary_59_60_61_columns_keep_bordered_composer_and_footer() {
    for width in [59u16, 60, 61] {
        let app = idle_shell_app();
        let rendered = render_at(&app, width, 20);

        assert!(
            rendered.contains('❯'),
            "boundary {width}x20: composer glyph required\n{rendered}"
        );
        assert_eq!(
            count_char(&rendered, '╭'),
            1,
            "boundary {width}x20: exactly one bordered box\n{rendered}"
        );
        assert!(
            !rendered.contains("Shift+Tab:mode") && !rendered.contains("Ctrl+x:shortcuts"),
            "boundary {width}x20: idle footer must collapse\n{rendered}"
        );
    }
}

/// Boundary 79/80/81 columns: composer border and footer survive the minimum breakpoint.
#[test]
fn boundary_79_80_81_columns_keep_bordered_composer_and_footer() {
    for width in [79u16, 80, 81] {
        let app = idle_shell_app();
        let rendered = render_at(&app, width, 24);

        assert!(
            rendered.contains('❯'),
            "boundary {width}x24: composer glyph required\n{rendered}"
        );
        assert_eq!(
            count_char(&rendered, '╭'),
            1,
            "boundary {width}x24: exactly one bordered box\n{rendered}"
        );
        assert!(
            !rendered.contains("Shift+Tab:mode") && !rendered.contains("Ctrl+x:shortcuts"),
            "boundary {width}x24: idle footer must collapse\n{rendered}"
        );
    }
}

/// Boundary 99/100/101 columns: composer border and footer survive the primary breakpoint.
#[test]
fn boundary_99_100_101_columns_keep_bordered_composer_and_footer() {
    for width in [99u16, 100, 101] {
        let app = idle_shell_app();
        let rendered = render_at(&app, width, 30);

        assert!(
            rendered.contains('❯'),
            "boundary {width}x30: composer glyph required\n{rendered}"
        );
        assert_eq!(
            count_char(&rendered, '╭'),
            1,
            "boundary {width}x30: exactly one bordered box\n{rendered}"
        );
        assert!(
            !rendered.contains("Shift+Tab:mode") && !rendered.contains("Ctrl+x:shortcuts"),
            "boundary {width}x30: idle footer must collapse\n{rendered}"
        );
    }
}

/// Boundary 119/120/121 columns: composer border and footer survive the wide breakpoint.
#[test]
fn boundary_119_120_121_columns_keep_bordered_composer_and_footer() {
    for width in [119u16, 120, 121] {
        let app = idle_shell_app();
        let rendered = render_at(&app, width, 32);

        assert!(
            rendered.contains('❯'),
            "boundary {width}x32: composer glyph required\n{rendered}"
        );
        assert_eq!(
            count_char(&rendered, '╭'),
            1,
            "boundary {width}x32: exactly one bordered box\n{rendered}"
        );
        assert!(
            !rendered.contains("Shift+Tab:mode") && !rendered.contains("Ctrl+x:shortcuts"),
            "boundary {width}x32: idle footer must collapse\n{rendered}"
        );
    }
}

/// Boundary 120×40 and 120×50: tall viewports keep anatomy.
#[test]
fn boundary_120_col_tall_heights_keep_bordered_composer_and_footer() {
    for height in [40u16, 50] {
        let app = idle_shell_app();
        let rendered = render_at(&app, 120, height);

        assert_eq!(
            count_char(&rendered, '╭'),
            1,
            "120x{height}: exactly one bordered box\n{rendered}"
        );
        assert!(
            !rendered.contains("Shift+Tab:mode") && !rendered.contains("Ctrl+x:shortcuts"),
            "120x{height}: idle footer must collapse\n{rendered}"
        );
    }
}

/// Layout plan boundary: composer and disclosure rects are valid at all boundaries.
#[test]
fn layout_plan_boundary_viewports_have_valid_composer_and_disclosure() {
    use harness_tui::FrameLayoutPlan;
    use ratatui::layout::Rect;

    let app = idle_shell_app();
    let boundaries: &[(u16, u16)] = &[
        (59, 20),
        (60, 20),
        (61, 20),
        (79, 24),
        (80, 24),
        (81, 24),
        (99, 30),
        (100, 30),
        (101, 30),
        (119, 32),
        (120, 32),
        (121, 32),
        (120, 40),
        (120, 50),
        (140, 40),
    ];

    for &(w, h) in boundaries {
        let plan = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, w, h));
        let composer = plan
            .composer
            .unwrap_or_else(|| panic!("composer at {w}x{h}"));
        assert!(
            composer.height >= 3,
            "composer ≥3 rows at {w}x{h}; got {composer:?}"
        );
        assert!(
            composer.y + composer.height <= plan.shell.y + plan.shell.height,
            "composer fits in shell at {w}x{h}"
        );
        if let Some(disc) = plan.disclosure {
            assert!(
                disc.height >= 1,
                "disclosure ≥1 row at {w}x{h}; got {disc:?}"
            );
            assert!(
                disc.y + disc.height <= plan.shell.y + plan.shell.height,
                "disclosure fits in shell at {w}x{h}"
            );
        }
    }
}

/// Characterization: preserve the current full-width live-shell contract while
/// the measured dock rhythm is tightened below.
#[test]
fn characterization_current_live_shell_geometry_is_full_width_and_bottom_safe() {
    use harness_tui::FrameLayoutPlan;
    use ratatui::layout::Rect;

    let app = idle_shell_app();
    for (width, height) in [
        (60u16, 20u16),
        (79, 24),
        (80, 24),
        (100, 30),
        (120, 32),
        (120, 40),
        (120, 50),
        (140, 40),
    ] {
        let plan = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, width, height));
        let transcript = plan
            .transcript
            .unwrap_or_else(|| panic!("transcript at {width}x{height}"));
        let composer = plan
            .composer
            .unwrap_or_else(|| panic!("composer at {width}x{height}"));
        let inset = if width <= 60 { 1 } else { 2 };

        assert_eq!(transcript.x, plan.shell.x);
        assert_eq!(transcript.width, plan.shell.width);
        assert_eq!(composer.x, plan.shell.x + inset);
        assert_eq!(composer.width, plan.shell.width.saturating_sub(inset * 2));
        assert!(composer.y + composer.height <= plan.shell.y + plan.shell.height);
        assert_eq!(plan.disclosure, None);
    }
}

/// Regression: the pinned idle `120x40` frame keeps a three-row composer at
/// rows 34..=36, one blank row, then its disclosure at row 38.
#[test]
fn resp_120x40_matches_reference_dock_rhythm() {
    use harness_tui::FrameLayoutPlan;
    use ratatui::layout::Rect;

    let app = idle_shell_app();
    let plan = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, 120, 40));
    let composer = plan.composer.expect("composer at 120x40");
    assert_eq!(
        composer,
        Rect::new(2, 37, 116, 3),
        "idle 120x40 composer must retain the pinned three-row dock"
    );
    assert_eq!(plan.disclosure, None);
}
