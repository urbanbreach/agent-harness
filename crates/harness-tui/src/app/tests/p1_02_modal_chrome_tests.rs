use super::*;
use crate::app::modal_interaction::ModalTarget;

const VIEWPORTS: [(u16, u16, Rect); 3] = [
    (80, 24, Rect::new(0, 0, 80, 24)),
    (120, 40, Rect::new(16, 6, 88, 28)),
    (160, 50, Rect::new(36, 11, 88, 28)),
];

#[test]
fn p1_02_settings_chrome_renders_exactly_at_canonical_viewports() {
    // arrange
    let mut app = command_palette_snapshot();
    app.execute_action(Action::OpenSettings);

    // act
    for (width, height, expected_popup) in VIEWPORTS {
        let area = Rect::new(0, 0, width, height);
        let model = modal_model(&app, area);
        let rows = rendered_rows(&app, width, height);
        let cells = |row: u16, column: u16, width: usize| {
            rows[usize::from(row)]
                .chars()
                .skip(usize::from(column))
                .take(width)
                .collect::<String>()
        };

        // assert
        assert_eq!(model.popup, expected_popup, "viewport={width}x{height}");
        assert_eq!(
            cells(model.popup.y, model.popup.x, 30),
            "┌─ Settings ──────────────────"
        );
        assert_eq!(
            cells(model.popup.y + 1, model.popup.x + 2, 19),
            "Commands / Settings"
        );
        assert_eq!(
            cells(model.popup.y + 2, model.popup.x + 2, 15),
            "[Runtime]  TUI "
        );
        assert!(
            rows[usize::from(model.popup.bottom() - 2)]
                .contains("↑/↓ navigate · Enter edit · Esc close"),
            "viewport={width}x{height}"
        );
    }
}

#[test]
fn p1_02_modal_surfaces_share_title_footer_and_six_cell_close_metadata() {
    // arrange
    let mut settings = AppState::new_live(None, false, None);
    settings.execute_action(Action::OpenSettings);
    let mut help = AppState::new_live(None, false, None);
    help.handle_key(key_with_modifiers(
        KeyCode::Char('x'),
        KeyModifiers::CONTROL,
    ));
    let mut model = AppState::new_live(None, false, None);
    model.open_model_switcher();
    let mut commands = AppState::new_live(None, false, None);
    commands.open_palette();
    let surfaces = [
        (&settings, "Settings"),
        (&help, "Keyboard Shortcuts"),
        (&model, "Models"),
        (&commands, "Commands"),
    ];

    // act
    for (app, title) in surfaces {
        let model = modal_model(app, Rect::new(0, 0, 120, 40));
        let rows = rendered_rows(app, 120, 40);
        let close = region(&model, ModalTarget::Close);

        // assert
        assert!(
            rows[usize::from(model.popup.y)].contains(title),
            "title={title}"
        );
        assert!(
            rows[usize::from(model.popup.bottom() - 2)].contains("Esc close"),
            "title={title}"
        );
        assert_eq!(
            close,
            Rect::new(model.popup.right() - 6, model.popup.y, 6, 1)
        );
    }
}

#[test]
fn p1_02_settings_tabs_switch_and_filter_runtime_and_tui_rows() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    app.execute_action(Action::OpenSettings);
    assert!(!app.settings_editor_rows().is_empty());
    assert!(app
        .settings_editor_rows()
        .iter()
        .all(|row| row.surface == "runtime"));

    // act
    app.handle_key(key(KeyCode::Tab));

    // assert
    let tui_rows = app.settings_editor_rows();
    assert!(!tui_rows.is_empty());
    assert!(tui_rows.iter().all(|row| row.surface == "tui"));
    assert!(render_text(&app, 120, 40).contains("Runtime  [TUI]"));

    // act
    app.handle_key(key_with_modifiers(KeyCode::BackTab, KeyModifiers::SHIFT));

    // assert
    assert!(app
        .settings_editor_rows()
        .iter()
        .all(|row| row.surface == "runtime"));
    assert_eq!(app.settings_editor_selected_index(), 0);
}

#[test]
fn p1_02_settings_owns_cursor_until_commands_is_restored() {
    for (width, height, _) in VIEWPORTS {
        // arrange
        let mut app = command_palette_snapshot();
        app.focus = Focus::Prompt;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        terminal
            .draw(|frame| render_app(frame, &app))
            .expect("render Commands");
        assert!(terminal.backend().cursor_visible());

        // act
        app.execute_action(Action::OpenSettings);
        terminal
            .draw(|frame| render_app(frame, &app))
            .expect("render Settings");

        // assert
        assert!(
            !terminal.backend().cursor_visible(),
            "viewport={width}x{height}"
        );

        // act
        app.handle_key(key(KeyCode::Esc));
        terminal
            .draw(|frame| render_app(frame, &app))
            .expect("render restored Commands");

        // assert
        assert!(terminal.backend().cursor_visible());
    }
}

