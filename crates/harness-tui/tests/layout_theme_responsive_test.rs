//! Task 14: Layout engine, chrome, theme, and responsive geometry contracts.
//!
//! Proves (from outside the crate — true public contract surface):
//! - `FrameLayoutPlan` geometry correctness across the seven canonical viewports
//!   plus a startup and a replay shell.
//! - Theme color-token stability, semantic status roles, and centralised
//!   `Style` application from tokens.
//! - Responsive mode transitions (`SessionResponsiveMode`) follow the shared
//!   breakpoint table, and density selection maps targets to spacing density.
//! - Chrome element presence: header/footer bands, bordered composer, status
//!   band boundary at 33 rows, and permission-dock geometry.
//!
//! Companion owners: `reference_parity_responsive_test.rs` (RESP rendered rows),
//! `responsive_terminal_theme_mouse_clipboard_leaf_test.rs` (task 25 leaves),
//! `shell_topology_contract_test.rs` (topology).

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration parity tests use fail-fast asserts"
)]

use std::path::PathBuf;

use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionRequestedEvent, SCHEMA_VERSION,
};
use harness_tui::app::{AppState, LaunchMetadata};
use harness_tui::layout::{session_responsive_mode, FrameLayoutPlan, SessionResponsiveMode};
use harness_tui::render_test::render_to_string;
use harness_tui::responsive::{
    density_for_viewport, spacing_density_for, ViewportClassification, ViewportId, ViewportPlan,
};
use harness_tui::theme::{
    ChromeMode, DividerIntensity, ShellGeometryTarget, SpacingDensity, StatusRole, Theme,
};
use harness_tui::{ui, UnwrapOrAbort};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn idle_live_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-task14").with_mode_label("Demo"),
    );
    app
}

fn plan_at(app: &AppState, width: u16, height: u16) -> FrameLayoutPlan {
    FrameLayoutPlan::for_app(app, Rect::new(0, 0, width, height))
}

fn render_at(app: &AppState, width: u16, height: u16) -> String {
    render_to_string(app, Rect::new(0, 0, width, height), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-task14-{seq:04}"),
        seq,
        run_id: "run_task14_layout_theme".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("task14-parity".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_task14_layout_theme".to_string()),
        payload,
    }
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
            tool_call_id: Some(tool_call_id.into()),
            summary: "Apply hashline edit to demo.txt".to_string(),
            request_digest: format!("digest-{permission_id}"),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    )
}

// ---------------------------------------------------------------------------
// Group 1 — layout plan correctness for different viewports
// ---------------------------------------------------------------------------

/// The frame plan roots track the viewport exactly at every manifest size.
#[test]
fn frame_plan_roots_track_viewport_at_every_manifest_size() {
    // arrange
    let app = idle_live_app();

    for id in ViewportId::ALL {
        let (cols, rows) = id.dims();

        // act
        let plan = plan_at(&app, cols, rows);

        // assert
        assert_eq!(
            plan.root,
            Rect::new(0, 0, cols, rows),
            "{}: root must equal the viewport",
            id.behavior_id()
        );
        assert_eq!(
            plan.content.x,
            plan.root.x,
            "{}: content must share the root left edge",
            id.behavior_id()
        );
        assert!(
            plan.shell.width <= plan.content.width,
            "{}: shell must fit inside content; shell={:?} content={:?}",
            id.behavior_id(),
            plan.shell,
            plan.content
        );
        assert!(
            plan.shell.y >= plan.content.y && plan.shell.height <= plan.content.height,
            "{}: shell must fit vertically inside content",
            id.behavior_id()
        );
    }
}

/// Transcript spans the full shell width and stays strictly above the composer.
#[test]
fn transcript_spans_shell_and_stays_above_composer_at_every_viewport() {
    // arrange
    let app = idle_live_app();

    for id in ViewportId::ALL {
        let (cols, rows) = id.dims();

        // act
        let plan = plan_at(&app, cols, rows);
        let transcript = plan
            .transcript
            .unwrap_or_else(|| panic!("{}: transcript must exist", id.behavior_id()));
        let composer = plan
            .composer
            .unwrap_or_else(|| panic!("{}: composer must exist", id.behavior_id()));

        // assert
        assert_eq!(
            transcript.x,
            plan.shell.x,
            "{}: transcript must share the shell left edge",
            id.behavior_id()
        );
        assert_eq!(
            transcript.width,
            plan.shell.width,
            "{}: transcript must span the shell width",
            id.behavior_id()
        );
        assert!(
            transcript.y.saturating_add(transcript.height) <= composer.y,
            "{}: transcript must end at or above the composer; transcript={transcript:?} composer={composer:?}",
            id.behavior_id()
        );
    }
}

