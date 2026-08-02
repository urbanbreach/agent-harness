#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration test helpers use fail-fast messages for missing layout rects"
)]

use std::path::PathBuf;

use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestStartedEvent,
    ProviderStreamDeltaEvent, RunStartedEvent, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_tui::app::AppState;
use harness_tui::FrameLayoutPlan;
use ratatui::layout::Rect;

const CONTRACT_VIEWPORTS: [(u16, u16); 3] = [(120, 40), (100, 30), (80, 24)];
const WIDE_VIEWPORTS: [(u16, u16); 3] = [(121, 40), (140, 36), (160, 40)];

#[test]
fn live_session_shell_has_no_right_operator_sidebar_at_contract_viewports() {
    // arrange
    let app = live_session_app();

    for (width, height) in CONTRACT_VIEWPORTS {
        // act
        let plan = plan_for(&app, width, height);
        // assert
        assert_no_operator_rail_primary_chrome(&plan, width, height);
    }
}

#[test]
fn live_session_transcript_spans_full_shell_width_above_composer() {
    // arrange
    let app = live_session_app();

    for (width, height) in CONTRACT_VIEWPORTS {
        // act
        let plan = plan_for(&app, width, height);
        // assert
        assert_full_width_transcript_above_composer(&plan, width, height);
    }
}

#[test]
fn live_session_composer_is_bottom_anchored() {
    // arrange
    let app = live_session_app();

    for (width, height) in CONTRACT_VIEWPORTS {
        // act
        let plan = plan_for(&app, width, height);
        // assert
        assert_composer_bottom_anchored(&plan, width, height);
    }
}

#[test]
fn live_session_sidebar_must_not_reappear_as_primary_chrome_at_width_ge_121() {
    // arrange
    let app = live_session_app();

    for (width, height) in WIDE_VIEWPORTS {
        // assert
        assert!(
            width >= 121,
            "wide-viewport matrix must stay at/above the 121-column threshold"
        );
        // act
        let plan = plan_for(&app, width, height);
        // assert
        assert_no_operator_rail_primary_chrome(&plan, width, height);
        assert_full_width_transcript_above_composer(&plan, width, height);
        assert_composer_bottom_anchored(&plan, width, height);
    }
}

#[test]
fn live_session_replacement_topology_holds_across_all_named_viewports() {
    // arrange
    let app = live_session_app();
    let mut cases = Vec::with_capacity(CONTRACT_VIEWPORTS.len() + WIDE_VIEWPORTS.len());
    cases.extend(CONTRACT_VIEWPORTS);
    cases.extend(WIDE_VIEWPORTS);

    // act
    for (width, height) in cases {
        let plan = plan_for(&app, width, height);
        // assert
        assert_no_operator_rail_primary_chrome(&plan, width, height);
        assert_full_width_transcript_above_composer(&plan, width, height);
        assert_composer_bottom_anchored(&plan, width, height);
    }
}

