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

#[test]
fn permission_suppression_replaces_the_composer_with_its_dedicated_prompt() {
    let app = permission_app();
    assert!(!app.live_turn_status_visible());

    for expected in VIEWPORTS {
        let plan = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, expected.width, expected.height));
        let permission = plan.status.expect("permission prompt band");
        let composer = plan.composer.expect("permission composer");

        assert_eq!(permission.height, 9 + QUESTION_OUTER_FOOTER_ROWS);
        assert_eq!(composer.height, 0);
        assert_eq!(
            composer.y.saturating_sub(permission.bottom()),
            0,
            "permission prompt owns the composer slot at {}x{}",
            expected.width,
            expected.height,
        );
        assert!(plan.disclosure.is_none());
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
                permission_area.bottom().saturating_sub(1),
                "permission footer bottom margin",
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