/// Live shells collapse header and footer chrome; startup reserves a footer.
#[test]
fn live_shell_collapses_chrome_while_startup_reserves_footer() {
    // arrange
    let live = idle_live_app();
    let startup = AppState::new_startup(Vec::new(), None);

    for id in ViewportId::ALL {
        let (cols, rows) = id.dims();

        // act
        let live_plan = plan_at(&live, cols, rows);
        let startup_plan = plan_at(&startup, cols, rows);

        // assert
        assert_eq!(
            live_plan.header.height,
            0,
            "{}: live shell must be headerless",
            id.behavior_id()
        );
        assert_eq!(
            live_plan.footer.height,
            0,
            "{}: live shell must not reserve a footer band",
            id.behavior_id()
        );
        assert_eq!(
            startup_plan.header.height,
            0,
            "{}: startup shell must be headerless",
            id.behavior_id()
        );
        assert_eq!(
            startup_plan.footer.height,
            2,
            "{}: startup shell reserves a 2-row footer",
            id.behavior_id()
        );
    }
}

#[test]
fn idle_live_dock_collapses_irrelevant_disclosure_and_unfocused_empty_composer() {
    // arrange
    let mut live = idle_live_app();
    live.focus = harness_tui::app::Focus::List;
    let startup = AppState::new_startup(Vec::new(), None);
    let replay = AppState::new_replay(PathBuf::from("/tmp/task14-replay-disclosure"), Vec::new());

    for (cols, rows) in [(100u16, 32u16), (100, 33), (120, 40), (60, 20)] {
        // act
        let live_plan = plan_at(&live, cols, rows);

        // assert
        assert!(
            live_plan.status.is_none(),
            "{cols}x{rows}: status must stay fused into the composer band; got {:?}",
            live_plan.status
        );
        assert!(live_plan.disclosure.is_none());
        assert_eq!(live_plan.composer.unwrap_or_abort().height, 1);
    }

    // assert
    assert!(
        plan_at(&startup, 120, 40).disclosure.is_none(),
        "startup shell carries no disclosure row"
    );
    assert!(
        plan_at(&replay, 120, 40).disclosure.is_none(),
        "replay shell carries no disclosure row"
    );

    live.focus = harness_tui::app::Focus::Prompt;
    assert_eq!(plan_at(&live, 120, 40).composer.unwrap_or_abort().height, 3);
}

/// Replay shells reserve header and footer bands and never claim a live anchor.
#[test]
fn replay_plan_reserves_header_footer_bands_without_live_anchor() {
    // arrange
    let app = AppState::new_replay(PathBuf::from("/tmp/task14-replay"), Vec::new());

    // act
    let plan = plan_at(&app, 120, 40);

    // assert
    assert_eq!(plan.header.height, 1, "replay reserves a 1-row header");
    assert_eq!(plan.footer.height, 1, "replay reserves a 1-row footer");
    assert_eq!(
        plan.header_text.width, plan.root.width,
        "replay header text spans the full root width"
    );
    assert!(
        plan.live_anchor.is_none(),
        "replay never reserves a live anchor"
    );
    assert!(
        plan.transcript.is_some() && plan.composer.is_some(),
        "replay keeps transcript and composer surfaces"
    );
}

/// Composer horizontal insets follow the narrow-viewport breakpoint.
#[test]
fn composer_inset_follows_narrow_viewport_breakpoint() {
    // arrange
    let app = idle_live_app();

    // act
    let narrow = plan_at(&app, 60, 20).composer.unwrap_or_abort();
    let wide = plan_at(&app, 120, 40).composer.unwrap_or_abort();

    // assert
    // Dense viewports (width <= DENSE_SESSION_MAX_WIDTH) keep the measured
    // one-cell composer inset; see composer_horizontal_inset + reference fixtures.
    assert_eq!(
        narrow.x, 1,
        "dense viewports (<=60 cols) use a one-cell composer inset"
    );
    assert_eq!(wide.x, 2, "wide viewports use a 2-column composer inset");
}

// ---------------------------------------------------------------------------
// Group 2 — theme color token stability and style application
// ---------------------------------------------------------------------------