#[test]
fn operator_sidebar_chrome_has_no_persistent_primary_variant() {
    // arrange
    let secondary_src = include_str!("../src/ui_secondary.rs");
    let interaction_src = include_str!("../src/ui_secondary/sidebar_interaction.rs");
    let ui_src = include_str!("../src/ui.rs");

    // act
    let chrome_absent = !secondary_src.contains("OperatorSidebarChrome")
        && !interaction_src.contains("OperatorSidebarChrome")
        && !ui_src.contains("OperatorSidebarChrome");
    let persistent_absent = !secondary_src.contains("OperatorSidebarChrome::Persistent")
        && !interaction_src.contains("OperatorSidebarChrome::Persistent")
        && !ui_src.contains("OperatorSidebarChrome::Persistent")
        && !secondary_src.contains("enum OperatorSidebarChrome");
    let mut app = live_session_app();

    // assert — no persistent primary chrome
    assert!(
        chrome_absent,
        "P0-SHELL-02: OperatorSidebarChrome must be removed (overlay-only secondary surface)"
    );
    assert!(
        persistent_absent,
        "P0-SHELL-02: Persistent chrome path must not ship"
    );
    for (width, height) in WIDE_VIEWPORTS {
        let plan = plan_for(&app, width, height);
        assert!(
            plan.operator_sidebar.is_none(),
            "P0-SHELL-02: live FrameLayoutPlan.operator_sidebar must be None at {width}x{height}"
        );
        assert_full_width_transcript_above_composer(&plan, width, height);
    }

    // act — secondary operator facts via status dialog (leader+s / Ctrl+x s)
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('x'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('s'),
        crossterm::event::KeyModifiers::NONE,
    ));
    let status_render = {
        use harness_tui::render_test::render_to_string;
        use harness_tui::ui;
        use ratatui::layout::Rect;
        render_to_string(&app, Rect::new(0, 0, 120, 40), |app, frame, _area| {
            ui::render_app(frame, app)
        })
    };
    let status_plan = plan_for(&app, 120, 40);
    let status_overlay = app.overlay_stack().top();

    // assert — status dialog reveals operator facts under full-width shell
    assert!(
        matches!(
            status_overlay,
            Some(harness_tui::overlay::OverlayKind::StatusDialog)
        ),
        "P0-SHELL-02: status dialog must open as OverlayKind::StatusDialog; overlay={status_overlay:?}\n{status_render}"
    );
    assert!(
        status_plan.operator_sidebar.is_none(),
        "P0-SHELL-02: status dialog must not allocate a primary operator sidebar"
    );
    assert!(
        status_render.contains("Status"),
        "P0-SHELL-02: status dialog must present Status header\n{status_render}"
    );

    // act — palette also remains a secondary operator-facts route
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::NONE,
    ));
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    let palette_plan = plan_for(&app, 120, 40);
    assert!(
        palette_plan.operator_sidebar.is_none(),
        "P0-SHELL-02: palette must not reintroduce primary sidebar chrome"
    );
}

fn live_session_app() -> AppState {
    let mut app = AppState::new_live(
        Some(PathBuf::from("/tmp/run_topology_contract")),
        false,
        None,
    );
    for event in live_session_events() {
        app.ingest_event(event);
    }
    app
}

fn plan_for(app: &AppState, width: u16, height: u16) -> FrameLayoutPlan {
    FrameLayoutPlan::for_app(app, Rect::new(0, 0, width, height))
}

fn assert_no_operator_rail_primary_chrome(plan: &FrameLayoutPlan, width: u16, height: u16) {
    assert!(
        plan.operator_sidebar.is_none(),
        "replacement topology forbids a dedicated right operator-sidebar rect at {width}x{height}; got {:?}",
        plan.operator_sidebar
    );
    assert!(
        plan.details_overlay.is_none(),
        "replacement topology forbids operator-rail primary chrome via details overlay at {width}x{height}; got {:?}",
        plan.details_overlay
    );
    assert!(
        plan.wheel_hit_areas.inspector.is_none(),
        "replacement topology forbids an operator-rail inspector hit target as primary chrome at {width}x{height}; got {:?}",
        plan.wheel_hit_areas.inspector
    );
    assert!(
        plan.wheel_hit_areas.overlay.is_none(),
        "replacement topology forbids an operator-rail overlay hit target as primary chrome at {width}x{height}; got {:?}",
        plan.wheel_hit_areas.overlay
    );
}

fn assert_full_width_transcript_above_composer(plan: &FrameLayoutPlan, width: u16, height: u16) {
    let transcript = plan.transcript.unwrap_or_else(|| {
        panic!("live session shell must keep a transcript surface at {width}x{height}")
    });
    let composer = plan.composer.unwrap_or_else(|| {
        panic!("live session shell must keep a composer rect at {width}x{height}")
    });

    assert_eq!(
        transcript.width, plan.shell.width,
        "transcript must span full shell content width at {width}x{height} (no right-rail reservation); transcript={transcript:?} shell={:?}",
        plan.shell
    );
    // Freeze-matched composer inset: one cell at 60 columns, two cells above it.
    let composer_inset = if width <= 60 { 1 } else { 2 };
    assert_eq!(
        composer.x,
        plan.shell.x.saturating_add(composer_inset),
        "composer must keep freeze horizontal inset at {width}x{height}; composer={composer:?} shell={:?}",
        plan.shell
    );
    assert_eq!(
        composer.width,
        plan.shell
            .width
            .saturating_sub(composer_inset.saturating_mul(2)),
        "composer width must keep freeze horizontal inset at {width}x{height}; composer={composer:?} shell={:?}",
        plan.shell
    );
    assert_eq!(
        transcript.x, plan.shell.x,
        "transcript must share the shell left edge at {width}x{height}"
    );
    assert!(
        transcript.y + transcript.height <= composer.y,
        "transcript/scrollback must sit above the composer at {width}x{height}; transcript={transcript:?} composer={composer:?}"
    );
}

