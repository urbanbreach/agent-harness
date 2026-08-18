use super::*;
use crate::app::modal_interaction::{ModalSurfaceKey, ModalTarget};

pub(super) fn help_mouse_hover_preserves_keyboard_selection_and_click_opens_detail() {
    let mut app = AppState::new_live(None, false, None);
    app.handle_key(key_with_modifiers(
        KeyCode::Char('x'),
        KeyModifiers::CONTROL,
    ));
    let frame_area = Rect::new(0, 0, 100, 30);
    let model = help_model(&app, frame_area);
    let rows = app.help_rows();
    let (index, area) = model
        .regions
        .iter()
        .find_map(|region| match region.target {
            ModalTarget::Row(index)
                if index != app.help_browser.selected
                    && matches!(rows.get(index), Some(crate::app::HelpRow::Shortcut { .. })) =>
            {
                Some((index, region.area))
            }
            ModalTarget::Close
            | ModalTarget::Input
            | ModalTarget::Row(_)
            | ModalTarget::Footer(_) => None,
        })
        .unwrap_or_abort();
    let selected = app.help_browser.selected;

    app.handle_mouse(
        mouse(MouseEventKind::Moved, area),
        frame_area,
        None,
        None,
        None,
    );

    assert_eq!(app.help_browser.selected, selected);
    assert!(app.modal_target_hovered(ModalSurfaceKey::Help, ModalTarget::Row(index)));

    app.handle_mouse(
        mouse(MouseEventKind::Down(MouseButton::Left), area),
        frame_area,
        None,
        None,
        None,
    );

    let expected = rows.get(index).and_then(|row| match row {
        crate::app::HelpRow::Shortcut { action, .. } => Some(*action),
        crate::app::HelpRow::Section { .. } => None,
    });
    assert_eq!(app.help_detail().map(|(action, _)| action), expected);
}

pub(super) fn help_mouse_search_close_and_scroll_use_modal_hit_regions() {
    let mut app = AppState::new_live(None, false, None);
    app.handle_key(key_with_modifiers(
        KeyCode::Char('x'),
        KeyModifiers::CONTROL,
    ));
    let frame_area = Rect::new(0, 0, 100, 30);
    let model = help_model(&app, frame_area);
    let search = region(&model, ModalTarget::Input);

    app.handle_mouse(
        mouse(MouseEventKind::Down(MouseButton::Left), search),
        frame_area,
        None,
        None,
        None,
    );
    assert!(app.help_browser.search_active);

    app.help_browser.search_active = false;
    app.help_browser.collapsed_sections.clear();
    let model = help_model(&app, frame_area);
    app.handle_mouse(
        mouse(MouseEventKind::ScrollDown, model.popup),
        frame_area,
        None,
        None,
        None,
    );
    assert!(app.help_browser.visual_offset > 0);

    app.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    let keyboard_model = help_model(&app, frame_area);
    assert!(keyboard_model.visual_offset > 3);
    app.handle_mouse(
        mouse(MouseEventKind::ScrollUp, keyboard_model.popup),
        frame_area,
        None,
        None,
        None,
    );
    assert_eq!(
        app.help_browser.visual_offset,
        keyboard_model.visual_offset.saturating_sub(3)
    );

    let model = help_model(&app, frame_area);
    let close = region(&model, ModalTarget::Close);
    app.handle_mouse(
        mouse(MouseEventKind::Down(MouseButton::Left), close),
        frame_area,
        None,
        None,
        None,
    );
    assert_eq!(app.review_surface(), None);
}

pub(super) fn help_detail_mouse_wheel_uses_single_row_steps_and_clamps() {
    let mut app = AppState::new_live(None, false, None);
    app.handle_key(key_with_modifiers(
        KeyCode::Char('x'),
        KeyModifiers::CONTROL,
    ));
    app.help_browser.collapsed_sections.clear();
    let action = app
        .help_rows()
        .into_iter()
        .filter_map(|row| match row {
            HelpRow::Shortcut {
                action,
                label,
                description,
                ..
            } if description != label => Some((action, description.len())),
            HelpRow::Section { .. } | HelpRow::Shortcut { .. } => None,
        })
        .max_by_key(|(_, description_len)| *description_len)
        .map(|(action, _)| action)
        .unwrap_or_abort();
    app.help_browser.mode = HelpMode::Detail { action, scroll: 0 };
    let frame_area = Rect::new(0, 0, 60, 17);
    let model = help_model(&app, frame_area);
    assert!(model.max_scroll > 1);

    app.handle_mouse(
        mouse(MouseEventKind::ScrollDown, model.popup),
        frame_area,
        None,
        None,
        None,
    );
    assert_eq!(app.help_detail(), Some((action, 1)));

    for _ in 0..=model.max_scroll {
        app.handle_mouse(
            mouse(MouseEventKind::ScrollDown, model.popup),
            frame_area,
            None,
            None,
            None,
        );
    }
    assert_eq!(app.help_detail(), Some((action, model.max_scroll)));
}

fn help_model(app: &AppState, frame_area: Rect) -> crate::ui::ui_overlays::ModalSurfaceModel {
    crate::ui::ui_overlays::modal_surface_model(app, frame_area).unwrap_or_abort()
}

fn region(model: &crate::ui::ui_overlays::ModalSurfaceModel, target: ModalTarget) -> Rect {
    model
        .regions
        .iter()
        .find_map(|region| (region.target == target).then_some(region.area))
        .unwrap_or_abort()
}

fn mouse(kind: MouseEventKind, area: Rect) -> MouseEvent {
    MouseEvent {
        kind,
        column: area.x.saturating_add(area.width.saturating_sub(1) / 2),
        row: area.y.saturating_add(area.height.saturating_sub(1) / 2),
        modifiers: KeyModifiers::NONE,
    }
}
