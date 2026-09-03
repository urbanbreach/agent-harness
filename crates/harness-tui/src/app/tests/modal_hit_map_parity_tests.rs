use super::*;
use crate::app::modal_interaction::ModalTarget;

const VIEWPORTS: [(u16, u16); 3] = [(80, 24), (120, 40), (160, 50)];

fn modal_model(app: &AppState, area: Rect) -> crate::ui::ui_overlays::ModalSurfaceModel {
    crate::ui::ui_overlays::modal_surface_model(app, area).unwrap_or_abort()
}

fn close_region(model: &crate::ui::ui_overlays::ModalSurfaceModel) -> Rect {
    model
        .regions
        .iter()
        .find_map(|region| (region.target == ModalTarget::Close).then_some(region.area))
        .unwrap_or_abort()
}

fn regions_overlap(first: Rect, second: Rect) -> bool {
    first.x < second.right()
        && second.x < first.right()
        && first.y < second.bottom()
        && second.y < first.bottom()
}

fn assert_close_and_rows_are_valid(model: &crate::ui::ui_overlays::ModalSurfaceModel) {
    let close = close_region(model);
    assert_eq!(
        close,
        Rect::new(model.popup.right() - 6, model.popup.y, 6, 1)
    );
    let rows = model
        .regions
        .iter()
        .filter(|region| matches!(region.target, ModalTarget::Row(_)))
        .collect::<Vec<_>>();
    assert!(rows.iter().all(|region| {
        region.area.x >= model.popup.x
            && region.area.y >= model.popup.y
            && region.area.right() <= model.popup.right()
            && region.area.bottom() <= model.popup.bottom()
    }));
    for (index, first) in rows.iter().enumerate() {
        assert!(rows
            .iter()
            .skip(index + 1)
            .all(|second| !regions_overlap(first.area, second.area)));
    }
}

pub(super) fn settings_and_help_hit_maps_are_bounded_at_canonical_viewports() {
    // arrange
    for (width, height) in VIEWPORTS {
        let area = Rect::new(0, 0, width, height);
        let mut settings = AppState::new_live(None, false, None);
        settings.execute_action(Action::OpenSettings);
        let mut help = AppState::new_live(None, false, None);
        help.execute_action(Action::Help);

        // act
        let settings_model = modal_model(&settings, area);
        let help_model = modal_model(&help, area);

        // assert
        assert_close_and_rows_are_valid(&settings_model);
        assert_close_and_rows_are_valid(&help_model);
    }
}

pub(super) fn settings_and_help_close_restore_focus_at_canonical_viewports() {
    // arrange
    for (width, height) in VIEWPORTS {
        let area = Rect::new(0, 0, width, height);
        let mut settings = AppState::new_live(None, false, None);
        settings.focus = Focus::Details;
        settings.execute_action(Action::OpenSettings);
        let settings_close = close_region(&modal_model(&settings, area));
        let mut help = AppState::new_live(None, false, None);
        help.focus = Focus::Prompt;
        help.execute_action(Action::Help);
        let help_close = close_region(&modal_model(&help, area));

        // act
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            settings.handle_mouse(mouse(kind, settings_close), area, None, None, None);
            help.handle_mouse(mouse(kind, help_close), area, None, None, None);
        }

        // assert
        assert_eq!(settings.focus, Focus::Details, "viewport={width}x{height}");
        assert_eq!(help.focus, Focus::Prompt, "viewport={width}x{height}");
    }
}

#[test]
#[ignore = "product gap: settings tabs render without ModalTarget tab variants or tab hit regions"]
fn settings_tab_hit_regions_are_non_overlapping_and_inside_modal_frame() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    app.execute_action(Action::OpenSettings);

    for (width, height) in VIEWPORTS {
        let model = modal_model(&app, Rect::new(0, 0, width, height));
        let tab_row = model.popup.y.saturating_add(2);

        // act
        let tab_regions = model
            .regions
            .iter()
            .filter(|region| region.area.y == tab_row)
            .collect::<Vec<_>>();

        // assert
        assert_eq!(tab_regions.len(), 2, "viewport={width}x{height}");
        assert!(tab_regions.iter().all(|region| {
            region.area.x > model.popup.x && region.area.right() < model.popup.right()
        }));
        assert!(!regions_overlap(tab_regions[0].area, tab_regions[1].area));
    }
}

fn mouse(kind: MouseEventKind, area: Rect) -> MouseEvent {
    MouseEvent {
        kind,
        column: area.x + area.width.saturating_sub(1) / 2,
        row: area.y + area.height.saturating_sub(1) / 2,
        modifiers: KeyModifiers::NONE,
    }
}