fn assert_composer_bottom_anchored(plan: &FrameLayoutPlan, width: u16, height: u16) {
    let composer = plan.composer.unwrap_or_else(|| {
        panic!("live session shell must keep a composer rect at {width}x{height}")
    });

    let dock_bottom = match plan.disclosure {
        Some(disclosure) => disclosure.y + disclosure.height + 1,
        None => composer.y + composer.height,
    };

    assert_eq!(
        dock_bottom,
        plan.shell.y + plan.shell.height,
        "composer dock stack must be bottom-anchored in the live shell at {width}x{height}; composer={composer:?} disclosure={:?} shell={:?}",
        plan.disclosure,
        plan.shell
    );
    assert!(
        composer.y + composer.height <= plan.shell.y + plan.shell.height,
        "composer must not extend past the shell bottom at {width}x{height}; composer={composer:?} shell={:?}",
        plan.shell
    );
    if let Some(transcript) = plan.transcript {
        assert!(
            composer.y >= transcript.y + transcript.height,
            "bottom-anchored composer must remain below transcript/scrollback at {width}x{height}; composer={composer:?} transcript={transcript:?}"
        );
    }
}

fn live_session_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_topology_contract";
    vec![
        envelope(
            1,
            Some(request_id),
            EventV1::RunStarted(RunStartedEvent {
                run_name: "topology-contract".into(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: "Prove replacement shell topology".to_string(),
            }),
        ),
        envelope(
            3,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "topology-model".to_string(),
                prompt_summary: "Prove replacement shell topology".to_string(),
                request_digest: "digest-topology-contract".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            4,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: "Transcript content for topology geometry checks.".to_string(),
            }),
        ),
    ]
}

fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-topology-{seq:04}"),
        seq,
        run_id: "run_topology_contract".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(
            ActorKind::System,
            Some("shell-topology-contract".to_string()),
        ),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_topology_contract".to_string()),
        payload,
    }
}

// ---------------------------------------------------------------------------
// Boundary tests: 59/60/61, 79/80/81, 99/100/101, 119/120/121 columns
// ---------------------------------------------------------------------------

/// Boundary viewports around the dense (60-col), compact (80-col),
/// primary (100-col), and wide (120-col) breakpoints. Verifies that the
/// composer and disclosure/footer are never clipped at any boundary.
#[test]
fn boundary_viewports_never_clip_composer_or_disclosure() {
    let app = live_session_app();
    let boundary_cases: &[(u16, u16)] = &[
        // 59/60/61 columns × required heights
        (59, 20),
        (60, 20),
        (61, 20),
        // 79/80/81 columns × required heights
        (79, 24),
        (80, 24),
        (81, 24),
        // 99/100/101 columns × required heights
        (99, 30),
        (100, 30),
        (101, 30),
        // 119/120/121 columns × required heights
        (119, 32),
        (120, 32),
        (121, 32),
        // Also verify tall variants for the 120-col boundary
        (119, 40),
        (120, 40),
        (121, 40),
    ];

    for &(width, height) in boundary_cases {
        let plan = plan_for(&app, width, height);

        // Composer must exist and have at least 3 rows (border + content + border)
        let composer = plan
            .composer
            .unwrap_or_else(|| panic!("composer must exist at boundary {width}x{height}"));
        assert!(
            composer.height >= 3,
            "composer must have ≥3 rows at boundary {width}x{height}; got {composer:?}"
        );
        assert!(
            composer.y + composer.height <= plan.shell.y + plan.shell.height,
            "composer must not extend past shell bottom at boundary {width}x{height}; composer={composer:?} shell={:?}",
            plan.shell
        );

        // Disclosure (footer) must exist and have at least 1 row
        if let Some(disclosure) = plan.disclosure {
            assert!(
                disclosure.height >= 1,
                "disclosure must have ≥1 row at boundary {width}x{height}; got {disclosure:?}"
            );
            assert!(
                disclosure.y + disclosure.height <= plan.shell.y + plan.shell.height,
                "disclosure must not extend past shell bottom at boundary {width}x{height}; disclosure={disclosure:?} shell={:?}",
                plan.shell
            );
        }

        // Transcript must exist and have at least 1 row
        if let Some(transcript) = plan.transcript {
            assert!(
                transcript.height >= 1,
                "transcript must have ≥1 row at boundary {width}x{height}; got {transcript:?}"
            );
            assert!(
                transcript.y + transcript.height <= composer.y,
                "transcript must sit above composer at boundary {width}x{height}; transcript={transcript:?} composer={composer:?}"
            );
        }
    }
}

