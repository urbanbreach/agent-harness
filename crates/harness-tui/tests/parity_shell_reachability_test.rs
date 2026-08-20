use harness_tui::app::AppState;
use harness_tui::layout::FrameLayoutPlan;
use harness_tui::render_test::render_to_buffer;
use harness_tui::theme_family::ThemeFamily;
use harness_tui::ui;
use ratatui::layout::Rect;

const APP_SOURCE: &str = include_str!("../src/app.rs");
const LIB_SOURCE: &str = include_str!("../src/lib.rs");
const LAYOUT_SOURCE: &str = include_str!("../src/layout.rs");
const RUNTIME_SOURCE: &str = include_str!("../src/runtime.rs");
const RUNTIME_INTEGRATION_SOURCE: &str = include_str!("../src/runtime_integration.rs");
const THEME_SOURCE: &str = include_str!("../src/theme.rs");
const UI_SOURCE: &str = include_str!("../src/ui.rs");
const UI_LIFECYCLE_SOURCE: &str = include_str!("../src/ui_lifecycle.rs");

fn assert_source_order(source: &str, markers: &[&str]) {
    let mut offset = 0;
    for marker in markers {
        let relative = source[offset..].find(marker);
        assert!(
            relative.is_some(),
            "source is missing ordered marker {marker:?}"
        );
        let relative = relative.unwrap_or_default();
        offset = offset.saturating_add(relative + marker.len());
    }
}

fn render(app: &AppState, width: u16, height: u16) -> ratatui::buffer::Buffer {
    render_to_buffer(app, Rect::new(0, 0, width, height), |app, frame, _| {
        ui::render_app(frame, app);
    })
}

#[test]
fn live_shell_call_chain_keeps_one_parity_renderer_owner() {
    // arrange
    // Given: the production runtime and renderer source, not a test-only facade.
    // When: the live frame ownership seams are checked in their execution order.
    assert_source_order(
        RUNTIME_SOURCE,
        &[
            "pub fn run_tui_with_options",
            "AppState::new_live_with_session_history_and_prompt_history_path",
            "let mut experience = RuntimeExperience::new()",
            "experience.tick(&app)",
            "ui::render_app(frame, &app)",
            "experience.post_flush",
        ],
    );
    assert_source_order(
        UI_SOURCE,
        &[
            "pub fn render_app",
            "FrameLayoutPlan::for_app(app, area)",
            "render_content(frame, app, plan.content, theme, &plan)",
            "render_overlays(frame, app, theme, &plan)",
        ],
    );

    // act
    // Then: AppState, RuntimeExperience, and all three authored parity families are
    // production-owned seams rather than disconnected public modules.
    for (source, markers) in [
        (
            APP_SOURCE,
            &[
                "ViewportId",
                "ThemeChoice",
                "ThemeFamily",
                "WelcomeState",
                "WelcomeHitMap",
                "pub(crate) fn welcome_hit_map",
            ] as &[&str],
        ),
        (
            RUNTIME_INTEGRATION_SOURCE,
            &["LifecycleState", "pub(crate) struct RuntimeExperience"] as &[&str],
        ),
        (
            LAYOUT_SOURCE,
            &["DESIGN_TOKENS", "pub struct FrameLayoutPlan"] as &[&str],
        ),
        (
            THEME_SOURCE,
            &[
                "ThemeFamily",
                "FallbackLadder",
                "DESIGN_TOKENS",
                "from_family",
            ] as &[&str],
        ),
        (
            UI_LIFECYCLE_SOURCE,
            &["WelcomeLayout", "app.welcome_layout"] as &[&str],
        ),
    ] {
        for marker in markers {
            // assert
            assert!(source.contains(marker), "production source lost {marker:?}");
        }
    }
    assert!(LIB_SOURCE.contains("pub mod design_contract;"));
    assert!(LIB_SOURCE.contains("pub mod theme_family;"));
    assert!(LIB_SOURCE.contains("pub mod welcome_surface;"));
    assert!(LIB_SOURCE.contains("run_tui_with_options"));
}

#[test]
fn startup_and_live_shells_render_through_parity_primitives() {
    // arrange
    // Given: real AppState instances for the startup and live production modes.
    let startup = AppState::new_startup(Vec::new(), None);
    let live = AppState::new_live(None, false, None);

    // When: both surfaces use the shipped ui::render_app/TestBackend seam.
    let startup_buffer = render(&startup, 100, 30);
    let live_buffer = render(&live, 100, 30);

    // Then: the authored theme is painted, the welcome surface is visible, and
    // the live shell owns one transcript/composer surface without a legacy rail.
    assert_eq!(startup.theme_family(), ThemeFamily::Dark);
    assert!(startup_buffer
        .content
        .iter()
        .any(|cell| cell.bg == startup.theme().surface.canvas));
    assert!(startup_buffer
        .content
        .iter()
        .any(|cell| cell.symbol() == "H"));

    // act
    let plan = FrameLayoutPlan::for_app(&live, Rect::new(0, 0, 100, 30));
    // assert
    assert!(plan.transcript.is_some());
    assert!(plan.composer.is_some());
    assert!(plan.operator_sidebar.is_none());
    assert!(live_buffer
        .content
        .iter()
        .any(|cell| cell.bg == live.theme().surface.canvas));
}