/// Theme switching resolves documented names and rejects unknown ones.
#[test]
fn theme_switching_resolves_documented_names_only() {
    // arrange
    // act
    // assert
    assert_eq!(Theme::by_name("default"), Some(Theme::harness_chat()));
    assert_eq!(Theme::by_name("harness-chat"), Some(Theme::harness_chat()));
    assert_eq!(Theme::by_name("harness-dark"), Some(Theme::harness_dark()));
    assert_eq!(
        Theme::by_name("harness-light"),
        Some(Theme::harness_light())
    );
    assert_eq!(
        Theme::by_name("high-contrast"),
        Some(Theme::harness_high_contrast())
    );
    assert_eq!(Theme::by_name("solarized"), None);
    assert_eq!(
        Theme::available_theme_names(),
        ["default", "harness-light", "high-contrast"]
    );
}

/// Semantic status roles map to distinct, stable colors in every theme.
#[test]
fn status_roles_map_to_distinct_stable_colors() {
    // arrange
    // act
    // assert
    for theme in [Theme::harness_dark(), Theme::harness_high_contrast()] {
        let colors = [
            theme.status.success,
            theme.status.warning,
            theme.status.error,
            theme.status.info,
        ];
        for (index, color) in colors.iter().enumerate() {
            assert!(
                !colors[..index].contains(color),
                "status role colors must be pairwise distinct; duplicate at index {index}"
            );
        }
        assert_ne!(
            theme.text.primary, theme.surface.canvas,
            "primary text must contrast the canvas"
        );
    }
}

/// Style application derives directly from theme tokens.
#[test]
fn style_application_derives_from_theme_tokens() {
    // arrange
    let theme = Theme::default();

    // act
    // assert
    assert_eq!(
        theme.primary_text_style(),
        Style::new().fg(theme.text.primary),
        "primary text style must carry the text.primary token"
    );
    assert_eq!(
        theme.secondary_text_style(),
        Style::new().fg(theme.text.secondary),
        "secondary text style must carry the text.secondary token"
    );
    assert_eq!(
        theme.accent_text_style(),
        Style::new()
            .fg(theme.text.accent)
            .add_modifier(Modifier::BOLD),
        "accent text style must carry the accent token and bold emphasis"
    );
    assert_eq!(
        theme.status_style(StatusRole::Success),
        Style::new().fg(theme.status.success)
    );
    assert_eq!(
        theme.status_style(StatusRole::Warning),
        Style::new().fg(theme.status.warning)
    );
    assert_eq!(
        theme.status_style(StatusRole::Error),
        Style::new().fg(theme.status.error)
    );
    assert_eq!(
        theme.status_style(StatusRole::Info),
        Style::new().fg(theme.status.info)
    );
    assert_eq!(
        theme.status_style(StatusRole::Disabled),
        Style::new().fg(theme.status.disabled)
    );
    assert_eq!(
        theme.border_style(DividerIntensity::None),
        Style::new(),
        "no-divider intensity must produce an unstyled result"
    );
    assert_eq!(
        theme.border_style(DividerIntensity::Subtle),
        Style::new().fg(theme.border.subtle)
    );
    assert_eq!(
        theme.border_style(DividerIntensity::Strong),
        Style::new().fg(theme.border.strong)
    );
    assert_eq!(
        theme.border_style(DividerIntensity::Focus),
        Style::new().fg(theme.border.focus)
    );
    assert_eq!(
        theme.chrome_style(ChromeMode::Chromeless),
        Style::new().bg(theme.surface.shell),
        "chromeless surfaces carry the shell background without a border color"
    );
    assert_eq!(
        theme.chrome_style(ChromeMode::Divided),
        Style::new().bg(theme.surface.panel).fg(theme.border.subtle),
        "divided chrome pairs the panel background with the subtle border"
    );
    assert_eq!(
        theme.chrome_style(ChromeMode::Card),
        Style::new()
            .bg(theme.surface.overlay)
            .fg(theme.border.subtle),
        "card chrome pairs the overlay background with the subtle border"
    );
}

/// Token families expose the documented chrome and composer chrome modes.
#[test]
fn token_families_expose_chrome_and_composer_contracts() {
    // arrange
    let theme = Theme::default();

    // act
    let families = theme.token_families();

    // assert
    assert_eq!(
        families.semantic.chrome.divided.border.intensity,
        DividerIntensity::Subtle
    );
    assert_eq!(
        families.semantic.chrome.divided.border.color,
        Some(theme.border.subtle)
    );
    assert!(
        families.semantic.chrome.chromeless.border.color.is_none(),
        "chromeless chrome must not carry a border color"
    );
    assert_eq!(
        families.semantic.composer.minimum.chrome,
        ChromeMode::Card,
        "minimum composer chrome uses card presentation"
    );
    assert_eq!(
        families.semantic.composer.split.chrome,
        ChromeMode::Divided,
        "split composer chrome uses divided presentation"
    );
    assert_eq!(
        families.semantic.composer.primary.chrome,
        ChromeMode::Divided,
        "primary composer chrome uses divided presentation"
    );
}