/// Boundary viewport 59/60/61: composer-footer spacer is 0 at ≤60, 1 at >60.
#[test]
fn boundary_spacer_transitions_at_60_column_cutoff() {
    let app = live_session_app();

    // At 60 columns: no spacer (ultra-compact)
    for &height in &[20u16, 24, 30, 40] {
        let plan = plan_for(&app, 60, height);
        let composer = plan.composer.expect("composer at 60 cols");
        let disclosure = plan
            .disclosure
            .unwrap_or_else(|| panic!("disclosure at 60x{height}"));
        let gap = disclosure.y.saturating_sub(composer.y + composer.height);
        assert_eq!(
            gap, 0,
            "spacer must be 0 at 60x{height} (ultra-compact); got gap={gap}"
        );
    }

    // At 61 columns: spacer present
    for &height in &[20u16, 24, 30, 40] {
        let plan = plan_for(&app, 61, height);
        let composer = plan.composer.expect("composer at 61 cols");
        let disclosure = plan
            .disclosure
            .unwrap_or_else(|| panic!("disclosure at 61x{height}"));
        let gap = disclosure.y.saturating_sub(composer.y + composer.height);
        assert_eq!(gap, 1, "spacer must be 1 at 61x{height}; got gap={gap}");
    }
}

/// Boundary breakpoint transitions: 79→80 (minimum), 99→100 (primary), 119→120.
#[test]
fn boundary_breakpoint_targets_match_theme_contract() {
    use harness_tui::responsive::ViewportClassification;

    // 79 cols → Minimum (below 80-col minimum breakpoint)
    assert!(ViewportClassification::from_dims(79, 24).is_compact());
    // 80 cols → Minimum (at minimum breakpoint)
    assert!(ViewportClassification::from_dims(80, 24).is_compact());
    // 81 cols → Minimum (still below 90-col split)
    assert!(ViewportClassification::from_dims(81, 24).is_compact());

    // 99 cols → Minimum (below 100-col primary)
    assert!(ViewportClassification::from_dims(99, 30).is_compact());
    // 100 cols → Primary
    assert!(ViewportClassification::from_dims(100, 30).is_primary());
    // 101 cols → Primary
    assert!(ViewportClassification::from_dims(101, 30).is_primary());

    // 119 cols × 32 → Primary (below 120 is not a breakpoint, but verify)
    assert!(ViewportClassification::from_dims(119, 32).is_primary());
    assert!(ViewportClassification::from_dims(120, 32).is_primary());
    assert!(ViewportClassification::from_dims(121, 32).is_primary());
}

/// Composer horizontal inset transitions at the 60-column boundary.
#[test]
fn boundary_composer_inset_transitions_at_60_columns() {
    let app = live_session_app();

    // At ≤60 cols: retain the reference's one-cell outer inset.
    let plan_60 = plan_for(&app, 60, 20);
    let composer_60 = plan_60.composer.expect("composer at 60x20");
    assert_eq!(
        composer_60.x,
        plan_60.shell.x + 1,
        "composer must have a one-cell inset at 60x20"
    );
    assert_eq!(
        composer_60.width,
        plan_60.shell.width - 2,
        "composer must keep the one-cell inset on both sides at 60x20"
    );

    // At >60 cols: horizontal inset of 2 (freeze-matched lead=2)
    let plan_61 = plan_for(&app, 61, 20);
    let composer_61 = plan_61.composer.expect("composer at 61x20");
    assert_eq!(
        composer_61.x,
        plan_61.shell.x + 2,
        "composer must have 2-col inset at 61x20"
    );
    assert_eq!(
        composer_61.width,
        plan_61.shell.width - 4,
        "composer must keep 2-col inset on both sides at 61x20"
    );
}