#[test]
fn p1_02_settings_escape_restores_exact_command_palette_snapshot() {
    // arrange
    let mut app = command_palette_snapshot();
    let expected_input = app.palette_input.clone();
    let expected_filtered = app.palette_filtered.clone();
    let expected_selected = app.palette_selected;
    app.execute_action(Action::OpenSettings);

    // act
    app.handle_key(key(KeyCode::Esc));

    // assert
    assert!(app.palette_visible);
    assert_eq!(app.overlay_stack().top(), Some(OverlayKind::CommandPalette));
    assert_eq!(app.palette_input, expected_input);
    assert_eq!(app.palette_filtered, expected_filtered);
    assert_eq!(app.palette_selected, expected_selected);
    assert_eq!(app.focus, Focus::List);
}

#[test]
fn p1_02_settings_close_click_restores_exact_command_palette_snapshot() {
    // arrange
    let mut app = command_palette_snapshot();
    let expected_input = app.palette_input.clone();
    let expected_selected = app.palette_selected;
    app.execute_action(Action::OpenSettings);
    let area = Rect::new(0, 0, 120, 40);
    let close = region(&modal_model(&app, area), ModalTarget::Close);

    // act
    app.handle_mouse(
        mouse(MouseEventKind::Down(MouseButton::Left), close),
        area,
        None,
        None,
        None,
    );
    app.handle_mouse(
        mouse(MouseEventKind::Up(MouseButton::Left), close),
        area,
        None,
        None,
        None,
    );

    // assert
    assert!(app.palette_visible);
    assert_eq!(app.palette_input, expected_input);
    assert_eq!(app.palette_selected, expected_selected);
    assert_eq!(app.focus, Focus::List);
}

#[test]
fn p1_02_stale_or_mismatched_outside_release_cannot_dismiss_settings() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    app.execute_action(Action::OpenSettings);
    let area = Rect::new(0, 0, 120, 40);
    let popup = modal_model(&app, area).popup;
    let outside = Rect::new(0, 0, 1, 1);
    app.handle_mouse(
        mouse(MouseEventKind::Down(MouseButton::Left), outside),
        area,
        None,
        None,
        None,
    );

    // act
    app.handle_mouse(
        mouse(MouseEventKind::Up(MouseButton::Left), popup),
        area,
        None,
        None,
        None,
    );

    // assert
    assert!(app.settings_editor_is_visible());
    assert_eq!(app.overlay_stack().top(), Some(OverlayKind::SettingsEditor));
}

#[test]
fn p1_02_dense_models_reserve_the_shared_footer_row_from_hits() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    let models = (0..64)
        .map(|index| ModelOption::from_model_ref("build", &format!("mock:model-{index}")))
        .collect::<Vec<_>>();
    app.set_launch_metadata(
        LaunchMetadata::from_model_option(&models[0]).with_available_models(models),
    );
    app.open_model_switcher();

    // act
    let model = modal_model(&app, Rect::new(0, 0, 80, 24));
    let footer_y = model.popup.bottom().saturating_sub(2);

    // assert
    assert!(
        model
            .regions
            .iter()
            .filter(|region| matches!(region.target, ModalTarget::Row(_)))
            .all(|region| region.area.bottom() <= footer_y),
        "row hit regions must end before footer row {footer_y}: {model:?}"
    );
}

#[test]
fn p1_02_settings_restores_commands_visual_scroll_offset() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    app.open_palette();
    let area = Rect::new(0, 0, 80, 24);
    let popup = modal_model(&app, area).popup;
    for _ in 0..5 {
        app.handle_mouse(
            mouse(MouseEventKind::ScrollDown, popup),
            area,
            None,
            None,
            None,
        );
    }
    let expected_offset = modal_model(&app, area).visual_offset;
    assert!(expected_offset > 0);
    app.execute_action(Action::OpenSettings);
    app.handle_key(key(KeyCode::Tab));

    // act
    app.handle_key(key(KeyCode::Esc));

    // assert
    assert_eq!(modal_model(&app, area).visual_offset, expected_offset);
}

fn command_palette_snapshot() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::List;
    app.open_palette();
    for character in "set".chars() {
        app.handle_key(key(KeyCode::Char(character)));
    }
    app.handle_key(key(KeyCode::Down));
    app
}

fn modal_model(app: &AppState, area: Rect) -> crate::ui::ui_overlays::ModalSurfaceModel {
    crate::ui::ui_overlays::modal_surface_model(app, area).expect("modal surface model")
}

fn region(model: &crate::ui::ui_overlays::ModalSurfaceModel, target: ModalTarget) -> Rect {
    model
        .regions
        .iter()
        .find_map(|region| (region.target == target).then_some(region.area))
        .expect("modal hit region")
}

fn rendered_rows(app: &AppState, width: u16, height: u16) -> Vec<String> {
    render_text(app, width, height)
        .lines()
        .map(str::to_owned)
        .collect()
}

fn mouse(kind: MouseEventKind, area: Rect) -> MouseEvent {
    MouseEvent {
        kind,
        column: area.x + area.width.saturating_sub(1) / 2,
        row: area.y + area.height.saturating_sub(1) / 2,
        modifiers: KeyModifiers::NONE,
    }
}