#[test]
fn agent_accent_selection_is_profile_independent() {
    // arrange
    // act
    // assert
    for theme in [Theme::harness_dark(), Theme::harness_high_contrast()] {
        for profile in ["zebra-lane", "worker", "Worker", "build", "plan"] {
            assert_eq!(theme.agent_accent(profile), theme.text.accent);
        }
    }
}

// ---------------------------------------------------------------------------
// Group 3 — responsive mode transitions and density selection
// ---------------------------------------------------------------------------

/// Session responsive mode follows the breakpoint table across boundaries.
#[test]
fn session_mode_transitions_follow_breakpoint_boundaries() {
    // arrange
    let theme = Theme::default();

    let cases = [
        ((60u16, 18u16), SessionResponsiveMode::Dense),
        ((60, 20), SessionResponsiveMode::CompactMinimum),
        ((79, 24), SessionResponsiveMode::CompactMinimum),
        ((80, 24), SessionResponsiveMode::CompactMinimum),
        ((81, 24), SessionResponsiveMode::StandardMinimum),
        ((90, 35), SessionResponsiveMode::StandardMinimum),
        ((90, 36), SessionResponsiveMode::Split),
        ((100, 29), SessionResponsiveMode::StandardMinimum),
        ((100, 30), SessionResponsiveMode::Primary),
        ((120, 40), SessionResponsiveMode::Primary),
        ((120, 50), SessionResponsiveMode::Primary),
        ((140, 40), SessionResponsiveMode::Primary),
    ];

    for ((cols, rows), expected) in cases {
        // act
        let mode = session_responsive_mode(
            Rect::new(0, 0, cols, rows),
            theme.live_shell_layout(cols, rows),
        );

        // assert
        assert_eq!(
            mode, expected,
            "{cols}x{rows}: responsive mode must follow the breakpoint table"
        );
    }
}

/// The responsive density bridge agrees with the layout-owned mode selection.
#[test]
fn density_bridge_agrees_with_layout_mode_selection() {
    // arrange
    let theme = Theme::default();

    for id in ViewportId::ALL {
        let (cols, rows) = id.dims();

        // act
        let bridged = density_for_viewport(&theme, cols, rows);
        let direct = session_responsive_mode(
            Rect::new(0, 0, cols, rows),
            theme.live_shell_layout(cols, rows),
        );

        // assert
        assert_eq!(
            bridged,
            direct,
            "{}: density bridge must match the layout mode selection",
            id.behavior_id()
        );
    }

    assert_eq!(
        density_for_viewport(&theme, 60, 20),
        SessionResponsiveMode::CompactMinimum,
        "60x20 (above the dense height cut) stays compact-minimum"
    );
    assert_eq!(
        density_for_viewport(&theme, 60, 18),
        SessionResponsiveMode::Dense,
        "60x18 hits the dense cut exactly"
    );
}

/// Spacing density selection maps geometry targets without gaps.
#[test]
fn spacing_density_selection_maps_every_geometry_target() {
    // arrange
    let theme = Theme::default();

    // act
    // assert
    assert_eq!(
        spacing_density_for(&theme, ShellGeometryTarget::Minimum),
        SpacingDensity::Compact
    );
    assert_eq!(
        spacing_density_for(&theme, ShellGeometryTarget::Split),
        SpacingDensity::Standard
    );
    assert_eq!(
        spacing_density_for(&theme, ShellGeometryTarget::Primary),
        SpacingDensity::Roomy
    );

    for target in [
        ShellGeometryTarget::Minimum,
        ShellGeometryTarget::Split,
        ShellGeometryTarget::Primary,
    ] {
        let via_bridge = spacing_density_for(&theme, target);
        let via_families = theme
            .token_families()
            .semantic
            .density
            .select(target)
            .density;
        assert_eq!(
            via_bridge, via_families,
            "bridge and token-family density must agree for {target:?}"
        );
    }
}

