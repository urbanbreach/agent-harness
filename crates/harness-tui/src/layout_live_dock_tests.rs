use super::{live_dock_test_fixtures::*, *};
use crate::{render_test::render_to_buffer, ui};

fn rendered_row(buffer: &ratatui::buffer::Buffer, width: u16, y: u16) -> String {
    let start = usize::from(y) * usize::from(width);
    buffer.content[start..start + usize::from(width)]
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

fn assert_blank_row(buffer: &ratatui::buffer::Buffer, width: u16, y: u16, context: &str) {
    assert!(
        rendered_row(buffer, width, y)
            .chars()
            .all(|symbol| symbol == ' '),
        "{context} must be blank at row {}",
        y.saturating_add(1),
    );
}

fn assert_composer_and_footer_cells(
    buffer: &ratatui::buffer::Buffer,
    plan: FrameLayoutPlan,
    expected: ExpectedDockRows,
) {
    let composer = plan.composer.expect("composer");
    let disclosure = plan.disclosure.expect("disclosure");
    assert_eq!(buffer[(composer.x, composer.y)].symbol(), "╭");
    assert_eq!(
        buffer[(composer.x, composer.y.saturating_add(1))].symbol(),
        "│"
    );
    assert_eq!(
        buffer[(composer.x, composer.bottom().saturating_sub(1))].symbol(),
        "╰"
    );
    assert!(rendered_row(buffer, expected.width, disclosure.y).contains("shortcuts"));
    if expected.outer_spacer > 0 {
        assert_blank_row(
            buffer,
            expected.width,
            composer.bottom(),
            "composer/footer spacer",
        );
        assert_blank_row(buffer, expected.width, disclosure.bottom(), "bottom margin");
    }
}

fn assert_active_rows(state: &str, app: &AppState, expected: ExpectedDockRows) {
    let plan = FrameLayoutPlan::for_app(app, Rect::new(0, 0, expected.width, expected.height));
    let status = plan.status.unwrap_or_else(|| {
        panic!(
            "{state} must reserve a status row at {}x{}",
            expected.width, expected.height
        )
    });
    let composer = plan.composer.expect("live composer");
    let disclosure = plan.disclosure.expect("live disclosure");

    assert_eq!(
        status.y, expected.status,
        "{state} status at {}x{}",
        expected.width, expected.height
    );
    assert_eq!(
        status.height, 1,
        "{state} status height at {}x{}",
        expected.width, expected.height
    );
    assert_eq!(
        composer.y,
        expected.composer,
        "{state} composer at {}x{}; runtime={:?}, review={:?}, plan={plan:?}",
        expected.width,
        expected.height,
        app.runtime_state().kind,
        app.review_surface(),
    );
    assert_eq!(
        composer.height, 3,
        "{state} composer height at {}x{}",
        expected.width, expected.height
    );
    assert_eq!(
        disclosure.y, expected.disclosure,
        "{state} disclosure at {}x{}",
        expected.width, expected.height
    );
    assert_eq!(
        disclosure.height, 1,
        "{state} disclosure height at {}x{}",
        expected.width, expected.height
    );
    assert_eq!(
        composer.y.saturating_sub(status.bottom()),
        expected.outer_spacer,
        "{state} status/composer spacer at {}x{}",
        expected.width,
        expected.height
    );
    assert_eq!(
        disclosure.y.saturating_sub(composer.bottom()),
        expected.outer_spacer,
        "{state} composer/disclosure spacer at {}x{}",
        expected.width,
        expected.height
    );
    assert_eq!(
        expected.height.saturating_sub(disclosure.bottom()),
        expected.outer_spacer,
        "{state} bottom margin at {}x{}",
        expected.width,
        expected.height
    );
}

fn assert_terminal_rows(state: &str, app: &AppState, expected: ExpectedDockRows) {
    let plan = FrameLayoutPlan::for_app(app, Rect::new(0, 0, expected.width, expected.height));
    let composer = plan.composer.expect("terminal composer");
    let disclosure = plan.disclosure.expect("terminal disclosure");

    assert!(
        plan.status.is_none(),
        "{state} must not retain active status at {}x{}",
        expected.width,
        expected.height
    );
    assert_eq!(
        composer.y,
        expected.composer,
        "{state} composer at {}x{}; runtime={:?}, review={:?}, plan={plan:?}",
        expected.width,
        expected.height,
        app.runtime_state().kind,
        app.review_surface(),
    );
    assert_eq!(
        disclosure.y, expected.disclosure,
        "{state} disclosure at {}x{}",
        expected.width, expected.height
    );
    assert_eq!(
        disclosure.y.saturating_sub(composer.bottom()),
        expected.outer_spacer,
        "{state} composer/disclosure spacer at {}x{}",
        expected.width,
        expected.height
    );
    assert_eq!(
        expected.height.saturating_sub(disclosure.bottom()),
        expected.outer_spacer,
        "{state} bottom margin at {}x{}",
        expected.width,
        expected.height
    );
}

#[test]
fn active_status_dock_matches_reference_rows_at_all_measured_viewports() {
    for (state, app) in [
        ("waiting", waiting_app()),
        ("parked", parked_app()),
        ("watcher", watcher_app()),
    ] {
        for expected in VIEWPORTS {
            assert_active_rows(state, &app, expected);
        }
    }
}

#[test]
fn permission_suppression_keeps_dedicated_prompt_above_the_composer() {
    let app = permission_app();
    assert!(!app.live_turn_status_visible());

    for expected in VIEWPORTS {
        let plan = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, expected.width, expected.height));
        let permission = plan.status.expect("permission prompt band");
        let composer = plan.composer.expect("permission composer");

        assert_eq!(permission.height, 9);
        assert_eq!(
            composer.y.saturating_sub(permission.bottom()),
            expected.outer_spacer,
            "permission/status suppression spacer at {}x{}",
            expected.width,
            expected.height,
        );
        assert!(plan.disclosure.is_none());
    }
}

