#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner contract tests use direct fail-fast assertions"
)]

use harness_tui::design_contract::{ViewportId, VIEWPORTS};
use harness_tui::shell_geometry::{
    cursor_for, identity_rectangles, layout_for, HitTarget, ShellState, ALL_SHELL_STATES,
};

#[test]
fn shell_geometry_is_deterministic_and_complete_for_every_viewport_and_state() {
    // arrange
    for viewport in ViewportId::ALL {
        for state in ALL_SHELL_STATES {
            let first = layout_for(viewport, state);
            let second = layout_for(viewport, state);
            assert_eq!(
                first, second,
                "{viewport:?}/{state:?} must be deterministic"
            );
            let (width, height) = viewport.dimensions();
            assert_eq!(
                (first.viewport.width, first.viewport.height),
                (width, height)
            );
            assert!(
                first.contains_all_regions(),
                "{viewport:?}/{state:?} must fit"
            );

            // act
            let hit_map = first.hit_map();
            // assert
            assert_eq!(hit_map.regions.len(), 6, "all shell hit regions are named");
            let expected_target = if state.is_overlay() {
                HitTarget::Overlay
            } else {
                HitTarget::Composer
            };
            let point = if state.is_overlay() {
                let overlay = first.overlays[0];
                (overlay.x, overlay.y)
            } else {
                (first.composer.x, first.composer.y)
            };
            assert_eq!(hit_map.hit_test(point.0, point.1), Some(expected_target));
        }
    }
    assert_eq!(VIEWPORTS.len(), 7);
}

#[test]
fn unicode_width_drives_cursor_columns_without_splitting_wide_cells() {
    // arrange
    // act
    let regions = layout_for(ViewportId::Compact40x10, ShellState::Drafting);
    let cursor = cursor_for(
        &regions,
        ShellState::Drafting,
        "A川✅e\u{301}",
        "A川✅e\u{301}".chars().count(),
    );

    // assert
    assert_eq!(cursor.display_column, 6);
    assert!(cursor.position.0 < regions.composer.right());
    assert!(cursor.position.1 < regions.composer.bottom());
    assert!(!cursor.clipped);
}

#[test]
fn every_requested_shell_state_has_state_specific_geometry_and_hit_focus() {
    // arrange
    for state in ALL_SHELL_STATES {
        let regions = layout_for(ViewportId::Default80x24, state);
        let overlay_active = matches!(state, ShellState::Permission | ShellState::Question);
        assert_eq!(regions.overlays.is_empty(), !overlay_active, "{state:?}");

        // act
        let hit_map = regions.hit_map();
        let focus = hit_map
            .regions
            .iter()
            .find(|region| region.target == HitTarget::Composer)
            .expect("composer hit region");
        // assert
        assert_eq!(focus.active, !overlay_active);
        assert_eq!(focus.covered, overlay_active);
    }
}

#[test]
fn harness_identity_substitutions_fit_exact_reference_rectangles() {
    // arrange
    // act
    let copy = harness_tui::shell_geometry::IdentityCopy {
        product: "Harness",
        logo: "◆",
        version: "0.1.0",
        auth: "OAuth/API key",
        model: "mock:model-日本語",
        workspace: "/workspace/日本語",
    };
    let identity = identity_rectangles(ViewportId::Wide132x40, &copy);

    // assert
    assert_eq!(identity.product.width, 7);
    assert_eq!(identity.logo.width, 1);
    assert!(identity.version.x >= identity.product.right());
    assert!(identity.auth.x >= identity.version.right());
    assert!(identity.model.x >= identity.auth.right());
    assert!(identity.workspace.x >= identity.model.right());
    assert!(identity.workspace.right() <= 132);
    assert_eq!(identity.height(), 1);
}

#[test]
fn minimum_viewport_never_clips_focus_or_cursor() {
    // arrange
    // act
    for state in ALL_SHELL_STATES {
        let regions = layout_for(ViewportId::Compact40x10, state);
        let cursor = cursor_for(&regions, state, "川✅", 2);
        // assert
        assert!(cursor.position.0 < 40, "{state:?} cursor column clipped");
        assert!(cursor.position.1 < 10, "{state:?} cursor row clipped");
        assert!(
            !cursor.clipped,
            "{state:?} cursor must clamp into minimum shell"
        );
        for region in regions.hit_map().regions {
            if region.active {
                assert!(region.rect.right() <= 40, "{state:?} focus region clipped");
                assert!(region.rect.bottom() <= 10, "{state:?} focus region clipped");
            }
        }
    }
}

#[test]
fn shell_state_registry_is_exactly_the_requested_nine_states() {
    // arrange
    // act
    // assert
    assert_eq!(
        ALL_SHELL_STATES,
        [
            ShellState::Idle,
            ShellState::Drafting,
            ShellState::Streaming,
            ShellState::Permission,
            ShellState::Question,
            ShellState::Queued,
            ShellState::Cancelling,
            ShellState::Failed,
            ShellState::Completed,
        ]
    );
}