/// Viewport classifications stay consistent with session responsive modes.
#[test]
fn viewport_classifications_align_with_session_modes() {
    // arrange
    let theme = Theme::default();

    for plan in ViewportPlan::all_plans() {
        let (cols, rows) = plan.id.dims();
        let classification = ViewportClassification::from_dims(cols, rows);
        assert_eq!(
            classification,
            plan.classification,
            "{}: plan classification must match from_dims",
            plan.id.behavior_id()
        );

        // act
        let mode = density_for_viewport(&theme, cols, rows);

        // assert
        if classification.is_compact() {
            assert_eq!(
                mode,
                SessionResponsiveMode::CompactMinimum,
                "{}: compact classification maps to compact-minimum mode",
                plan.id.behavior_id()
            );
        }
        if classification.is_primary() {
            assert_eq!(
                mode,
                SessionResponsiveMode::Primary,
                "{}: primary classification maps to primary mode",
                plan.id.behavior_id()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Group 4 — chrome element presence
// ---------------------------------------------------------------------------

/// The idle live shell renders exactly one bordered surface (the composer).
#[test]
fn idle_live_shell_renders_bordered_composer_and_prompt_glyph() {
    // arrange
    let app = idle_live_app();

    // act
    let rendered = render_at(&app, 120, 40);
    let plan = plan_at(&app, 120, 40);

    // assert
    let box_corners = rendered.chars().filter(|ch| *ch == '╭').count();
    assert_eq!(
        box_corners, 1,
        "idle live shell renders exactly one bordered box\n{rendered}"
    );
    assert!(
        rendered.contains('❯'),
        "idle live shell renders the prompt glyph\n{rendered}"
    );
    assert_eq!(
        plan.header.height, 0,
        "chrome is collapsed: no header band in the plan"
    );
}

/// The startup shell renders bordered chrome and reserves a footer band.
#[test]
fn startup_shell_renders_border_chrome_with_footer_band() {
    // arrange
    let app = AppState::new_startup(Vec::new(), None);

    // act
    let rendered = render_at(&app, 80, 24);
    let plan = plan_at(&app, 80, 24);
    let dock = plan.dock.unwrap_or_abort();

    // assert
    assert!(
        rendered.contains('╭') && rendered.contains('╰'),
        "startup shell renders bordered composer chrome\n{rendered}"
    );
    assert_eq!(plan.footer.height, 2, "startup footer band is reserved");
    assert_eq!(dock.composer.height, 3, "empty startup composer is 3 rows");
    assert!(
        dock.shell.y.saturating_add(dock.shell.height)
            <= plan.content.y.saturating_add(plan.content.height),
        "startup dock stays inside the content area"
    );
}

/// A pending permission attaches its fixed tray above the stable composer.
#[test]
fn permission_dock_attaches_fixed_tray_above_stable_composer() {
    // arrange
    let mut app = idle_live_app();
    let idle_composer = plan_at(&app, 120, 40).composer;
    app.ingest_event(permission_requested_event(
        1,
        "perm-task14-1",
        "tool-call-1",
    ));
    assert!(
        app.active_permission().is_some(),
        "permission event must activate the dock"
    );

    // act
    let plan = plan_at(&app, 120, 40);
    let rendered = render_at(&app, 120, 40);
    let dock = plan.dock.unwrap_or_abort();

    // assert
    assert_eq!(
        plan.header.height, 0,
        "permission dock keeps the headerless shell"
    );
    let status = plan
        .status
        .expect("permission dock must reserve an attached status tray");
    assert_eq!(status.height, 11, "permission tray uses the freeze height");
    assert_eq!(
        plan.composer, idle_composer,
        "permission tray must not move or resize the composer"
    );
    assert!(
        dock.composer.width > 0,
        "permission dock keeps a usable composer band"
    );
    assert!(
        rendered.contains('┃') && rendered.contains('●'),
        "permission dock renders its rail and radio chrome\n{rendered}"
    );
}

/// Wheel hit areas track the transcript and stay clear of inactive surfaces.
#[test]
fn wheel_hit_areas_track_transcript_without_inactive_surfaces() {
    // arrange
    let app = idle_live_app();

    for id in ViewportId::ALL {
        let (cols, rows) = id.dims();

        // act
        let plan = plan_at(&app, cols, rows);

        // assert
        assert!(
            plan.wheel_hit_areas.transcript.is_some(),
            "{}: transcript must own a wheel hit area",
            id.behavior_id()
        );
        assert!(
            plan.wheel_hit_areas.terminal_panel.is_none(),
            "{}: terminal panel is off by default",
            id.behavior_id()
        );
        assert!(
            plan.wheel_hit_areas.overlay.is_none(),
            "{}: idle shell keeps no overlay hit area",
            id.behavior_id()
        );
        assert!(
            plan.wheel_hit_areas.inspector.is_none(),
            "{}: idle shell keeps no inspector hit area",
            id.behavior_id()
        );
    }
}