/// All required viewports produce a valid, non-clipped layout plan.
#[test]
fn all_required_viewports_produce_valid_layout_plan() {
    let app = live_session_app();
    let required: &[(u16, u16)] = &[
        (60, 20),
        (79, 24),
        (80, 24),
        (100, 30),
        (120, 32),
        (120, 40),
        (120, 50),
        (140, 40),
    ];

    for &(width, height) in required {
        let plan = plan_for(&app, width, height);

        // Shell must be non-empty
        assert!(
            plan.shell.width > 0 && plan.shell.height > 0,
            "shell must be non-empty at {width}x{height}"
        );

        // Composer must exist and fit within shell
        let composer = plan
            .composer
            .unwrap_or_else(|| panic!("composer at {width}x{height}"));
        assert!(
            composer.width > 0 && composer.height >= 3,
            "composer must be valid at {width}x{height}; got {composer:?}"
        );
        assert!(
            composer.y + composer.height <= plan.shell.y + plan.shell.height,
            "composer must fit within shell at {width}x{height}"
        );

        // Transcript must exist and be above composer
        let transcript = plan
            .transcript
            .unwrap_or_else(|| panic!("transcript at {width}x{height}"));
        assert!(
            transcript.height >= 1,
            "transcript must have ≥1 row at {width}x{height}"
        );
        assert!(
            transcript.y + transcript.height <= composer.y,
            "transcript must be above composer at {width}x{height}"
        );
    }
}

// ---------------------------------------------------------------------------
// Seam-level regression: composer→disclosure spacer gap at every required viewport
// ---------------------------------------------------------------------------

/// The composer→disclosure spacer gap must be 0 at ultra-compact (≤60 cols)
/// and exactly 1 row at every wider required viewport. This locks the
/// centralized `composer_footer_spacer_rows` contract via the public
/// `FrameLayoutPlan` seam across all eight required viewports, preventing
/// the missing-spacer regression from recurring.
#[test]
fn composer_disclosure_spacer_gap_matches_centralized_contract_at_all_viewports() {
    let app = live_session_app();

    // (width, height, expected_gap) — gap is 0 only at ≤60 cols.
    let cases: &[(u16, u16, u16)] = &[
        (60, 20, 0),
        (79, 24, 1),
        (80, 24, 1),
        (100, 30, 1),
        (120, 32, 1),
        (120, 40, 1),
        (120, 50, 1),
        (140, 40, 1),
    ];

    for &(width, height, expected_gap) in cases {
        let plan = plan_for(&app, width, height);
        let composer = plan
            .composer
            .unwrap_or_else(|| panic!("composer at {width}x{height}"));
        let disclosure = plan
            .disclosure
            .unwrap_or_else(|| panic!("disclosure at {width}x{height}"));

        let actual_gap = disclosure.y.saturating_sub(composer.y + composer.height);
        assert_eq!(
            actual_gap, expected_gap,
            "composer→disclosure spacer at {width}x{height}: expected {expected_gap}, got {actual_gap}; \
             composer={composer:?} disclosure={disclosure:?}"
        );
    }
}

/// Composer horizontal inset must be 1 at ultra-compact (≤60 cols) and 2 at
/// every wider required viewport. Locks the centralized
/// `composer_horizontal_inset` contract via the public seam.
#[test]
fn composer_horizontal_inset_matches_centralized_contract_at_all_viewports() {
    let app = live_session_app();

    let cases: &[(u16, u16, u16)] = &[
        (60, 20, 1),
        (79, 24, 2),
        (80, 24, 2),
        (100, 30, 2),
        (120, 32, 2),
        (120, 40, 2),
        (120, 50, 2),
        (140, 40, 2),
    ];

    for &(width, height, expected_inset) in cases {
        let plan = plan_for(&app, width, height);
        let composer = plan
            .composer
            .unwrap_or_else(|| panic!("composer at {width}x{height}"));

        let actual_inset = composer.x.saturating_sub(plan.shell.x);
        assert_eq!(
            actual_inset, expected_inset,
            "composer inset at {width}x{height}: expected {expected_inset}, got {actual_inset}; \
             composer={composer:?} shell={:?}",
            plan.shell
        );
        assert_eq!(
            composer.width,
            plan.shell.width.saturating_sub(expected_inset * 2),
            "composer width must reflect inset on both sides at {width}x{height}; \
             composer={composer:?} shell={:?}",
            plan.shell
        );
    }
}