#[test]
fn terminal_dock_matches_reference_rows_at_all_measured_viewports() {
    for (state, app) in [
        ("completed", completed_app()),
        ("failed", failed_app()),
        ("cancelled", cancelled_app()),
    ] {
        for expected in VIEWPORTS {
            assert_terminal_rows(state, &app, expected);
        }
    }
}

#[test]
fn rendered_status_and_dock_cells_match_the_normalized_reference_contract() {
    for (state, app, marker, stop_visible) in [
        (
            "waiting",
            interruptible_waiting_app(),
            "Waiting for response…",
            true,
        ),
        (
            "parked",
            parked_app(),
            "waiting · send a message to interrupt",
            false,
        ),
        ("watcher", watcher_app(), "1 command still running", false),
    ] {
        for expected in VIEWPORTS {
            let area = Rect::new(0, 0, expected.width, expected.height);
            let plan = FrameLayoutPlan::for_app(&app, area);
            let status = plan.status.expect("active status");
            let buffer = render_to_buffer(&app, area, |app, frame, _area| {
                ui::render_app(frame, app);
            });
            let status_row = rendered_row(&buffer, expected.width, status.y);
            assert!(
                status_row.contains(marker),
                "{state} at {}x{}: {status_row:?}",
                expected.width,
                expected.height
            );
            assert_eq!(buffer[(status.x, status.y)].symbol(), " ");
            assert_eq!(buffer[(status.x.saturating_add(1), status.y)].symbol(), " ");
            assert_eq!(status_row.contains("[stop]"), stop_visible);
            assert_composer_and_footer_cells(&buffer, plan, expected);
            if expected.outer_spacer > 0 {
                assert_blank_row(
                    &buffer,
                    expected.width,
                    status.bottom(),
                    "status/composer spacer",
                );
            }
        }
    }
}

#[test]
fn rendered_permission_and_terminal_cells_keep_their_state_contracts() {
    let permission = permission_app();
    for expected in VIEWPORTS {
        let area = Rect::new(0, 0, expected.width, expected.height);
        let plan = FrameLayoutPlan::for_app(&permission, area);
        let permission_area = plan.status.expect("permission band");
        let buffer = render_to_buffer(&permission, area, |app, frame, _area| {
            ui::render_app(frame, app);
        });
        let permission_text = (permission_area.y..permission_area.bottom())
            .map(|y| rendered_row(&buffer, expected.width, y))
            .collect::<String>();
        assert!(permission_text.contains("Allow Edit"));
        assert!(!permission_text.contains("Waiting for response"));
        if expected.outer_spacer > 0 {
            assert_blank_row(
                &buffer,
                expected.width,
                permission_area.bottom(),
                "permission/composer spacer",
            );
        }
    }

    for app in [completed_app(), failed_app(), cancelled_app()] {
        for expected in VIEWPORTS {
            let area = Rect::new(0, 0, expected.width, expected.height);
            let plan = FrameLayoutPlan::for_app(&app, area);
            let buffer = render_to_buffer(&app, area, |app, frame, _area| {
                ui::render_app(frame, app);
            });
            assert!(plan.status.is_none());
            assert_composer_and_footer_cells(&buffer, plan, expected);
            if expected.outer_spacer > 0 {
                let composer = plan.composer.expect("terminal composer");
                assert_blank_row(
                    &buffer,
                    expected.width,
                    composer.y.saturating_sub(1),
                    "terminal prompt gap",
                );
            }
        }
    }
}
